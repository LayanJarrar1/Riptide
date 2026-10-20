//! NASA Image and Video Library (<https://images.nasa.gov>, API at
//! <https://images-api.nasa.gov>): images and videos made by NASA.
//!
//! One search request lists the items; the `collection.json` of the first few items lists every
//! rendition of the file, from which the best direct link is picked (the original image, or a
//! medium-sized MP4 for video, since originals of films can be several gigabytes). NASA media is
//! generally not copyrighted in the United States, but the agency's media usage guidelines
//! apply (no endorsement, no use of the insignia), which the licence text says.

use async_trait::async_trait;
use serde_json::Value;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{is_web_url, jstr, strip_html, tidy, year_of, Category, Hit, Source};

/// Items whose `collection.json` is fetched per search.
const MAX_ITEMS: usize = 5;

/// The licence text shown on every hit.
pub const LICENSE: &str = "Public domain (NASA media usage guidelines apply)";

/// NASA Image and Video Library adapter.
pub struct Nasa {
    base: String,
    http: Http,
}

impl Nasa {
    /// Talks to the real service.
    pub fn new() -> Self {
        Self::with_base("https://images-api.nasa.gov", client_for(Source::Nasa))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Nasa {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Nasa {
    fn default() -> Self {
        Self::new()
    }
}

/// What kind of media a search asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Media {
    Image,
    Video,
}

impl Media {
    fn api(self) -> &'static str {
        match self {
            Media::Image => "image",
            Media::Video => "video",
        }
    }
}

/// One search item before its files are known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    pub nasa_id: String,
    pub title: String,
    pub description: Option<String>,
    pub year: Option<u16>,
    pub creators: Vec<String>,
    /// URL of the item's `collection.json`.
    pub collection: String,
    /// The best still image the search answer itself lists: `(url, size)`.
    pub inline_original: Option<(String, Option<u64>)>,
}

/// The ids NASA uses are plain (`jsc2007e034221`, `KSC-19690716-...`); anything else would
/// not make a sensible path.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !id.contains("..")
}

/// Asset links come as `http://images-assets.nasa.gov/...`, which the CDN redirects to https;
/// ask for https directly, and encode spaces.
fn clean_asset_url(u: &str) -> Option<String> {
    let u = u.trim();
    let u = match u.strip_prefix("http://images-assets.nasa.gov/") {
        Some(rest) => format!("https://images-assets.nasa.gov/{rest}"),
        None => u.to_string(),
    };
    if !is_web_url(&u) || u.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(u.replace(' ', "%20"))
}

pub(crate) fn parse_search(body: &Value) -> Result<Vec<Item>> {
    let items = body
        .pointer("/collection/items")
        .and_then(Value::as_array)
        .ok_or_else(|| ResearchError::Parse("no collection.items field".into()))?;
    Ok(items
        .iter()
        .filter_map(|it| {
            let data = it.get("data")?.as_array()?.first()?;
            let nasa_id = jstr(data.get("nasa_id")?)?;
            if !safe_id(nasa_id) {
                return None;
            }
            let collection = clean_asset_url(jstr(it.get("href")?)?)?;
            let title = tidy(&strip_html(jstr(data.get("title")?)?), 300);
            if title.is_empty() {
                return None;
            }
            let creators = ["photographer", "secondary_creator"]
                .iter()
                .filter_map(|k| data.get(*k).and_then(jstr))
                .map(|c| tidy(&strip_html(c), 80))
                .filter(|c| !c.is_empty())
                .take(2)
                .collect();
            let inline_original = it
                .get("links")
                .and_then(Value::as_array)
                .and_then(|ls| best_inline_image(ls));
            Some(Item {
                nasa_id: nasa_id.to_string(),
                title,
                description: data
                    .get("description")
                    .and_then(jstr)
                    .map(|d| tidy(&strip_html(d), 300))
                    .filter(|d| !d.is_empty()),
                year: data.get("date_created").and_then(jstr).and_then(year_of),
                creators,
                collection,
                inline_original,
            })
        })
        .collect())
}

