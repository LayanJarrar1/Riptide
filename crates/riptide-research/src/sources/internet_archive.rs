//! Internet Archive (<https://archive.org/developers/>): text items that declare a Creative
//! Commons or public-domain licence and are not access-restricted (no lending-library scans;
//! unlicensed uploads are not offered, since their copyright status is unknown). Two steps: the advanced search finds items,
//! then each item's metadata names the PDF file that gets downloaded.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{jstr, tidy, year_of, Hit, Source};

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
}
