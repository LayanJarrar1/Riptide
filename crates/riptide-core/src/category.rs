//! File categories by extension (Archives, Documents, Music, …).
//!
//! The engine assigns a [`Category`] to every download from its file name and, when the user
//! configured a folder for that category, saves the file there. The keys returned by
//! [`Category::key`] are the ones the store seeds in its `categories` table and the ones the
//! desktop UI translates (`category.<key>`).
//!
//! The extension table here is the single source of truth on the Rust side. The desktop UI
//! keeps an identical copy in `apps/desktop/src/app/model/download.ts` (`CATEGORY_EXTENSIONS`)
//! because it classifies names before the engine is asked; the `extension_table_is_pinned`
//! test below fails whenever this table changes, as a reminder to change the UI copy (and its
//! spec) in the same commit.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::filename::split_extension;

/// A file category. Stable, ordered the way the UI lists them.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Archives and disk images.
    Compressed,
    /// Office documents, PDFs, e-books, plain text.
    Documents,
    /// Audio.
    Music,
    /// Video containers.
    Video,
    /// Installers and packages.
    Programs,
    /// Raster and vector images.
    Images,
    /// Everything else (no extension, or an unknown one).
    Other,
}

impl Category {
    /// Every category in UI order; [`Category::Other`] last.
    pub const ALL: [Category; 7] = [
        Category::Compressed,
        Category::Documents,
        Category::Music,
        Category::Video,
        Category::Programs,
        Category::Images,
        Category::Other,
    ];

