//! OpenAlex (<https://docs.openalex.org/>): works filtered to open access. The direct link is
//! the best open-access location's `pdf_url`; `open_access.oa_url` is used only when it clearly
//! points at a file or a repository that serves the paper itself.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, prefer_https, tidy, Hit, Source};

/// OpenAlex adapter.
pub struct OpenAlex {
    base: String,
    http: Http,
}

impl OpenAlex {
    /// Talks to `api.openalex.org`.
    pub fn new() -> Self {
        Self::with_base("https://api.openalex.org", client_for(Source::OpenAlex))
    }

    /// Talks to `base` (tests point this at a mock server).
    pub fn with_base(base: &str, http: Http) -> Self {
        OpenAlex {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for OpenAlex {
    fn default() -> Self {
        Self::new()
    }
}

/// `oa_url` values worth offering when there is no `pdf_url`.
fn looks_direct(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    let path = u.split(['?', '#']).next().unwrap_or("");
    path.ends_with(".pdf")
        || u.contains("arxiv.org/pdf/")
        || u.contains("europepmc.org/articles/")
        || u.contains("ncbi.nlm.nih.gov/pmc/articles/") && u.contains("/pdf")
}

fn is_doi_redirect(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.starts_with("https://doi.org/")
        || u.starts_with("http://doi.org/")
        || u.starts_with("https://dx.doi.org/")
        || u.starts_with("http://dx.doi.org/")
}

pub(crate) fn parse(body: &Value) -> Vec<Hit> {
    let Some(results) = body.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|w| {
            let title = jstr(w.get("display_name")?).or_else(|| jstr(w.get("title")?))?;
            let id = jstr(w.get("id")?)?.rsplit('/').next()?.to_string();
            let loc = w.get("best_oa_location").filter(|l| l.is_object());
            let pdf = loc.and_then(|l| l.get("pdf_url")).and_then(jstr);
            let oa = w
                .pointer("/open_access/oa_url")
                .and_then(jstr)
                .filter(|u| looks_direct(u));
            // a DOI link is a redirect to the publisher page, not a file
            let url = pdf
                .or(oa)
                .filter(|u| is_web_url(u))
                .filter(|u| !is_doi_redirect(u))?;
            let mut h = Hit::new(Source::OpenAlex, &id, title, &prefer_https(url));
            h.authors = w
                .get("authorships")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.pointer("/author/display_name").and_then(jstr))
                        .take(6)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            h.year = w
                .get("publication_year")
                .and_then(Value::as_u64)
                .and_then(|y| u16::try_from(y).ok());
            h.license = loc
                .and_then(|l| l.get("license"))
                .and_then(jstr)
                .map(str::to_string);
            h.page_url = loc
                .and_then(|l| l.get("landing_page_url"))
                .and_then(jstr)
                .map(str::to_string);
            h.doi = w.get("doi").and_then(jstr).map(|d| {
                d.trim_start_matches("https://doi.org/")
                    .to_ascii_lowercase()
            });
            h.pmid = w
                .pointer("/ids/pmid")
                .and_then(jstr)
                .and_then(|u| u.rsplit('/').next())
                .map(str::to_string);
            h.pmcid = w
                .pointer("/ids/pmcid")
                .and_then(jstr)
                .and_then(|u| u.rsplit('/').next())
                .map(str::to_string);
            if let Some(a) = w.get("abstract_inverted_index").filter(|a| a.is_object()) {
                h.summary = Some(tidy(&uninvert(a), 300));
            }
            Some(h)
        })
        .collect()
}

/// OpenAlex ships abstracts as `{word: [positions]}`; put them back in order.
fn uninvert(index: &Value) -> String {
    let Some(map) = index.as_object() else {
        return String::new();
    };
    let mut words: Vec<(u64, &str)> = Vec::new();
    for (w, pos) in map {
        for p in pos
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
        {
            words.push((p, w));
        }
    }
    words.sort_unstable();
    words
        .into_iter()
        .map(|(_, w)| w)
        .collect::<Vec<_>>()
        .join(" ")
}

#[async_trait]
impl SearchSource for OpenAlex {
    fn source(&self) -> Source {
        Source::OpenAlex
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        // fetch extra: works without a direct file link are dropped after the fact
        let per_page = (clamp(limit) * 2).min(50);
        let url = format!(
            "{}/works?search={}&filter=open_access.is_oa:true&per-page={per_page}&select=id,doi,display_name,publication_year,authorships,open_access,best_oa_location,ids,abstract_inverted_index",
            self.base,
            enc(&q)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.get("results").is_none() {
            return Err(ResearchError::Parse("no results field".into()));
        }
        let mut hits = parse(&body);
        hits.truncate(clamp(limit));
        Ok(hits)
    }
}
