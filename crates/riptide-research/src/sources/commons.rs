//! Wikimedia Commons (<https://www.mediawiki.org/wiki/API:Search>): freely licensed files
//! (File: namespace) with the licence name from the file's own metadata.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, strip_html, tidy, year_of, Hit, Source};

/// Commons adapter.
pub struct Commons {
    base: String,
    http: Http,
}

impl Commons {
    /// Talks to `commons.wikimedia.org`.
    pub fn new() -> Self {
        Self::with_base("https://commons.wikimedia.org", client_for(Source::Commons))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Commons {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Commons {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn parse(body: &Value) -> Vec<Hit> {
    let Some(pages) = body.pointer("/query/pages").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut rows: Vec<(u64, Hit)> = pages
        .values()
        .filter_map(|p| {
            let title = jstr(p.get("title")?)?;
            let info = p.get("imageinfo")?.as_array()?.first()?;
            let url = jstr(info.get("url")?).filter(|u| is_web_url(u))?;
            let pretty = title.trim_start_matches("File:");
            let mut h = Hit::new(
                Source::Commons,
                &format!("{}", p.get("pageid")?.as_u64()?),
                pretty,
                url,
            );
            h.size = info.get("size").and_then(Value::as_u64);
            h.page_url = info
                .get("descriptionurl")
                .and_then(jstr)
                .map(str::to_string);
            h.filename = Some(pretty.to_string());
            let ext = |name: &str| {
                info.pointer(&format!("/extmetadata/{name}/value"))
                    .and_then(jstr)
            };
            h.license = ext("LicenseShortName").map(str::to_string);
            h.authors = ext("Artist")
                .map(|a| vec![tidy(&strip_html(a), 80)])
                .unwrap_or_default();
            h.year = ext("DateTimeOriginal")
                .or_else(|| ext("DateTime"))
                .and_then(|d| year_of(&strip_html(d)));
            h.summary = ext("ImageDescription").map(|d| tidy(&strip_html(d), 300));
            let order = p.get("index").and_then(Value::as_u64).unwrap_or(u64::MAX);
            Some((order, h))
        })
        .collect();
    rows.sort_by_key(|(i, _)| *i);
    rows.into_iter().map(|(_, h)| h).collect()
}

#[async_trait]
impl SearchSource for Commons {
    fn source(&self) -> Source {
        Source::Commons
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let url = format!(
            "{}/w/api.php?action=query&format=json&generator=search&gsrnamespace=6&gsrlimit={}&gsrsearch={}&prop=imageinfo&iiprop=url|size|mime|extmetadata&iiextmetadatafilter=LicenseShortName|Artist|DateTimeOriginal|DateTime|ImageDescription",
            self.base,
            clamp(limit),
            enc(&q)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        if let Some(err) = body.get("error") {
            let code = err.get("code").and_then(jstr).unwrap_or("error");
            return Err(ResearchError::Parse(format!("the API said: {code}")));
        }
        // no matches: the API omits `query` entirely
        Ok(parse(&body))
    }
}
