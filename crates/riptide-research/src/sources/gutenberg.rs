//! Project Gutenberg through Gutendex (<https://gutendex.com>): books whose copyright has
//! expired. Gutendex reports `copyright: false` for public-domain items; items flagged as
//! copyrighted are dropped and items with an unknown status are offered without a licence
//! badge. EPUB is preferred, then plain text, then HTML (never a cover image or an RDF file).

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, tidy, Category, Hit, Source};

/// Project Gutenberg through Gutendex adapter.
pub struct Gutenberg {
    base: String,
    http: Http,
}

impl Gutenberg {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://gutendex.com", client_for(Source::Gutenberg))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Gutenberg {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Gutenberg {
    fn default() -> Self {
        Self::new()
    }
}

/// The best downloadable format of one book: `(url, extension)`.
fn pick_format(formats: &serde_json::Map<String, Value>) -> Option<(String, &'static str)> {
    let find = |prefix: &str| {
        formats
            .iter()
            .filter(|(mime, _)| mime.starts_with(prefix))
            .filter_map(|(_, u)| jstr(u))
            .find(|u| is_web_url(u))
            .map(str::to_string)
    };
    find("application/epub+zip")
        .map(|u| (u, "epub"))
        .or_else(|| find("text/plain").map(|u| (u, "txt")))
        .or_else(|| find("text/html").map(|u| (u, "html")))
}

/// `Carroll, Lewis` becomes `Lewis Carroll`.
fn natural_name(name: &str) -> String {
    match name.split_once(',') {
        Some((last, first)) if !first.trim().is_empty() => {
            tidy(&format!("{} {}", first.trim(), last.trim()), 80)
        }
        _ => tidy(name, 80),
    }
}

/// Parses a Gutendex page. Public-domain items first-class; copyrighted and non-text items out.
pub(crate) fn parse(body: &Value) -> Vec<Hit> {
    body.get("results")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|b| {
                    if b.get("copyright").and_then(Value::as_bool) == Some(true) {
                        return None;
                    }
                    if b.get("media_type")
                        .and_then(jstr)
                        .is_some_and(|m| m != "Text")
                    {
                        return None;
                    }
                    let id = b.get("id")?.as_u64()?;
                    let title = jstr(b.get("title")?)?;
                    let (url, ext) = pick_format(b.get("formats")?.as_object()?)?;
                    let mut h = Hit::new(Source::Gutenberg, &id.to_string(), title, &url);
                    h.authors = b
                        .get("authors")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.get("name").and_then(jstr))
                                .map(natural_name)
                                .take(6)
                                .collect()
                        })
                        .unwrap_or_default();
                    if b.get("copyright").and_then(Value::as_bool) == Some(false) {
                        h.license = Some("Public domain".into());
                    }
                    h.summary = b
                        .get("summaries")
                        .and_then(Value::as_array)
                        .and_then(|s| s.first())
                        .and_then(jstr)
                        .map(|s| tidy(&crate::types::strip_html(s), 300));
                    h.page_url = Some(format!("https://www.gutenberg.org/ebooks/{id}"));
                    h.filename = Some(format!("pg{id}.{ext}"));
                    Some(h)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl SearchSource for Gutenberg {
    fn source(&self) -> Source {
        Source::Gutenberg
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        if !matches!(category, Category::All | Category::Books) {
            return Ok(Vec::new());
        }
        let q = check_query(query)?;
        let url = format!("{}/books/?search={}", self.base, enc(&q));
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.get("results").is_none() {
            return Err(ResearchError::Parse("no results field".into()));
        }
        let mut hits = parse(&body);
        hits.truncate(clamp(limit));
        Ok(hits)
    }
}
