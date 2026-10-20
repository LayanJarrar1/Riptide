//! arXiv (<https://info.arxiv.org/help/api/user-manual.html>): the Atom feed of preprints. The
//! PDF link is arXiv's own `/pdf/<id>`. The API does not report a licence, so none is shown.
//! arXiv asks for at most one request every three seconds; the adapter paces itself.

use std::sync::OnceLock;

use async_trait::async_trait;
use regex::Regex;

use super::{check_query, clamp, client_for, SearchSource};
use crate::error::{ResearchError, Result};
use crate::http::{enc, Http};
use crate::types::{tidy, year_of, Hit, Source};

/// arXiv adapter.
pub struct Arxiv {
    base: String,
    http: Http,
}

impl Arxiv {
    /// Talks to `export.arxiv.org`.
    pub fn new() -> Self {
        Self::with_base("https://export.arxiv.org", client_for(Source::Arxiv))
    }

    /// Talks to `base` (tests).
    pub fn with_base(base: &str, http: Http) -> Self {
        Arxiv {
            base: base.trim_end_matches('/').into(),
            http,
        }
    }
}

impl Default for Arxiv {
    fn default() -> Self {
        Self::new()
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("static regex"))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn tag(entry: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?s)<{name}(?:\s[^>]*)?>(.*?)</{name}>")).ok()?;
    re.captures(entry)
        .and_then(|c| c.get(1))
        .map(|m| unescape(m.as_str()))
}

pub(crate) fn parse(xml: &str) -> Vec<Hit> {
    static ENTRY: OnceLock<Regex> = OnceLock::new();
    static AUTHOR: OnceLock<Regex> = OnceLock::new();
    static PDF: OnceLock<Regex> = OnceLock::new();
    let entry_re = re(&ENTRY, r"(?s)<entry>(.*?)</entry>");
    let author_re = re(&AUTHOR, r"(?s)<author>\s*<name>(.*?)</name>");
    let pdf_re = re(&PDF, r#"<link\s[^>]*title="pdf"[^>]*>"#);
    entry_re
        .captures_iter(xml)
        .filter_map(|c| {
            let entry = c.get(1)?.as_str();
            let abs = tag(entry, "id")?;
            let id_part = abs.trim().rsplit_once("/abs/")?.1.to_string();
            let versionless = riptide_core::dedup::extract_arxiv(&format!("arxiv:{id_part}"))?;
            let title = tag(entry, "title")?;
            // the pdf link is arXiv's own address for this id
            let has_pdf_link = pdf_re.is_match(entry);
            if !has_pdf_link {
                return None;
            }
            let mut h = Hit::new(
                Source::Arxiv,
                &versionless,
                &title,
                &format!("https://arxiv.org/pdf/{id_part}"),
            );
            h.arxiv = Some(versionless.clone());
            h.page_url = Some(format!("https://arxiv.org/abs/{versionless}"));
            h.filename = Some(format!("{}.pdf", versionless.replace('/', "_")));
            h.summary = tag(entry, "summary").map(|s| tidy(&s, 300));
            h.year = tag(entry, "published").and_then(|p| year_of(&p));
            h.authors = author_re
                .captures_iter(entry)
                .filter_map(|a| a.get(1).map(|m| tidy(&unescape(m.as_str()), 80)))
                .take(6)
                .collect();
            h.doi = tag(entry, "arxiv:doi")
                .map(|d| d.trim().to_ascii_lowercase())
                .filter(|d| !d.is_empty());
            Some(h)
        })
        .collect()
}

/// `all:term AND all:term ...` for the first terms (arXiv's default is OR, which buries the
/// relevant papers under everything that mentions one common word).
fn search_query(q: &str) -> String {
    q.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .take(5)
        .map(|w| {
            let clean: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-')
                .collect();
            format!("all:{}", enc(&clean))
        })
        .collect::<Vec<_>>()
        .join("+AND+")
}

#[async_trait]
impl SearchSource for Arxiv {
    fn source(&self) -> Source {
        Source::Arxiv
    }

    fn max_queries(&self) -> usize {
        2
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let q = check_query(query)?;
        let url = format!(
            "{}/api/query?search_query={}&start=0&max_results={}&sortBy=relevance",
            self.base,
            search_query(&q),
            clamp(limit)
        );
        let xml = self.http.text(|| self.http.get(&url)).await?;
        if xml
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("rate exceeded")
        {
            return Err(ResearchError::RateLimited { retry_after: None });
        }
        if !xml.contains("<feed") {
            return Err(ResearchError::Parse("not an Atom feed".into()));
        }
        Ok(parse(&xml))
    }
}
