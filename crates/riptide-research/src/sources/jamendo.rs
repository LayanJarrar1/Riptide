//! Jamendo (<https://developer.jamendo.com/v3.0/tracks>): Creative Commons music. Needs the
//! user's own free client id (created at developer.jamendo.com); without one the adapter
//! answers [`ResearchError::Invalid`]. Only tracks whose download is allowed
//! (`audiodownload_allowed`) are offered, with the `audiodownload` link and the licence URL
//! the artist chose.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, tidy, year_of, Category, Hit, Source};

/// Jamendo adapter.
pub struct Jamendo {
    base: String,
    client_id: String,
    http: Http,
}

impl Jamendo {
    /// Talks to `api.jamendo.com` with the user's client id.
    pub fn new(client_id: &str) -> Self {
        Self::with_base(
            "https://api.jamendo.com",
            client_id,
            client_for(Source::Jamendo),
        )
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, client_id: &str, http: Http) -> Self {
        Jamendo {
            base: base.trim_end_matches('/').into(),
            client_id: client_id.trim().into(),
            http,
        }
    }
}

/// A file name that is safe to suggest: no separators or control characters.
fn safe_name(artist: &str, track: &str) -> String {
    let raw = if artist.is_empty() {
        track.to_string()
    } else {
        format!("{artist} - {track}")
    };
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    format!("{}.mp3", tidy(&cleaned, 120).trim_matches('.'))
}

pub(crate) fn parse(body: &Value) -> Vec<Hit> {
    body.get("results")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|t| t.get("audiodownload_allowed").and_then(Value::as_bool) == Some(true))
                .filter_map(|t| {
                    let id = match t.get("id")? {
                        Value::String(s) => s.trim().to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => return None,
                    };
                    let name = jstr(t.get("name")?)?;
                    let url = jstr(t.get("audiodownload")?).filter(|u| is_web_url(u))?;
                    let artist = t.get("artist_name").and_then(jstr).unwrap_or("");
                    let mut h = Hit::new(Source::Jamendo, &id, name, url);
                    h.authors = Some(tidy(artist, 80))
                        .filter(|a| !a.is_empty())
                        .into_iter()
                        .collect();
                    h.license = t
                        .get("license_ccurl")
                        .and_then(jstr)
                        .filter(|u| is_web_url(u))
                        .map(str::to_string);
                    h.year = t.get("releasedate").and_then(jstr).and_then(year_of);
                    h.page_url = t
                        .get("shareurl")
                        .and_then(jstr)
                        .filter(|u| is_web_url(u))
                        .map(str::to_string);
                    h.filename = Some(safe_name(artist, name));
                    h.summary = t
                        .get("album_name")
                        .and_then(jstr)
                        .map(|a| tidy(&format!("Album: {a}"), 300));
                    Some(h)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl SearchSource for Jamendo {
    fn source(&self) -> Source {
        Source::Jamendo
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        if !matches!(category, Category::All | Category::Music) {
            return Ok(Vec::new());
        }
        if self.client_id.is_empty() {
            return Err(ResearchError::Invalid("Jamendo needs a client id".into()));
        }
        let q = check_query(query)?;
        let url = format!(
            "{}/v3.0/tracks/?client_id={}&format=json&search={}&include=licenses&audiodownload_allowed=true&limit={}",
            self.base,
            enc(&self.client_id),
            enc(&q),
            clamp(limit)
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        let status = body.pointer("/headers/status").and_then(jstr);
        if status == Some("failed") || body.get("results").is_none() {
            // Jamendo answers HTTP 200 with a failure status for a bad client id; the message
            // never contains the id, but it is not shown anyway.
            return Err(ResearchError::Parse(
                "Jamendo rejected the request (is the client id valid?)".into(),
            ));
        }
        let mut hits = parse(&body);
        hits.truncate(clamp(limit));
        Ok(hits)
    }
}