    /// Stable lower-case key: the `categories.key` column, the `category_dirs` settings key and
    /// the UI's `CategoryKey`.
    pub const fn key(self) -> &'static str {
        match self {
            Category::Compressed => "compressed",
            Category::Documents => "documents",
            Category::Music => "music",
            Category::Video => "video",
            Category::Programs => "programs",
            Category::Images => "images",
            Category::Other => "other",
        }
    }

    /// Inverse of [`Self::key`].
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }

    /// Lower-case extensions (no dot) that map to this category. Empty for `Other`.
    ///
    /// Must stay identical to `CATEGORY_EXTENSIONS` in `apps/desktop/src/app/model/download.ts`
    /// and to the seed in `riptide-store/src/sql/v1.sql`.
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Category::Compressed => &[
                "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "iso", "cab",
            ],
            Category::Documents => &[
                "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "txt",
                "rtf", "epub", "mobi", "csv",
            ],
            Category::Music => &[
                "mp3", "flac", "wav", "aac", "m4a", "ogg", "opus", "wma", "aiff",
            ],
            Category::Video => &[
                "mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "ts", "flv", "mpg", "mpeg",
            ],
            Category::Programs => &[
                "exe", "msi", "msix", "appx", "apk", "dmg", "pkg", "deb", "rpm", "appimage",
            ],
            Category::Images => &[
                "jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "tif", "tiff", "heic", "avif",
                "psd",
            ],
            Category::Other => &[],
        }
    }

    /// Category of a bare extension (with or without a leading dot, any case).
    pub fn from_extension(ext: &str) -> Self {
        let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        if ext.is_empty() {
            return Category::Other;
        }
        Self::ALL
            .into_iter()
            .find(|c| c.extensions().contains(&ext.as_str()))
            .unwrap_or(Category::Other)
    }

    /// Category of a file name, by its extension. Only the last extension counts, so
    /// `backup.tar.gz` is `Compressed` (`gz`) and `notes.txt.exe` is `Programs`.
    ///
    /// ```
    /// use riptide_core::category::Category;
    /// assert_eq!(Category::from_filename("ubuntu-24.04.iso"), Category::Compressed);
    /// assert_eq!(Category::from_filename("التقرير.PDF"), Category::Documents);
    /// assert_eq!(Category::from_filename("README"), Category::Other);
    /// ```
    pub fn from_filename(filename: &str) -> Self {
        match split_extension(filename) {
            (_, Some(ext)) => Self::from_extension(ext),
            (_, None) => Category::Other,
        }
    }

    /// Category of a `Content-Type` value (parameters and case ignored), for a file whose name
    /// says nothing. `None` for types that do not tell (`application/octet-stream`, unknown).
    ///
    /// ```
    /// use riptide_core::category::Category;
    /// assert_eq!(Category::from_mime("video/mp4"), Some(Category::Video));
    /// assert_eq!(Category::from_mime("application/PDF; charset=binary"), Some(Category::Documents));
    /// assert_eq!(Category::from_mime("application/octet-stream"), None);
    /// ```
    pub fn from_mime(content_type: &str) -> Option<Self> {
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let (kind, sub) = mime.split_once('/')?;
        Some(match (kind, sub) {
            ("video", _) => Category::Video,
            ("audio", _) => Category::Music,
            ("image", _) => Category::Images,
            ("text", "plain" | "csv" | "rtf") => Category::Documents,
            ("application", sub) => {
                match sub {
                    "pdf" | "msword" | "rtf" | "epub+zip" | "vnd.ms-excel"
                    | "vnd.ms-powerpoint" => Category::Documents,
                    s if s.starts_with("vnd.openxmlformats-officedocument.")
                        || s.starts_with("vnd.oasis.opendocument.") =>
                    {
                        Category::Documents
                    }
                    "zip" | "x-zip-compressed" | "x-7z-compressed" | "x-rar-compressed"
                    | "vnd.rar" | "gzip" | "x-gzip" | "x-tar" | "x-bzip2" | "x-xz" | "zstd"
                    | "x-iso9660-image" => Category::Compressed,
                    "x-msdownload"
                    | "x-msi"
                    | "x-ms-installer"
                    | "vnd.microsoft.portable-executable"
                    | "x-dosexec"
                    | "vnd.android.package-archive"
                    | "x-apple-diskimage"
                    | "vnd.debian.binary-package"
                    | "x-rpm"
                    | "msix"
                    | "appx" => Category::Programs,
                    _ => return None,
                }
            }
            _ => return None,
        })
    }

    /// Category of a download: the file name's extension decides; only when it says nothing
    /// (`Other`) does the server's `Content-Type` get a say. The name wins because the server's
    /// type is often generic or wrong, and the name is what the user sees.
    pub fn detect(filename: &str, content_type: Option<&str>) -> Self {
        match Self::from_filename(filename) {
            Category::Other => content_type
                .and_then(Self::from_mime)
                .unwrap_or(Category::Other),
            known => known,
        }
    }

    /// Name of this category's folder when downloads are organised into folders (see the
    /// engine's `organize` setting). English, like the other folders of a Windows profile.
    pub const fn folder_name(self) -> &'static str {
        match self {
            Category::Compressed => "Archives",
            Category::Documents => "Documents",
            Category::Music => "Audio",
            Category::Video => "Videos",
            Category::Programs => "Applications",
            Category::Images => "Images",
            Category::Other => "Other",
        }
    }
}

