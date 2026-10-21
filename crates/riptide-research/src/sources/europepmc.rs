//! Europe PMC (<https://europepmc.org/RestfulWebService>): PubMed Central articles in the
//! open-access subset (`OPEN_ACCESS:y`), with the licence the API reports. The PDF link is the
//! article's open-access full-text PDF, or Europe PMC's PDF render of the PMC id.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, prefer_https, tidy, Hit, Source};

/// Europe PMC adapter.
pub struct EuropePmc {
    base: String,
    http: Http,
}

impl EuropePmc {
    /// Talks to `www.ebi.ac.uk/europepmc/webservices/rest`.
    pub fn new() -> Self {
        Self::with_base(
            "https://www.ebi.ac.uk/europepmc/webservices/rest",
            client_for(Source::EuropePmc),
        )
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        EuropePmc {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for EuropePmc {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn parse(body: &Value) -> Vec<Hit> {
    let Some(results) = body.pointer("/resultList/result").and_then(Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|r| {
            if r.get("isOpenAccess").and_then(jstr) != Some("Y") {
                return None;
            }
            let pmcid = r.get("pmcid").and_then(jstr)?;
            let title = r.get("title").and_then(jstr)?;
            // prefer the PDF the API lists as open access; else Europe PMC's PDF render
            let listed = r
                .pointer("/fullTextUrlList/fullTextUrl")
                .and_then(Value::as_array)
                .and_then(|l| {
                    l.iter().find(|u| {
                        u.get("documentStyle").and_then(jstr) == Some("pdf")
                            && u.get("availability").and_then(jstr) == Some("Open access")
                    })
                })
                .and_then(|u| u.get("url").and_then(jstr))
                .filter(|u| is_web_url(u))
                .map(prefer_https);
            let url = listed
                .unwrap_or_else(|| format!("https://europepmc.org/articles/{pmcid}?pdf=render"));
            let mut h = Hit::new(Source::EuropePmc, pmcid, title.trim_end_matches('.'), &url);
            h.pmcid = Some(pmcid.to_string());
            h.pmid = r.get("pmid").and_then(jstr).map(str::to_string);
            h.doi = r.get("doi").and_then(jstr).map(|d| d.to_ascii_lowercase());
            h.license = r.get("license").and_then(jstr).map(str::to_string);
            h.page_url = Some(format!("https://europepmc.org/article/PMC/{pmcid}"));
            h.filename = Some(format!("{pmcid}.pdf"));
            h.year = r
                .get("pubYear")
                .and_then(jstr)
                .and_then(|y| y.parse::<u16>().ok());
            h.authors = r
                .get("authorString")
                .and_then(jstr)
                .map(|a| {
                    a.trim_end_matches('.')
                        .split(", ")
                        .take(6)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            h.summary = r
                .get("abstractText")
                .and_then(jstr)
                .map(|s| tidy(&crate::types::strip_html(s), 300));
            Some(h)
        })
        .collect()
}

#[async_trait]
impl SearchSource for EuropePmc {
    fn source(&self) -> Source {
        Source::EuropePmc
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let url = format!(
            "{}/search?query={}&format=json&resultType=core&pageSize={}",
            self.base,
            enc(&format!("({q}) AND OPEN_ACCESS:y AND HAS_PDF:y")),
            clamp(limit)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.pointer("/resultList/result").is_none() {
            return Err(ResearchError::Parse("no resultList".into()));
        }
        Ok(parse(&body))
    }
}
