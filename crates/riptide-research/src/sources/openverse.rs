//! Openverse (<https://api.openverse.org/v1/>): openly licensed audio and images. Every result
//! carries its licence and a direct file `url`. The anonymous API is rate limited; a 429 is
//! reported as [`ResearchError::RateLimited`] rather than hidden.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, strip_html, tidy, Category, Hit, Source};

/// Openverse adapter.
pub struct Openverse {
    base: String,
    http: Http,
}

impl Openverse {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://api.openverse.org", client_for(Source::Openverse))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Openverse {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }

    async fn fetch(&self, kind: Media, q: &str, limit: usize) -> Result<Vec<Hit>> {
        let url = format!(
            "{}/v1/{}/?q={}&page_size={}&mature=false",
            self.base,
            kind.path(),
            enc(q),
            limit
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.get("results").is_none() {
            return Err(ResearchError::Parse("no results field".into()));
        }
        let mut hits = parse(&body, kind);
        hits.truncate(limit);
        Ok(hits)
    }
}

impl Default for Openverse {
    fn default() -> Self {
        Self::new()
    }
}

/// Which Openverse index a result list comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Media {
    Audio,
    Images,
}

impl Media {
    fn path(self) -> &'static str {
        match self {
            Media::Audio => "audio",
            Media::Images => "images",
        }
    }
}

/// `CC BY 2.0` style text, used when the API gives no licence URL.
fn license_text(r: &Value) -> Option<String> {
    let lic = r.get("license").and_then(jstr)?;
    let ver = r.get("license_version").and_then(jstr).unwrap_or("");
    Some(match lic.to_ascii_lowercase().as_str() {
        "cc0" => "CC0".to_string(),
        "pdm" => "Public domain".to_string(),
        other => format!("CC {} {ver}", other.to_ascii_uppercase())
            .trim()
            .to_string(),
    })
}

/// File name from the last path segment of the link, without query or fragment.
fn file_name(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next()?;
    let last = path.rsplit('/').next()?;
    if last.is_empty() || last.len() > 150 || last.contains(char::is_control) {
        return None;
    }
    Some(last.to_string())
}

pub(crate) fn parse(body: &Value, kind: Media) -> Vec<Hit> {
    body.get("results")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|r| r.get("mature").and_then(Value::as_bool) != Some(true))
                .filter_map(|r| {
                    let id = jstr(r.get("id")?)?;
                    let url = jstr(r.get("url")?).filter(|u| is_web_url(u))?;
                    let title = tidy(
                        &strip_html(r.get("title").and_then(jstr).unwrap_or("")),
                        300,
                    );
                    let title = if title.is_empty() {
                        file_name(url).unwrap_or_else(|| id.to_string())
                    } else {
                        title
                    };
                    let mut h = Hit::new(Source::Openverse, id, &title, url);
                    h.license = r
                        .get("license_url")
                        .and_then(jstr)
                        .filter(|u| is_web_url(u))
                        .map(str::to_string)
                        .or_else(|| license_text(r));
                    h.authors = r
                        .get("creator")
                        .and_then(jstr)
                        .map(|c| vec![tidy(&strip_html(c), 80)])
                        .unwrap_or_default();
                    h.size = r.get("filesize").and_then(Value::as_u64);
                    h.page_url = r
                        .get("foreign_landing_url")
                        .and_then(jstr)
                        .filter(|u| is_web_url(u))
                        .map(str::to_string);
                    h.filename = file_name(url);
                    if kind == Media::Audio {
                        let mut parts = vec!["Audio".to_string()];
                        if let Some(ms) = r.get("duration").and_then(Value::as_u64) {
                            let s = ms / 1000;
                            parts.push(format!("{}:{:02}", s / 60, s % 60));
                        }
                        if let Some(g) = r.get("genres").and_then(Value::as_array) {
                            let names: Vec<&str> =
                                g.iter().filter_map(Value::as_str).take(3).collect();
                            if !names.is_empty() {
                                parts.push(names.join(", "));
                            }
                        }
                        h.summary = Some(tidy(&parts.join(" - "), 300));
                    }
                    Some(h)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl SearchSource for Openverse {
    fn source(&self) -> Source {
        Source::Openverse
    }

    fn max_queries(&self) -> usize {
        2
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        match category {
            Category::Music => self.fetch(Media::Audio, &q, wanted).await,
            Category::Images => self.fetch(Media::Images, &q, wanted).await,
            Category::All => {
                // two requests, half of the limit each; one failing does not hide the other
                let each = wanted.div_ceil(2);
                let audio = self.fetch(Media::Audio, &q, each).await;
                let images = self.fetch(Media::Images, &q, each).await;
                match (audio, images) {
                    (Ok(mut a), Ok(i)) => {
                        a.extend(i);
                        a.truncate(wanted);
                        Ok(a)
                    }
                    (Ok(a), Err(_)) => Ok(a),
                    (Err(_), Ok(i)) => Ok(i),
                    (Err(e), Err(_)) => Err(e),
                }
            }
            _ => Ok(Vec::new()),
        }
    }
}