/// Folder name for torrents when downloads are organised into folders.
pub const TORRENTS_FOLDER: &str = "Torrents";

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn mime_decides_only_when_the_name_does_not() {
        assert_eq!(
            Category::detect("clip.mp4", Some("application/zip")),
            Category::Video
        );
        assert_eq!(
            Category::detect("download", Some("application/pdf")),
            Category::Documents
        );
        assert_eq!(
            Category::detect("setup", Some("application/x-msdownload")),
            Category::Programs
        );
        assert_eq!(
            Category::detect("data.bin", Some("application/octet-stream")),
            Category::Other
        );
        assert_eq!(Category::detect("x", None), Category::Other);
        assert_eq!(
            Category::detect("song", Some("AUDIO/MPEG")),
            Category::Music
        );
        assert_eq!(
            Category::detect(
                "report",
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
            ),
            Category::Documents
        );
        assert_eq!(Category::from_mime("garbage"), None);
        assert_eq!(Category::from_mime("text/html"), None);
    }

    #[test]
    fn every_category_has_a_distinct_folder() {
        let mut names: Vec<_> = Category::ALL.iter().map(|c| c.folder_name()).collect();
        names.push(TORRENTS_FOLDER);
        let n = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    use super::*;

    /// A literal copy of the table. If this test fails you changed `extensions()`: update the
    /// UI copy in `apps/desktop/src/app/model/download.ts` (and `category.spec.ts` next to it)
    /// and the store seed, then this literal.
    #[test]
    fn extension_table_is_pinned() {
        let pinned: [(Category, &str); 7] = [
            (
                Category::Compressed,
                "zip,rar,7z,tar,gz,tgz,bz2,xz,zst,iso,cab",
            ),
            (
                Category::Documents,
                "pdf,doc,docx,xls,xlsx,ppt,pptx,odt,ods,odp,txt,rtf,epub,mobi,csv",
            ),
            (Category::Music, "mp3,flac,wav,aac,m4a,ogg,opus,wma,aiff"),
            (
                Category::Video,
                "mp4,mkv,avi,mov,wmv,webm,m4v,ts,flv,mpg,mpeg",
            ),
            (
                Category::Programs,
                "exe,msi,msix,appx,apk,dmg,pkg,deb,rpm,appimage",
            ),
            (
                Category::Images,
                "jpg,jpeg,png,gif,webp,bmp,svg,tif,tiff,heic,avif,psd",
            ),
            (Category::Other, ""),
        ];
        for (i, (category, list)) in pinned.iter().enumerate() {
            assert_eq!(Category::ALL[i], *category, "order is part of the contract");
            assert_eq!(
                category.extensions().join(","),
                *list,
                "{category}: the extension list changed; update the UI copy too"
            );
        }
    }

    #[test]
    fn every_extension_belongs_to_exactly_one_category_and_is_normalised() {
        let mut seen = std::collections::HashSet::new();
        for c in Category::ALL {
            for ext in c.extensions() {
                assert_eq!(*ext, ext.to_ascii_lowercase(), "{ext} is not lower-case");
                assert!(!ext.starts_with('.'), "{ext} has a dot");
                assert!(seen.insert(*ext), "{ext} is listed twice");
                assert_eq!(Category::from_extension(ext), c);
            }
        }
    }

    #[test]
    fn keys_round_trip_and_match_the_ui_and_store_keys() {
        let keys: Vec<&str> = Category::ALL.iter().map(|c| c.key()).collect();
        assert_eq!(
            keys,
            [
                "compressed",
                "documents",
                "music",
                "video",
                "programs",
                "images",
                "other"
            ]
        );
        for c in Category::ALL {
            assert_eq!(Category::from_key(c.key()), Some(c));
            assert_eq!(c.to_string(), c.key());
            assert_eq!(serde_json::to_value(c).unwrap(), c.key());
        }
        assert_eq!(Category::from_key("archives"), None);
    }

    #[test]
    fn filenames_classify_by_last_extension_case_insensitively() {
        assert_eq!(
            Category::from_filename("ubuntu-24.04.iso"),
            Category::Compressed
        );
        assert_eq!(
            Category::from_filename("backup.tar.gz"),
            Category::Compressed
        );
        assert_eq!(Category::from_filename("التقرير.PDF"), Category::Documents);
        assert_eq!(Category::from_filename("song.FLAC"), Category::Music);
        assert_eq!(Category::from_filename("movie.mkv"), Category::Video);
        assert_eq!(Category::from_filename("setup.exe"), Category::Programs);
        assert_eq!(Category::from_filename("photo.jpeg"), Category::Images);
        assert_eq!(Category::from_filename("README"), Category::Other);
        assert_eq!(Category::from_filename(".bashrc"), Category::Other);
        assert_eq!(Category::from_filename("weird.xyz"), Category::Other);
        assert_eq!(Category::from_filename("trailing."), Category::Other);
        assert_eq!(Category::from_extension(".ZIP"), Category::Compressed);
        assert_eq!(Category::from_extension(""), Category::Other);
    }
}
