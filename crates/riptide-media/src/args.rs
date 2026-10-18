//! The yt-dlp command lines, built from validated values only.
//!
//! Every run starts with [`COMMON`]: `--ignore-config` (a user's own yt-dlp config must not
//! change what Riptide does), `--no-playlist`, UTF-8 output. Nothing here ever adds a flag that
//! plays or keeps DRM-protected formats (`--allow-unplayable-formats`); the test
//! `no_drm_bypass_flag_is_ever_passed` pins that.

use std::ffi::OsString;
use std::path::Path;

use riptide_core::MediaSelection;
use url::Url;

use crate::{Error, Result};

/// Flags every run gets.
pub const COMMON: &[&str] = &[
    "--ignore-config",
    "--no-playlist",
    "--encoding",
    "utf-8",
    "--no-color",
];

/// Flags that would bypass DRM or keep unplayable formats. Never passed.
pub const FORBIDDEN: &[&str] = &["--allow-unplayable-formats", "--no-check-formats"];

/// `--progress-template` for the download phase; parsed by
/// [`crate::parse::parse_progress_line`].
pub const PROGRESS_TEMPLATE: &str = "download:RIPTIDE-P %(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.format_id)s";

/// `--print` template naming the finished file; parsed by [`crate::parse::parse_file_line`].
pub const FILE_TEMPLATE: &str = "after_move:RIPTIDE-F %(filepath)s";

/// Accepts only absolute `http(s)` URLs (no credentials in them, nothing that could pass for a
/// flag).
pub fn check_url(raw: &str) -> Result<Url> {
    let raw = raw.trim();
    if raw.starts_with('-') {
        return Err(Error::InvalidUrl);
    }
    let url = Url::parse(raw).map_err(|_| Error::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(Error::InvalidUrl);
    }
    Ok(url)
}

/// `yt-dlp -J` for one page.
pub fn probe_args(url: &Url, cookies: Option<&Path>) -> Vec<OsString> {
    let mut a: Vec<OsString> = COMMON.iter().map(OsString::from).collect();
    a.push("-J".into());
    if let Some(c) = cookies {
        a.push("--cookies".into());
        a.push(c.into());
    }
    a.push("--".into());
    a.push(url.as_str().into());
    a
}

/// Everything a download run needs.
#[derive(Debug, Clone)]
pub struct RunArgs<'a> {
    /// Page URL (already checked).
    pub url: &'a Url,
    /// Quality.
    pub selection: MediaSelection,
    /// Destination folder.
    pub dir: &'a Path,
    /// File name without extension (already sanitised; `%` is escaped here).
    pub stem: &'a str,
    /// Bundled ffmpeg, if any.
    pub ffmpeg: Option<&'a Path>,
    /// Temporary cookie file, if any.
    pub cookies: Option<&'a Path>,
    /// `--limit-rate` in bytes per second.
    pub rate_limit: Option<u64>,
    /// `--proxy` URL.
    pub proxy: Option<&'a str>,
}

/// The format selector and sort for a quality. Video prefers MP4/M4A streams so the merged file
/// plays everywhere; `-S res:N` takes the largest resolution not above N.
pub fn selection_args(selection: MediaSelection) -> Vec<OsString> {
    let v: Vec<String> = match selection {
        MediaSelection::Best => vec![
            "-f".into(),
            "bv*+ba/b".into(),
            "-S".into(),
            "res,fps,ext:mp4:m4a".into(),
            "--merge-output-format".into(),
            "mp4".into(),
        ],
        MediaSelection::MaxHeight(h) => vec![
            "-f".into(),
            "bv*+ba/b".into(),
            "-S".into(),
            format!("res:{h},fps,ext:mp4:m4a"),
            "--merge-output-format".into(),
            "mp4".into(),
        ],
        MediaSelection::AudioM4a => vec![
            "-f".into(),
            "ba[ext=m4a]/ba/b".into(),
            "-x".into(),
            "--audio-format".into(),
            "m4a".into(),
        ],
        MediaSelection::AudioMp3 => vec![
            "-f".into(),
            "ba/b".into(),
            "-x".into(),
            "--audio-format".into(),
            "mp3".into(),
            "--audio-quality".into(),
            "0".into(),
        ],
    };
    v.into_iter().map(OsString::from).collect()
}

