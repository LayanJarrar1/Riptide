//! Internet Archive (<https://archive.org/developers/>): text items that declare a Creative
//! Commons or public-domain licence and are not access-restricted (no lending-library scans;
//! unlicensed uploads are not offered, since their copyright status is unknown). Two steps: the advanced search finds items,
//! then each item's metadata names the PDF file that gets downloaded.
//!
//! Music and video searches (`search_in`) follow the same two steps with stricter
//! collection rules, see [`AUDIO_FILTER`] and [`VIDEO_FILTER`].

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{jstr, tidy, year_of, Category, Hit, Source};

/// Internet Archive adapter.
pub struct InternetArchive {
    base: String,
    http: Http,
}

impl InternetArchive {
    /// Talks to `archive.org`.
    pub fn new() -> Self {
        Self::with_base("https://archive.org", client_for(Source::InternetArchive))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        InternetArchive {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for InternetArchive {
    fn default() -> Self {
        Self::new()
    }
}

/// One advanced-search row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Doc {
    pub identifier: String,
    pub title: String,
    pub creator: Option<String>,
    pub year: Option<u16>,
}

fn as_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Array(a) => a.iter().find_map(as_text),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub(crate) fn parse_search(body: &Value) -> Vec<Doc> {
    body.pointer("/response/docs")
        .and_then(Value::as_array)
        .map(|docs| {
            docs.iter()
                .filter_map(|d| {
                    Some(Doc {
                        identifier: as_text(d.get("identifier")?)?,
                        title: as_text(d.get("title")?)?,
                        creator: d.get("creator").and_then(as_text),
                        year: d.get("year").and_then(as_text).and_then(|y| year_of(&y)),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pdf_rank(format: &str, name: &str) -> Option<u8> {
    let f = format.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    if !n.ends_with(".pdf") {
        return None;
    }
    if f == "text pdf" {
        Some(0)
    } else if f.contains("pdf") && !f.contains("container") {
        Some(1)
    } else if f.contains("pdf") {
        Some(2)
    } else {
        None
    }
}

/// The item's metadata → a hit, or `None` when it is access-restricted or has no PDF.
pub(crate) fn parse_metadata(base: &str, doc: &Doc, meta: &Value) -> Option<Hit> {
    let m = meta.get("metadata")?;
    let restricted = m
        .get("access-restricted-item")
        .and_then(as_text)
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if restricted {
        return None;
    }
    let file = meta
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(|f| {
            let name = f.get("name").and_then(jstr)?;
            let rank = pdf_rank(f.get("format").and_then(jstr).unwrap_or(""), name)?;
            let size = f
                .get("size")
                .and_then(as_text)
                .and_then(|s| s.parse::<u64>().ok());
            Some((rank, name, size))
        })
        .min_by_key(|(rank, _, _)| *rank)?;
    let (best, name, size) = file;
    // several equally good PDFs (volumes of a series): say which one this hit is
    let ties = meta["files"]
        .as_array()
        .map(|fs| {
            fs.iter()
                .filter(|f| {
                    let n = f.get("name").and_then(jstr).unwrap_or("");
                    pdf_rank(f.get("format").and_then(jstr).unwrap_or(""), n) == Some(best)
                })
                .count()
        })
        .unwrap_or(1);
    let title = if ties > 1 {
        let stem = name.trim_end_matches(".pdf").trim_end_matches(".PDF");
        format!("{} - {}", doc.title, stem)
    } else {
        doc.title.clone()
    };
    let url = format!(
        "{}/download/{}/{}",
        base.trim_end_matches('/'),
        enc(&doc.identifier).replace('+', "%20"),
        enc(name).replace('+', "%20")
    );
    let mut h = Hit::new(Source::InternetArchive, &doc.identifier, &title, &url);
    h.size = size;
    h.filename = Some(name.rsplit('/').next().unwrap_or(name).to_string());
    h.page_url = Some(format!(
        "{}/details/{}",
        base.trim_end_matches('/'),
        doc.identifier
    ));
    h.authors = doc.creator.iter().cloned().collect();
    h.year = doc
        .year
        .or_else(|| m.get("date").and_then(as_text).and_then(|d| year_of(&d)));
    h.license = m.get("licenseurl").and_then(as_text);
    h.summary = m
        .get("description")
        .and_then(as_text)
        .map(|d| tidy(&crate::types::strip_html(&d), 300));
    Some(h)
}

/// Audio items are offered when they carry a Creative Commons licence or sit in the
/// `netlabels` (Creative Commons netlabels) or `etree` (Live Music Archive, bands that allow
/// taping) collections.
pub const AUDIO_FILTER: &str = "mediatype:audio AND (licenseurl:(*creativecommons.org*) OR collection:(netlabels OR etree)) AND -access-restricted-item:true";

/// Films are offered when they carry a Creative Commons licence or sit in a collection of
/// public-domain material: `prelinger` (Prelinger Archives ephemeral films), `silent_films`
/// and `feature_films` (older features). `classic_tv` is left out: its copyright status is
/// mixed.
pub const VIDEO_FILTER: &str = "mediatype:movies AND (licenseurl:(*creativecommons.org*) OR collection:(prelinger OR silent_films OR feature_films)) AND -access-restricted-item:true";

/// Software items are offered only with a Creative Commons licence URL: the Archive's software
/// collections hold much that is not freely redistributable, so an explicit licence is required.
pub const SOFTWARE_FILTER: &str =
    "mediatype:software AND licenseurl:(*creativecommons.org*) AND -access-restricted-item:true";

/// Longest album offered as one zip.
const MAX_ALBUM_TRACKS: usize = 60;

/// Largest album offered as one zip (1.5 GiB).
const MAX_ALBUM_BYTES: u64 = 1_610_612_736;

/// Which audible or moving kind of item a media search looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Media {
    Audio,
    Video,
    Software,
}

/// How much a file looks like the program itself (lower is better); `None` for anything else.
fn software_rank(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    let ends = |exts: &[&str]| exts.iter().any(|e| n.ends_with(e));
    if ends(&[
        ".exe",
        ".msi",
        ".msix",
        ".dmg",
        ".appimage",
        ".deb",
        ".rpm",
        ".apk",
    ]) {
        Some(0)
    } else if ends(&[".zip", ".7z", ".tar.gz", ".tgz", ".tar.xz", ".iso"]) {
        Some(1)
    } else {
        None
    }
}

fn collections(m: &Value) -> Vec<String> {
    match m.get("collection") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|c| c.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn is_open_licence(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("creativecommons.org") && (u.starts_with("http://") || u.starts_with("https://"))
}

fn size_of(f: &Value) -> Option<u64> {
    f.get("size")
        .and_then(as_text)
        .and_then(|s| s.parse::<u64>().ok())
}

fn download_url(base: &str, id: &str, name: &str) -> String {
    format!(
        "{}/download/{}/{}",
        base.trim_end_matches('/'),
        enc(id).replace('+', "%20"),
        enc(name).replace('+', "%20")
    )
}

/// The item's metadata becomes a music or film hit, or `None` when the item is restricted,
/// fails the licence/collection rule or has no suitable file.
///
/// Music: the best audio format present (VBR MP3, MP3, Ogg Vorbis, FLAC). One file is linked
/// directly; an album of several files is linked as the Archive's on-the-fly zip of that
/// format (`/compress/<id>/formats=...`). Film: an MP4 (h.264, then 512Kb MPEG4, then MPEG4).
pub(crate) fn parse_media_metadata(
    base: &str,
    doc: &Doc,
    meta: &Value,
    media: Media,
) -> Option<Hit> {
    let m = meta.get("metadata")?;
    if m.get("access-restricted-item")
        .and_then(as_text)
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        return None;
    }
    let licence = m
        .get("licenseurl")
        .and_then(as_text)
        .filter(|l| is_open_licence(l));
    let cols = collections(m);
    let in_any = |names: &[&str]| cols.iter().any(|c| names.contains(&c.as_str()));
    let allowed = match media {
        Media::Audio => licence.is_some() || in_any(&["netlabels", "etree"]),
        Media::Video => {
            licence.is_some() || in_any(&["prelinger", "silent_films", "feature_films"])
        }
        Media::Software => licence.is_some(),
    };
    if !allowed {
        return None;
    }
    let files = meta.get("files")?.as_array()?;
    let id = doc.identifier.as_str();
    let (url, size, filename, title) = match media {
        Media::Audio => {
            let fmt_files = |fmt: &str| -> Vec<&Value> {
                files
                    .iter()
                    .filter(|f| {
                        f.get("format")
                            .and_then(jstr)
                            .is_some_and(|x| x.eq_ignore_ascii_case(fmt))
                            && f.get("name")
                                .and_then(jstr)
                                .is_some_and(|n| !n.contains("_sample"))
                    })
                    .collect()
            };
            let (fmt, list) = ["VBR MP3", "MP3", "Ogg Vorbis", "Flac"]
                .into_iter()
                .map(|f| (f, fmt_files(f)))
                .find(|(_, l)| !l.is_empty())?;
            if list.len() == 1 {
                let name = list[0].get("name").and_then(jstr)?;
                (
                    download_url(base, id, name),
                    size_of(list[0]),
                    name.rsplit('/').next().unwrap_or(name).to_string(),
                    doc.title.clone(),
                )
            } else {
                let total: Option<u64> = list.iter().map(|f| size_of(f)).sum();
                // a whole radio-show run or a 300-track dump is not an album
                if list.len() > MAX_ALBUM_TRACKS || total.is_some_and(|t| t > MAX_ALBUM_BYTES) {
                    return None;
                }
                (
                    format!(
                        "{}/compress/{}/formats={}&file=/{}.zip",
                        base.trim_end_matches('/'),
                        enc(id).replace('+', "%20"),
                        enc(fmt).replace('+', "%20"),
                        enc(id).replace('+', "%20")
                    ),
                    total,
                    format!("{id}.zip"),
                    format!("{} ({} tracks)", doc.title, list.len()),
                )
            }
        }
        Media::Software => {
            let (_, name, size) = files
                .iter()
                .filter_map(|f| {
                    let name = f.get("name").and_then(jstr)?;
                    // original uploads only: derivatives (thumbnails, torrents, listings) are not
                    // the program
                    let source = f
                        .get("source")
                        .and_then(jstr)
                        .is_none_or(|s| s.eq_ignore_ascii_case("original"));
                    (source && !name.contains('/'))
                        .then(|| software_rank(name))
                        .flatten()
                        .map(|rank| (rank, name, size_of(f)))
                })
                .min_by_key(|(rank, _, size)| (*rank, std::cmp::Reverse(size.unwrap_or(0))))?;
            (
                download_url(base, id, name),
                size,
                name.to_string(),
                doc.title.clone(),
            )
        }
        Media::Video => {
            let (_, name, size) = files
                .iter()
                .filter_map(|f| {
                    let name = f.get("name").and_then(jstr)?;
                    if !name.to_ascii_lowercase().ends_with(".mp4") {
                        return None;
                    }
                    let rank = match f
                        .get("format")
                        .and_then(jstr)?
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "h.264" | "h.264 hd" => 0u8,
                        "512kb mpeg4" => 1,
                        "mpeg4" => 2,
                        _ => return None,
                    };
                    Some((rank, name, size_of(f)))
                })
                .min_by_key(|(rank, _, _)| *rank)?;
            (
                download_url(base, id, name),
                size,
                name.rsplit('/').next().unwrap_or(name).to_string(),
                doc.title.clone(),
            )
        }
    };
    let mut h = Hit::new(Source::InternetArchive, id, &title, &url);
    h.size = size;
    h.filename = Some(filename);
    h.page_url = Some(format!("{}/details/{id}", base.trim_end_matches('/')));
    h.authors = doc.creator.iter().cloned().collect();
    h.year = doc
        .year
        .or_else(|| m.get("date").and_then(as_text).and_then(|d| year_of(&d)));
    // without a licence URL the reason for offering a film is its public-domain collection
    h.license = licence.or_else(|| {
        (media == Media::Video && in_any(&["prelinger", "silent_films"]))
            .then(|| "Public domain".to_string())
    });
    h.summary = m
        .get("description")
        .and_then(as_text)
        .map(|d| tidy(&crate::types::strip_html(&d), 300));
    Some(h)
}

impl InternetArchive {
    async fn search_media(&self, media: Media, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        let filter = match media {
            Media::Audio => AUDIO_FILTER,
            Media::Video => VIDEO_FILTER,
            Media::Software => SOFTWARE_FILTER,
        };
        let lucene = format!("({q}) AND {filter}");
        let url = format!(
            "{}/advancedsearch.php?q={}&fl[]=identifier&fl[]=title&fl[]=creator&fl[]=year&sort[]=downloads+desc&rows={}&page=1&output=json",
            self.base,
            enc(&lucene),
            (wanted * 2).min(30)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.pointer("/response/docs").is_none() {
            return Err(ResearchError::Parse("no response.docs".into()));
        }
        let mut hits = Vec::new();
        for doc in parse_search(&body) {
            if hits.len() >= wanted {
                break;
            }
            let murl = format!("{}/metadata/{}", self.base, enc(&doc.identifier));
            match self.http.json(|| self.http.get(&murl)).await {
                Ok(meta) => hits.extend(parse_media_metadata(&self.base, &doc, &meta, media)),
                Err(e) => tracing::debug!(error = %e, "archive item skipped"),
            }
        }
        Ok(hits)
    }
}

#[async_trait]
impl SearchSource for InternetArchive {
    fn source(&self) -> Source {
        Source::InternetArchive
    }

    fn max_queries(&self) -> usize {
        2
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        let lucene = format!(
            "({q}) AND mediatype:texts AND format:(\"Text PDF\") AND licenseurl:(*creativecommons.org*) AND -collection:(inlibrary OR printdisabled OR lendinglibrary) AND -access-restricted-item:true"
        );
        let url = format!(
            "{}/advancedsearch.php?q={}&fl[]=identifier&fl[]=title&fl[]=creator&fl[]=year&sort[]=downloads+desc&rows={}&page=1&output=json",
            self.base,
            enc(&lucene),
            (wanted * 2).min(30)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.pointer("/response/docs").is_none() {
            return Err(ResearchError::Parse("no response.docs".into()));
        }
        let mut hits = Vec::new();
        for doc in parse_search(&body) {
            if hits.len() >= wanted {
                break;
            }
            let murl = format!("{}/metadata/{}", self.base, enc(&doc.identifier));
            match self.http.json(|| self.http.get(&murl)).await {
                Ok(meta) => hits.extend(parse_metadata(&self.base, &doc, &meta)),
                // one broken item must not sink the search
                Err(e) => tracing::debug!(error = %e, "archive item skipped"),
            }
        }
        Ok(hits)
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        match category {
            Category::Music => self.search_media(Media::Audio, query, limit).await,
            Category::Video => self.search_media(Media::Video, query, limit).await,
            Category::Software => self.search_media(Media::Software, query, limit).await,
            _ => self.search(query, limit).await,
        }
    }
}
