//! What a search returns: one [`Hit`] per directly downloadable open item.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The eight sources. Each is open and legal: no paywalled or pirated content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// OpenAlex works with an open-access location.
    OpenAlex,
    /// Crossref works that carry an open licence and a full-text link.
    Crossref,
    /// arXiv preprints.
    Arxiv,
    /// Europe PMC open-access articles (PubMed Central).
    EuropePmc,
    /// Internet Archive items that are not access-restricted.
    InternetArchive,
    /// Wikimedia Commons files.
    Commons,
    /// Assets of GitHub releases in public repositories.
    Github,
    /// Data.gov datasets (CKAN).
    DataGov,
}

impl Source {
    /// Every source, in display order.
    pub const ALL: [Source; 8] = [
        Source::OpenAlex,
        Source::Crossref,
        Source::Arxiv,
        Source::EuropePmc,
        Source::InternetArchive,
        Source::Commons,
        Source::Github,
        Source::DataGov,
    ];

    /// Stable id (the same string serde uses).
    pub fn id(self) -> &'static str {
        match self {
            Source::OpenAlex => "open-alex",
            Source::Crossref => "crossref",
            Source::Arxiv => "arxiv",
            Source::EuropePmc => "europe-pmc",
            Source::InternetArchive => "internet-archive",
            Source::Commons => "commons",
            Source::Github => "github",
            Source::DataGov => "data-gov",
        }
    }

    /// Name shown on the badge.
    pub fn label(self) -> &'static str {
        match self {
            Source::OpenAlex => "OpenAlex",
            Source::Crossref => "Crossref",
            Source::Arxiv => "arXiv",
            Source::EuropePmc => "Europe PMC",
            Source::InternetArchive => "Internet Archive",
            Source::Commons => "Wikimedia Commons",
            Source::Github => "GitHub",
            Source::DataGov => "Data.gov",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One result: an item with a direct, open download link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// Stable within one search: `<source id>:<native id>`.
    pub id: String,
    /// Where it came from.
    pub source: Source,
    /// Title.
    pub title: String,
    /// Authors / creators (at most 6).
    pub authors: Vec<String>,
    /// Year of publication, when known.
    pub year: Option<u16>,
    /// A short plain-text description (at most 300 characters).
    pub summary: Option<String>,
    /// Licence exactly as the API reports it (name or URL).
    pub license: Option<String>,
    /// Size in bytes when the API says so.
    pub size: Option<u64>,
    /// Human landing page.
    pub page_url: Option<String>,
    /// Direct link to the file (what "Add to Riptide" downloads).
    pub download_url: String,
    /// Suggested file name.
    pub filename: Option<String>,
    /// DOI, if any.
    pub doi: Option<String>,
    /// arXiv id (versionless), if any.
    pub arxiv: Option<String>,
    /// PubMed id, if any.
    pub pmid: Option<String>,
    /// PubMed Central id, if any.
    pub pmcid: Option<String>,
}

impl Hit {
    /// A hit with only the mandatory fields.
    pub fn new(source: Source, native_id: &str, title: &str, download_url: &str) -> Self {
        Hit {
            id: format!("{}:{native_id}", source.id()),
            source,
            title: tidy(title, 300),
            authors: Vec::new(),
            year: None,
            summary: None,
            license: None,
            size: None,
            page_url: None,
            download_url: download_url.to_string(),
            filename: None,
            doi: None,
            arxiv: None,
            pmid: None,
            pmcid: None,
        }
    }

    /// The identity used to recognise a repeated item (here and in the download index).
    pub fn identity(&self) -> riptide_core::dedup::Identity<'_> {
        riptide_core::dedup::Identity {
            url: Some(&self.download_url),
            doi: self.doi.as_deref(),
            arxiv: self.arxiv.as_deref(),
            pmid: self.pmid.as_deref(),
            pmcid: self.pmcid.as_deref(),
            filename: self.filename.as_deref(),
            size: self.size,
            sha256: None,
        }
    }
}

/// Collapses whitespace and cuts to `max` characters (on a character boundary).
pub(crate) fn tidy(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Removes HTML tags and decodes the handful of entities the APIs use.
pub(crate) fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Only http(s) links are ever offered for download.
pub(crate) fn is_web_url(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://")
}

/// Upgrades `http://` to `https://` for hosts known to serve both.
pub(crate) fn prefer_https(u: &str) -> String {
    match u.strip_prefix("http://") {
        Some(rest)
            if [
                "arxiv.org/",
                "export.arxiv.org/",
                "archive.org/",
                "europepmc.org/",
            ]
            .iter()
            .any(|h| rest.starts_with(h)) =>
        {
            format!("https://{rest}")
        }
        _ => u.to_string(),
    }
}

/// The text of a JSON value when it is a string.
pub(crate) fn jstr(v: &serde_json::Value) -> Option<&str> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// A year from a string that starts or contains four digits (`2021-03-04`, `March 2020`).
pub(crate) fn year_of(s: &str) -> Option<u16> {
    let digits: Vec<char> = s.chars().collect();
    digits
        .windows(4)
        .find(|w| w.iter().all(char::is_ascii_digit))
        .and_then(|w| w.iter().collect::<String>().parse::<u16>().ok())
        .filter(|y| (1000..=2999).contains(y))
}
