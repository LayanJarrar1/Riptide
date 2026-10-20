//! OpenGameArt (<https://opengameart.org>): freely licensed game art, music and sound effects.
//!
//! The site has no API, so this adapter reads two kinds of public HTML pages: the advanced
//! search (`/art-search-advanced`, allowed by `robots.txt`; only `/search/` is disallowed) and
//! the art pages of the first few results, which carry the direct file links (always under
//! `/sites/default/files/`), their sizes and the licences. `robots.txt` asks for a crawl delay of
//! ten seconds, so [`OpenGameArt::new`] spaces its requests that far apart and opens at most
//! [`MAX_ART_PAGES`] art pages per search.
//!
//! The parsing is plain string and regex work over markup the site controls. Everything read from
//! a page is untrusted: only file links under the site's own file directory are offered, text is
//! stripped of tags and cut to length, and a page that no longer looks like the markup recorded in
//! `tests/fixtures/` produces [`ResearchError::Parse`] instead of a guess.

use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;

use super::{check_query, clamp, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{strip_html, tidy, year_of, Category, Hit, Source};

/// Art pages opened per search (each costs one request at the site's crawl delay).
pub const MAX_ART_PAGES: usize = 3;
/// Files offered per art page.
const MAX_FILES: usize = 2;
/// The crawl delay `robots.txt` asks for.
const CRAWL_DELAY: Duration = Duration::from_secs(10);

/// OpenGameArt adapter.
pub struct OpenGameArt {
    base: String,
    http: Http,
}

impl OpenGameArt {
    /// Talks to the real site, honouring its ten second crawl delay.
    pub fn new() -> Self {
        Self::with_base("https://opengameart.org", Http::new(CRAWL_DELAY))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        OpenGameArt {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for OpenGameArt {
    fn default() -> Self {
        Self::new()
    }
}

/// The site's "art type" term ids (`2D Art` 9, `3D Art` 10, `Concept Art` 7273, `Texture` 14,
/// `Music` 12, `Sound Effect` 13, `Document` 11), recorded from the search form.
fn type_ids(category: Category) -> &'static [u32] {
    match category {
        Category::Games => &[9, 10, 14, 7273],
        Category::Images => &[9, 7273],
        Category::Music => &[12, 13],
        _ => &[],
    }
}

fn pattern(p: &str) -> Result<Regex> {
    Regex::new(p).map_err(|_| ResearchError::Parse("internal pattern".into()))
}

/// Decodes the entities the site emits, including numeric ones (`&#039;`).
fn decode(text: &str) -> String {
    let plain = strip_html(text);
    let mut out = String::with_capacity(plain.len());
    let mut rest = plain.as_str();
    while let Some(i) = rest.find("&#") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        let end = tail.find(';').filter(|e| *e <= 8);
        let ch = end.and_then(|e| {
            let num = &tail[..e];
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse::<u32>().ok()?,
            };
            char::from_u32(code).filter(|c| !c.is_control())
        });
        match (ch, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            _ => {
                out.push_str("&#");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtLink {
    /// Path of the art page (`/content/...`).
    pub path: String,
    pub title: String,
}

/// The art pages listed by a search page. `Err(Parse)` when the page is not a search page.
pub(crate) fn parse_search(html: &str) -> Result<Vec<ArtLink>> {
    if !html.contains("Search Art") {
        return Err(ResearchError::Parse(
            "the search page is not recognised".into(),
        ));
    }
    // the sidebar lists forum topics in similar markup: only the result list counts
    let Some(start) = html.find("art-previews-inline") else {
        return Ok(Vec::new());
    };
    let re = pattern(r##"(?s)art-preview-title"><a href="(/content/[^"#?]+)"[^>]*>(.*?)</a>"##)?;
    let mut out: Vec<ArtLink> = Vec::new();
    for c in re.captures_iter(&html[start..]) {
        let path = c[1].to_string();
        let title = tidy(&decode(&c[2]), 200);
        if path.contains("..") || title.is_empty() || out.iter().any(|l| l.path == path) {
            continue;
        }
        out.push(ArtLink { path, title });
    }
    Ok(out)
}

/// One downloadable file of an art page.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtFile {
    url: String,
    name: String,
    mime: String,
    size: Option<u64>,
}

fn section<'a>(html: &'a str, marker: &str, stops: &[&str], max: usize) -> Option<&'a str> {
    let start = html.find(marker)?;
    let tail = &html[start..];
    let mut end = tail.len().min(max);
    for s in stops {
        if let Some(i) = tail[marker.len()..].find(s) {
            end = end.min(i + marker.len());
        }
    }
    while !tail.is_char_boundary(end) {
        end -= 1;
    }
    Some(&tail[..end])
}

/// Is `url` a file under the site's own file directory (on the real site or on `base`)?
fn is_site_file(url: &str, base: &str) -> bool {
    const REAL: &str = "https://opengameart.org";
    [REAL, base].iter().any(|origin| {
        url.strip_prefix(origin)
            .is_some_and(|p| p.starts_with("/sites/default/files/") && !p.contains(".."))
    })
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"(?:^|\s){name}="([^"]*)""#)).ok()?;
    re.captures(attrs).map(|c| c[1].to_string())
}

/// Parses an art page into hits (at most [`MAX_FILES`]). `link` is the search result it came from.
pub(crate) fn parse_art(html: &str, link: &ArtLink, base: &str) -> Result<Vec<Hit>> {
    let files_sec = section(
        html,
        "field-name-field-art-files",
        &["links inline", "group-footer"],
        60_000,
    );
    let lic_sec = section(
        html,
        "field-name-field-art-licenses",
        &["field-name-collect", "field-name-field-art-files"],
        4_000,
    );
    if files_sec.is_none() && lic_sec.is_none() {
        return Err(ResearchError::Parse(
            "the art page is not recognised".into(),
        ));
    }
    let Some(files_sec) = files_sec else {
        return Ok(Vec::new());
    };

    let anchor = pattern(r"(?s)<a\s([^>]*download=[^>]*)>")?;
    let mut files: Vec<ArtFile> = Vec::new();
    for c in anchor.captures_iter(files_sec) {
        let attrs = &c[1];
        let Some(href) = attr(attrs, "href").map(|h| h.replace("&amp;", "&")) else {
            continue;
        };
        if !is_site_file(&href, base) {
            continue;
        }
        let ty = attr(attrs, "type").unwrap_or_default();
        let (mime, size) = match ty.split_once(';') {
            Some((m, rest)) => (
                m.trim().to_ascii_lowercase(),
                rest.trim()
                    .strip_prefix("length=")
                    .and_then(|n| n.trim().parse::<u64>().ok())
                    .filter(|n| *n > 0),
            ),
            None => (ty.trim().to_ascii_lowercase(), None),
        };
        let fallback = href.rsplit('/').next().unwrap_or("file").to_string();
        let name = attr(attrs, "download")
            .map(|n| decode(&n))
            .map(|n| n.replace(['/', '\\'], "_"))
            .map(|n| tidy(&n, 150))
            .filter(|n| !n.is_empty() && n != "." && n != "..")
            .unwrap_or(fallback);
        files.push(ArtFile {
            url: href,
            name,
            mime,
            size,
        });
    }
    // archives hold whole packs; for music and sound the audio file itself is what is wanted
    let is_audio = files.iter().any(|f| f.mime.starts_with("audio/"));
    files.sort_by_key(|f| {
        let archive = f.mime.contains("zip")
            || f.mime.contains("7z")
            || f.mime.contains("rar")
            || f.mime.contains("tar");
        match (is_audio, archive, f.mime.starts_with("audio/")) {
            (true, _, true) => 0,
            (false, true, _) => 0,
            (_, true, _) => 2,
            _ => 1,
        }
    });

    // licences: the page lists every licence the author offers; the most permissive is shown
    let names: Vec<String> = match lic_sec {
        Some(sec) => pattern(r#"(?s)license-name['"]>(.*?)</div>"#)?
            .captures_iter(sec)
            .map(|c| tidy(&decode(&c[1]), 40))
            .filter(|n| !n.is_empty())
            .collect(),
        None => Vec::new(),
    };
    let license = names
        .iter()
        .find(|n| n.to_ascii_uppercase().starts_with("CC0"))
        .or_else(|| names.first())
        .cloned();

    let author = pattern(r#"(?s)class='username'><a[^>]*>(.*?)</a>"#)?
        .captures(html)
        .map(|c| tidy(&decode(&c[1]), 80))
        .filter(|a| !a.is_empty());
    let year = pattern(r#"(?s)field-name-post-date.*?field-item even">(.*?)</div>"#)?
        .captures(html)
        .and_then(|c| year_of(&decode(&c[1])));
    let summary = section(
        html,
        r#"property="content:encoded">"#,
        &[
            "field-name-field-art-attribution",
            "field-name-field-art-files",
        ],
        6_000,
    )
    .map(|s| {
        tidy(
            &decode(s.trim_start_matches(r#"property="content:encoded">"#)),
            280,
        )
    })
    .filter(|s| !s.is_empty());
    let summary = match (summary, names.len() > 1) {
        (Some(s), true) => Some(tidy(&format!("Licences: {}. {s}", names.join(", ")), 300)),
        (None, true) => Some(tidy(&format!("Licences: {}.", names.join(", ")), 300)),
        (s, false) => s,
    };

    let page_url = format!("{base}{}", link.path);
    let slug = link.path.trim_start_matches("/content/");
    Ok(files
        .into_iter()
        .take(MAX_FILES)
        .map(|f| {
            let mut h = Hit::new(
                Source::OpenGameArt,
                &format!("{slug}/{}", f.name),
                &format!("{} - {}", link.title, f.name),
                &f.url,
            );
            h.size = f.size;
            h.filename = Some(f.name);
            h.page_url = Some(page_url.clone());
            h.license = license.clone();
            h.year = year;
            h.authors = author.clone().into_iter().collect();
            h.summary = summary.clone();
            h
        })
        .collect())
}

#[async_trait]
impl SearchSource for OpenGameArt {
    fn source(&self) -> Source {
        Source::OpenGameArt
    }

    fn max_queries(&self) -> usize {
        1
    }

    async fn search_in(&self, category: Category, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let wanted = clamp(limit);
        let mut url = format!(
            "{}/art-search-advanced?keys={}&sort_by=score&sort_order=DESC",
            self.base,
            enc(&q)
        );
        for id in type_ids(category) {
            url.push_str(&format!("&field_art_type_tid%5B%5D={id}"));
        }
        let html = self.http.text(|| self.http.get(&url)).await?;
        let links = parse_search(&html)?;
        let mut hits: Vec<Hit> = Vec::new();
        let mut last_err: Option<ResearchError> = None;
        for link in links.iter().take(MAX_ART_PAGES) {
            if hits.len() >= wanted {
                break;
            }
            let page = format!("{}{}", self.base, link.path);
            match self.http.text(|| self.http.get(&page)).await {
                Ok(body) => match parse_art(&body, link, &self.base) {
                    Ok(found) => hits.extend(found),
                    Err(e) => last_err = Some(e),
                },
                Err(ResearchError::RateLimited { retry_after }) if hits.is_empty() => {
                    return Err(ResearchError::RateLimited { retry_after })
                }
                Err(e) => {
                    tracing::debug!(error = %e, "opengameart page skipped");
                    last_err = Some(e);
                }
            }
        }
        if hits.is_empty() {
            if let Some(e @ ResearchError::Parse(_)) = last_err {
                return Err(e);
            }
        }
        hits.truncate(wanted);
        Ok(hits)
    }
}
