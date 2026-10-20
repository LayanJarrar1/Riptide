//! Data.gov (<https://catalog.data.gov/openapi.json>): the U.S. government's open-data catalogue.
//! Data.gov replaced its old CKAN API (`/api/3/action/package_search`, now answering 404) with a
//! DCAT-based search at `/search?format=json`; this adapter uses the new one. Each dataset lists
//! `distribution`s; only plain file downloads (CSV, JSON, XML, ZIP, PDF, spreadsheets, ...) are
//! offered, never web pages or interactive endpoints. The licence is shown when the dataset
//! declares one.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, strip_html, tidy, year_of, Hit, Source};

/// Media types that are plain files.
const FILE_TYPES: &[(&str, &str)] = &[
    ("text/csv", "CSV"),
    ("application/json", "JSON"),
    ("application/geo+json", "GeoJSON"),
    ("application/xml", "XML"),
    ("text/xml", "XML"),
    ("application/zip", "ZIP"),
    ("application/x-zip-compressed", "ZIP"),
    ("application/pdf", "PDF"),
    ("application/vnd.ms-excel", "XLS"),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "XLSX",
    ),
    ("text/plain", "TXT"),
    ("text/tab-separated-values", "TSV"),
    ("application/gzip", "GZ"),
];
/// Downloads offered per dataset.
const MAX_FILES: usize = 2;

/// Data.gov adapter.
pub struct DataGov {
    base: String,
    http: Http,
}

impl DataGov {
    /// Talks to `catalog.data.gov`.
    pub fn new() -> Self {
        Self::with_base("https://catalog.data.gov", client_for(Source::DataGov))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        DataGov {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for DataGov {
    fn default() -> Self {
        Self::new()
    }
}

fn file_kind(dist: &Value) -> Option<&'static str> {
    let media = dist.get("mediaType").and_then(jstr)?.to_ascii_lowercase();
    FILE_TYPES
        .iter()
        .find(|(m, _)| *m == media)
        .map(|(_, label)| *label)
}

pub(crate) fn parse(base: &str, body: &Value) -> Vec<Hit> {
    let Some(results) = body.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ds in results {
        let (Some(id), Some(title)) = (
            ds.get("identifier").and_then(jstr),
            ds.get("title").and_then(jstr),
        ) else {
            continue;
        };
        let dcat = ds.get("dcat");
        let license = dcat
            .and_then(|d| d.get("license"))
            .and_then(jstr)
            .map(str::to_string);
        let publisher = ds.get("publisher").and_then(jstr);
        let year = dcat
            .and_then(|d| d.get("modified"))
            .and_then(jstr)
            .or_else(|| ds.get("last_harvested_date").and_then(jstr))
            .and_then(year_of);
        let page = ds
            .get("slug")
            .and_then(jstr)
            .map(|s| format!("{}/dataset/{s}", base.trim_end_matches('/')));
        let summary = ds
            .get("description")
            .and_then(jstr)
            .map(|n| tidy(&strip_html(n), 300));
        let dists = dcat
            .and_then(|d| d.get("distribution"))
            .and_then(Value::as_array);
        let files = dists.into_iter().flatten().filter_map(|d| {
            let url = d
                .get("downloadURL")
                .and_then(jstr)
                .filter(|u| is_web_url(u))?;
            Some((url, file_kind(d)?))
        });
        for (url, kind) in files.take(MAX_FILES) {
            let mut h = Hit::new(
                Source::DataGov,
                &format!("{id}#{kind}"),
                &format!("{title} ({kind})"),
                url,
            );
            h.license = license.clone();
            h.page_url = page.clone();
            h.year = year;
            h.authors = publisher.iter().map(|p| (*p).to_string()).collect();
            h.summary = summary.clone();
            out.push(h);
        }
    }
    out
}

#[async_trait]
impl SearchSource for DataGov {
    fn source(&self) -> Source {
        Source::DataGov
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        let url = format!(
            "{}/search?q={}&format=json&per_page={}",
            self.base,
            enc(&q),
            wanted
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if body.get("results").is_none() {
            return Err(ResearchError::Parse("no results field".into()));
        }
        let mut hits = parse(&self.base, &body);
        hits.truncate(wanted);
        Ok(hits)
    }
}