/// The download command line.
pub fn download_args(r: &RunArgs<'_>) -> Vec<OsString> {
    let mut a: Vec<OsString> = COMMON.iter().map(OsString::from).collect();
    for f in [
        "--newline",
        "--progress",
        "--no-simulate",
        "--continue",
        "--no-mtime",
        "--no-write-info-json",
        "--no-write-thumbnail",
    ] {
        a.push(f.into());
    }
    a.push("--progress-template".into());
    a.push(PROGRESS_TEMPLATE.into());
    a.push("--print".into());
    a.push(FILE_TEMPLATE.into());
    if let Some(f) = r.ffmpeg {
        a.push("--ffmpeg-location".into());
        a.push(f.into());
    }
    if let Some(c) = r.cookies {
        a.push("--cookies".into());
        a.push(c.into());
    }
    if let Some(limit) = r.rate_limit.filter(|l| *l > 0) {
        a.push("--limit-rate".into());
        a.push(limit.to_string().into());
    }
    if let Some(p) = r.proxy.filter(|p| !p.is_empty()) {
        a.push("--proxy".into());
        a.push(p.into());
    }
    a.extend(selection_args(r.selection));
    a.push("-P".into());
    a.push(r.dir.into());
    a.push("-o".into());
    a.push(format!("{}.%(ext)s", r.stem.replace('%', "%%")).into());
    a.push("--".into());
    a.push(r.url.as_str().into());
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn urls_must_be_http_and_cannot_pose_as_flags() {
        assert!(check_url("https://www.youtube.com/watch?v=x").is_ok());
        assert!(check_url(" http://vimeo.com/1 ").is_ok());
        for bad in [
            "--exec calc",
            "-o C:/x",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "ftp://x/y",
            "notaurl",
        ] {
            assert!(check_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn download_args_end_with_the_url_after_a_separator() {
        let url = check_url("https://www.youtube.com/watch?v=aqz-KE-bpKQ").unwrap();
        let r = RunArgs {
            url: &url,
            selection: MediaSelection::MaxHeight(720),
            dir: Path::new("C:/dl"),
            stem: "100% Bunny",
            ffmpeg: Some(Path::new("C:/app/ffmpeg.exe")),
            cookies: Some(Path::new("C:/tmp/c.txt")),
            rate_limit: Some(1000),
            proxy: Some("socks5://127.0.0.1:9050"),
        };
        let a = strings(&download_args(&r));
        let n = a.len();
        assert_eq!(a[n - 2], "--");
        assert_eq!(a[n - 1], "https://www.youtube.com/watch?v=aqz-KE-bpKQ");
        assert!(a.contains(&"100%% Bunny.%(ext)s".to_string()));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "-S" && w[1] == "res:720,fps,ext:mp4:m4a"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--ffmpeg-location" && w[1] == "C:/app/ffmpeg.exe"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--limit-rate" && w[1] == "1000"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--merge-output-format" && w[1] == "mp4"));
        assert!(a.contains(&"--ignore-config".to_string()));
        assert!(a.contains(&"--continue".to_string()));
    }

    #[test]
    fn no_drm_bypass_flag_is_ever_passed() {
        let url = check_url("https://x.test/v").unwrap();
        for selection in [
            MediaSelection::Best,
            MediaSelection::MaxHeight(1080),
            MediaSelection::AudioM4a,
            MediaSelection::AudioMp3,
        ] {
            let r = RunArgs {
                url: &url,
                selection,
                dir: Path::new("/tmp"),
                stem: "v",
                ffmpeg: None,
                cookies: None,
                rate_limit: None,
                proxy: None,
            };
            for list in [download_args(&r), probe_args(&url, None)] {
                let s = strings(&list);
                for f in FORBIDDEN {
                    assert!(!s.iter().any(|a| a == f), "{f} in {s:?}");
                }
            }
        }
        let a = strings(&selection_args(MediaSelection::AudioMp3));
        assert!(a.contains(&"-x".to_string()) && a.contains(&"mp3".to_string()));
    }
}