/// The best still image among the renditions the search answer lists itself: the original when
/// it is a JPEG or PNG (originals are often TIFF), else the large, else the medium JPEG.
fn best_inline_image(links: &[Value]) -> Option<(String, Option<u64>)> {
    ["~orig.", "~large.", "~medium."].iter().find_map(|marker| {
        links.iter().find_map(|l| {
            if l.get("render").and_then(jstr) != Some("image") {
                return None;
            }
            let href = jstr(l.get("href")?)?;
            let low = href.to_ascii_lowercase();
            let ok_ext = [".jpg", ".jpeg", ".png"].iter().any(|e| low.ends_with(e));
            if !(low.contains(marker) && ok_ext) {
                return None;
            }
            Some((
                clean_asset_url(href)?,
                l.get("size").and_then(Value::as_u64),
            ))
        })
    })
}

/// Picks the best direct file out of a `collection.json` listing.
pub(crate) fn pick_file(listing: &Value, media: Media) -> Option<String> {
    let urls: Vec<&str> = listing.as_array()?.iter().filter_map(jstr).collect();
    let find = |marker: &str, exts: &[&str]| {
        urls.iter().copied().find(|u| {
            let path = u.split(['?', '#']).next().unwrap_or(u).to_ascii_lowercase();
            path.contains(marker) && exts.iter().any(|e| path.ends_with(e))
        })
    };
    let chosen = match media {
        Media::Image => find("~orig.", &[".jpg", ".jpeg", ".png"])
            .or_else(|| find("~large.", &[".jpg", ".jpeg", ".png"]))
            .or_else(|| find("~medium.", &[".jpg", ".jpeg", ".png"])),
        Media::Video => find("~medium.", &[".mp4"])
            .or_else(|| find("~orig.", &[".mp4", ".mov"]))
            .or_else(|| find("~mobile.", &[".mp4"]))
            .or_else(|| find("~small.", &[".mp4"])),
    }?;
    clean_asset_url(chosen)
}

pub(crate) fn make_hit(item: &Item, url: &str, size: Option<u64>) -> Hit {
    let mut h = Hit::new(Source::Nasa, &item.nasa_id, &item.title, url);
    h.size = size;
    h.filename = url
        .split(['?', '#'])
        .next()
        .and_then(|p| p.rsplit('/').next())
        .map(|n| n.replace("%20", " ").replace('~', "_"))
        .filter(|n| !n.is_empty());
    h.page_url = Some(format!("https://images.nasa.gov/details/{}", item.nasa_id));
    h.license = Some(LICENSE.to_string());
    h.year = item.year;
    h.authors = item.creators.clone();
    h.summary = item.description.clone();
    h
}

#[async_trait]
impl SearchSource for Nasa {
    fn source(&self) -> Source {
        Source::Nasa
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        let media = if category == Category::Video {
            Media::Video
        } else {
            Media::Image
        };
        let url = format!(
            "{}/search?q={}&media_type={}&page_size={}",
            self.base,
            enc(&q),
            media.api(),
            MAX_ITEMS.max(wanted.min(10))
        );
        let body = self.http.json(|| self.http.get(&url)).await?;
        let items = parse_search(&body)?;
        let mut hits = Vec::new();
        for item in items.iter().take(MAX_ITEMS) {
            if hits.len() >= wanted {
                break;
            }
            // the search answer already names the original image and its size
            if let (Media::Image, Some((u, size))) = (media, &item.inline_original) {
                hits.push(make_hit(item, u, *size));
                continue;
            }
            match self.http.json(|| self.http.get(&item.collection)).await {
                Ok(listing) => {
                    if let Some(u) = pick_file(&listing, media) {
                        hits.push(make_hit(item, &u, None));
                    }
                }
                Err(ResearchError::RateLimited { retry_after }) if hits.is_empty() => {
                    return Err(ResearchError::RateLimited { retry_after })
                }
                Err(e) => tracing::debug!(error = %e, "nasa item skipped"),
            }
        }
        hits.truncate(wanted);
        Ok(hits)
    }
}
