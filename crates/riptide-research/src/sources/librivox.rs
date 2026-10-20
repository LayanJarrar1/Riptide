//! LibriVox (<https://librivox.org/api/info>): public-domain audiobooks read by volunteers.
//! `url_zip_file` is a direct download of the whole book as MP3 files in one zip.
//!
//! The feed's `title` filter is an exact match unless it starts with `^`, which makes it a
//! prefix match. The adapter therefore searches for the query as a title prefix and, when that
//! finds nothing and the query has several words, retries with its first word.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, strip_html, tidy, year_of, Category, Hit, Source};

/// LibriVox adapter.
pub struct LibriVox {
    base: String,
    http: Http,
}

impl LibriVox {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://librivox.org", client_for(Source::LibriVox))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        LibriVox {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }

    async fn feed(&self, prefix: &str, limit: usize) -> Result<Vec<Hit>> {
        let url = format!(
            "{}/api/feed/audiobooks/?title={}&format=json&limit={}",
            self.base,
            enc(&format!("^{prefix}")),
            limit
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        // "no match" is reported as {"error": "Audiobooks could not be found"} with HTTP 200
        if body.get("books").is_none() {
            return if body.get("error").is_some() {
                Ok(Vec::new())
            } else {
                Err(ResearchError::Parse("no books field".into()))
            };
        }
        Ok(parse(&body))
    }
}

impl Default for LibriVox {
    fn default() -> Self {
        Self::new()
    }
}

/// Keeps letters, digits, spaces, apostrophes and hyphens: the filter is a pattern on the
/// server side and special characters would change its meaning.
fn clean(q: &str) -> String {
    let kept: String = q
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '\'' | '-' | '’'))
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Spaces in the zip link (`formats=64KBPS MP3`) must be encoded for a valid URL.
fn fix_url(u: &str) -> Option<String> {
    let u = u.trim();
    if !is_web_url(u) || u.chars().any(char::is_control) {
        return None;
    }
    Some(u.replace(' ', "%20"))
}

pub(crate) fn parse(body: &Value) -> Vec<Hit> {
    body.get("books")
        .and_then(Value::as_array)
        .map(|books| {
            books
                .iter()
                .filter_map(|b| {
                    let id = jstr(b.get("id")?)?;
                    let title = strip_html(jstr(b.get("title")?)?);
                    let url = fix_url(jstr(b.get("url_zip_file")?)?)?;
                    let mut h = Hit::new(Source::LibriVox, id, &title, &url);
                    h.license = Some("Public domain".into());
                    h.authors = b
                        .get("authors")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| {
                                    let first = x.get("first_name").and_then(jstr).unwrap_or("");
                                    let last = x.get("last_name").and_then(jstr).unwrap_or("");
                                    let n = tidy(&format!("{first} {last}"), 80);
                                    Some(n).filter(|n| !n.is_empty())
                                })
                                .take(6)
                                .collect()
                        })
                        .unwrap_or_default();
                    h.year = b.get("copyright_year").and_then(jstr).and_then(year_of);
                    h.summary = b
                        .get("description")
                        .and_then(jstr)
                        .map(|d| tidy(&strip_html(&d.replace("<br", " <br")), 300))
                        .filter(|d| !d.is_empty());
                    h.page_url = b
                        .get("url_librivox")
                        .and_then(jstr)
                        .filter(|u| is_web_url(u))
                        .map(str::to_string);
                    let tail = url
                        .split("file=")
                        .nth(1)
                        .map(|f| f.trim_matches('/').to_string())
                        .filter(|f| !f.is_empty() && !f.contains('/'));
                    h.filename = Some(tail.unwrap_or_else(|| format!("librivox-{id}.zip")));
                    Some(h)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl SearchSource for LibriVox {
    fn source(&self) -> Source {
        Source::LibriVox
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        if !matches!(category, Category::All | Category::Books) {
            return Ok(Vec::new());
        }
        let q = clean(&check_query(query)?);
        if q.is_empty() {
            return Err(ResearchError::Invalid("empty query".into()));
        }
        let wanted = clamp(limit);
        let mut hits = self.feed(&q, wanted).await?;
        if hits.is_empty() {
            if let Some(first) = q
                .split(' ')
                .next()
                .filter(|w| w.len() < q.len() && w.len() >= 3)
            {
                hits = self.feed(first, wanted).await?;
            }
        }
        hits.truncate(wanted);
        Ok(hits)
    }
}
