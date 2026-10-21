//! Open Library (<https://openlibrary.org/dev/docs/api/search>): only fully open scans.
//! The search lists works; a work is kept only when its `ebook_access` is `public` (never
//! `borrowable` or `printdisabled`, which are lending copies). The Internet Archive identifier
//! of the work is then resolved through `archive.org/metadata/<id>` to an EPUB or PDF file.
//! At most [`MAX_LOOKUPS`] metadata requests are made per search.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{jstr, strip_html, tidy, Category, Hit, Source};

/// Internet Archive metadata requests allowed per search.
pub const MAX_LOOKUPS: usize = 4;

/// Open Library adapter.
pub struct OpenLibrary {
    base: String,
    archive: String,
    http: Http,
}

impl OpenLibrary {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://openlibrary.org", client_for(Source::OpenLibrary))
    }

    /// Talks to `base` (tests); the Internet Archive stays at `https://archive.org`.
    pub fn with_base(base: &str, http: Http) -> Self {
        OpenLibrary {
            base: base.trim_end_matches('/').into(),
            archive: "https://archive.org".into(),
            http,
        }
    }

    /// Resolves Internet Archive identifiers against `archive` (tests).
    pub fn with_archive(mut self, archive: &str) -> Self {
        self.archive = archive.trim_end_matches('/').into();
        self
    }
}

impl Default for OpenLibrary {
    fn default() -> Self {
        Self::new()
    }
}

/// One fully open work from the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Work {
    pub key: String,
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<u16>,
    /// Internet Archive identifiers, the most promising first.
    pub ia: Vec<String>,
}

fn safe_id(i: &str) -> bool {
    !i.is_empty()
        && i.len() <= 120
        && i.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Works that are public and have identifiers. Lending and unknown-access works are dropped.
pub(crate) fn parse_search(body: &Value) -> Vec<Work> {
    body.get("docs")
        .and_then(Value::as_array)
        .map(|docs| {
            docs.iter()
                .filter(|d| {
                    d.get("ebook_access").and_then(jstr) == Some("public")
                        && d.get("public_scan_b").and_then(Value::as_bool) != Some(false)
                })
                .filter_map(|d| {
                    let mut ia: Vec<String> = d
                        .get("ia")?
                        .as_array()?
                        .iter()
                        .filter_map(jstr)
                        .filter(|i| safe_id(i))
                        .map(str::to_string)
                        .collect();
                    // `...0000...`, `bwb_` and `isbn_` identifiers are mostly lending-library scans: last
                    ia.sort_by_key(|i| {
                        i.contains("0000")
                            || i.starts_with("bwb_")
                            || i.starts_with("isbn_")
                            || i.ends_with("_librivox")
                    });
                    if ia.is_empty() {
                        return None;
                    }
                    Some(Work {
                        key: jstr(d.get("key")?)?
                            .trim_start_matches("/works/")
                            .to_string(),
                        title: jstr(d.get("title")?)?.to_string(),
                        authors: d
                            .get("author_name")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .filter_map(jstr)
                                    .map(|n| tidy(n, 80))
                                    .take(6)
                                    .collect()
                            })
                            .unwrap_or_default(),
                        year: d
                            .get("first_publish_year")
                            .and_then(Value::as_u64)
                            .and_then(|y| u16::try_from(y).ok()),
                        ia,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Array(a) => a.iter().find_map(text_of),
        _ => None,
    }
}

fn has_collection(meta: &Value, names: &[&str]) -> bool {
    match meta.get("collection") {
        Some(Value::String(s)) => names.contains(&s.as_str()),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .any(|c| names.contains(&c)),
        _ => false,
    }
}

/// EPUB first, then a text PDF, then any other PDF.
fn file_rank(format: &str, name: &str) -> Option<u8> {
    let f = format.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    if n.ends_with(".epub") && f == "epub" {
        Some(0)
    } else if n.ends_with(".pdf") && f == "text pdf" {
        Some(1)
    } else if n.ends_with(".pdf") && f.contains("pdf") && !f.contains("container") {
        Some(2)
    } else {
        None
    }
}

/// One work plus its item metadata becomes a hit, unless the item is restricted, a lending
/// copy or has no EPUB/PDF.
pub(crate) fn parse_metadata(archive: &str, work: &Work, id: &str, meta: &Value) -> Option<Hit> {
    let m = meta.get("metadata")?;
    if m.get("access-restricted-item")
        .and_then(text_of)
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        return None;
    }
    if has_collection(m, &["inlibrary", "lendinglibrary", "browserlending"]) {
        return None;
    }
    let (name, size) = meta
        .get("files")?
        .as_array()?
        .iter()
        .filter_map(|f| {
            let name = f.get("name").and_then(jstr)?;
            let rank = file_rank(f.get("format").and_then(jstr).unwrap_or(""), name)?;
            let size = f
                .get("size")
                .and_then(text_of)
                .and_then(|s| s.parse::<u64>().ok());
            Some((rank, name, size))
        })
        .min_by_key(|(rank, _, _)| *rank)
        .map(|(_, n, s)| (n, s))?;
    let url = format!(
        "{}/download/{}/{}",
        archive.trim_end_matches('/'),
        enc(id).replace('+', "%20"),
        enc(name).replace('+', "%20")
    );
    let mut h = Hit::new(Source::OpenLibrary, &work.key, &work.title, &url);
    h.authors = work.authors.clone();
    h.year = work.year;
    h.size = size;
    h.filename = Some(name.rsplit('/').next().unwrap_or(name).to_string());
    h.page_url = Some(format!("https://openlibrary.org/works/{}", work.key));
    h.license = m
        .get("licenseurl")
        .and_then(text_of)
        .or_else(|| Some("Public domain".to_string()));
    h.summary = m
        .get("description")
        .and_then(text_of)
        .map(|d| tidy(&strip_html(&d), 300))
        .filter(|d| !d.is_empty());
    Some(h)
}

#[async_trait]
impl SearchSource for OpenLibrary {
    fn source(&self) -> Source {
        Source::OpenLibrary
    }

    fn max_queries(&self) -> usize {
        2
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        if !matches!(category, Category::All | Category::Books) {
            return Ok(Vec::new());
        }
        let q = check_query(query)?;
        let wanted = clamp(limit);
        let url = format!(
            "{}/search.json?q={}&has_fulltext=true&ebook_access=public&limit=20&fields=key,title,author_name,first_publish_year,ia,ebook_access,public_scan_b",
            self.base,
            enc(&q)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.get("docs").is_none() {
            return Err(ResearchError::Parse("no docs field".into()));
        }
        let mut hits = Vec::new();
        let mut lookups = 0;
        'works: for work in parse_search(&body) {
            if hits.len() >= wanted {
                break;
            }
            for id in work.ia.iter().take(MAX_LOOKUPS) {
                if lookups >= MAX_LOOKUPS {
                    break 'works;
                }
                lookups += 1;
                let murl = format!("{}/metadata/{}", self.archive, enc(id));
                match self.http.json(|| self.http.get(&murl)).await {
                    Ok(meta) => {
                        if let Some(h) = parse_metadata(&self.archive, &work, id, &meta) {
                            hits.push(h);
                            continue 'works;
                        }
                    }
                    Err(ResearchError::RateLimited { retry_after }) if hits.is_empty() => {
                        return Err(ResearchError::RateLimited { retry_after })
                    }
                    Err(e) => tracing::debug!(error = %e, "open library item skipped"),
                }
            }
        }
        Ok(hits)
    }
}
