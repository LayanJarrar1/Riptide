//! Crossref (<https://api.crossref.org/swagger-ui/index.html>): only works that declare an open
//! licence (Creative Commons or public domain) and expose a PDF full-text link. Publisher
//! links of closed articles are never returned.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, Hit, Source};

/// Crossref adapter.
pub struct Crossref {
    base: String,
    http: Http,
}

impl Crossref {
    /// Talks to `api.crossref.org`.
    pub fn new() -> Self {
        Self::with_base("https://api.crossref.org", client_for(Source::Crossref))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Crossref {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Crossref {
    fn default() -> Self {
        Self::new()
    }
}

fn is_open_license(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("creativecommons.org/licenses/")
        || u.contains("creativecommons.org/publicdomain/")
        || u.contains("opensource.org/licenses")
}

pub(crate) fn parse(body: &Value) -> Vec<Hit> {
    let Some(items) = body.pointer("/message/items").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|w| {
            let doi = jstr(w.get("DOI")?)?.to_ascii_lowercase();
            let title = w.get("title")?.as_array()?.first().and_then(jstr)?;
            let license = w
                .get("license")?
                .as_array()?
                .iter()
                .filter_map(|l| l.get("URL").and_then(jstr))
                .find(|u| is_open_license(u))?;
            let pdf = w
                .get("link")?
                .as_array()?
                .iter()
                .filter(|l| {
                    l.get("content-type").and_then(jstr) == Some("application/pdf")
                        && l.get("intended-application").and_then(jstr) != Some("syndication")
                })
                .filter_map(|l| l.get("URL").and_then(jstr))
                .find(|u| is_web_url(u))?;
            let mut h = Hit::new(Source::Crossref, &doi, title, pdf);
            h.doi = Some(doi.clone());
            h.license = Some(license.to_string());
            h.page_url = Some(format!("https://doi.org/{doi}"));
            h.authors = w
                .get("author")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            let family = x.get("family").and_then(jstr)?;
                            Some(match x.get("given").and_then(jstr) {
                                Some(g) => format!("{g} {family}"),
                                None => family.to_string(),
                            })
                        })
                        .take(6)
                        .collect()
                })
                .unwrap_or_default();
            h.year = w
                .pointer("/issued/date-parts/0/0")
                .and_then(Value::as_u64)
                .and_then(|y| u16::try_from(y).ok());
            Some(h)
        })
        .collect()
}

#[async_trait]
impl SearchSource for Crossref {
    fn source(&self) -> Source {
        Source::Crossref
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let rows = (clamp(limit) * 4).min(100);
        let url = format!(
            "{}/works?query={}&rows={rows}&filter=has-license:true,has-full-text:true,full-text.type:application/pdf&select=DOI,title,author,issued,license,link",
            self.base,
            enc(&q)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.pointer("/message/items").is_none() {
            return Err(ResearchError::Parse("no items field".into()));
        }
        let mut hits = parse(&body);
        hits.truncate(clamp(limit));
        Ok(hits)
    }
}
