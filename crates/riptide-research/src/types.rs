//! What a search returns: one [`Hit`] per directly downloadable open item.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What kind of thing the user is looking for. Each [`Source`] serves one or more of these.
///
/// The Research page offers exactly the seven [`Category::SEARCH`] categories and the user must
/// pick one (there is no silent "all"). The remaining variants are the older, finer-grained ids:
/// they still deserialize (stored history, `video` as an alias of `videos`) and the adapters use
/// them internally, and [`Category::canonical`] maps them to the seven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// Everything the sources offer (internal; a search must name a real category).
    All,
    /// Scholarly papers and preprints (legacy id of [`Category::Academic`]).
    Papers,
    /// Books and audiobooks (legacy id; [`Category::Documents`] is the user-facing one).
    Books,
    /// Music and other audio (legacy; no user-facing category).
    Music,
    /// Films, series, shows and other moving images.
    #[serde(rename = "videos", alias = "video")]
    Video,
    /// Open-source and freely licensed games and game assets (legacy).
    Games,
    /// Software releases (installers and archives).
    Software,
    /// Photos, illustrations and scientific imagery.
    Images,
    /// Open datasets (legacy; no user-facing category).
    Datasets,
    /// General web results (not files; "Find files on this page" lists the files).
    Web,
    /// Academic research: papers, preprints, journal articles.
    Academic,
    /// Documents and PDFs: public-domain books, scans, open-access full texts.
    Documents,
    /// Open-source projects: repositories with an open licence.
    OpenSource,
}

impl Category {
    /// Every category, legacy ones included.
    pub const ALL: [Category; 13] = [
        Category::All,
        Category::Papers,
        Category::Books,
        Category::Music,
        Category::Video,
        Category::Games,
        Category::Software,
        Category::Images,
        Category::Datasets,
        Category::Web,
        Category::Academic,
        Category::Documents,
        Category::OpenSource,
    ];

    /// The seven categories a search can ask for, in the order the page shows them.
    pub const SEARCH: [Category; 7] = [
        Category::Video,
        Category::Software,
        Category::Documents,
        Category::Images,
        Category::Academic,
        Category::OpenSource,
        Category::Web,
    ];

    /// The one of [`Category::SEARCH`] this category stands for, or `None` for `All`, `Music`
    /// and `Datasets`, which a search may not ask for (the user must choose).
    pub fn canonical(self) -> Option<Category> {
        match self {
            Category::Video
            | Category::Software
            | Category::Documents
            | Category::Images
            | Category::Academic
            | Category::OpenSource
            | Category::Web => Some(self),
            Category::Papers => Some(Category::Academic),
            Category::Books => Some(Category::Documents),
            Category::Games => Some(Category::OpenSource),
            Category::All | Category::Music | Category::Datasets => None,
        }
    }

    /// The sources that really serve this category, most useful first (the order in which a
    /// mode's source budget is spent). Empty for the legacy ids; use [`Category::canonical`].
    pub fn sources(self) -> &'static [Source] {
        match self {
            Category::Video => &[
                Source::Youtube,
                Source::InternetArchive,
                Source::Commons,
                Source::Nasa,
                Source::Blender,
            ],
            Category::Software => &[Source::Github, Source::InternetArchive],
            Category::Documents => &[
                Source::InternetArchive,
                Source::Gutenberg,
                Source::OpenLibrary,
                Source::EuropePmc,
            ],
            Category::Images => &[Source::Commons, Source::Openverse, Source::Nasa],
            Category::Academic => &[
                Source::OpenAlex,
                Source::Crossref,
                Source::Arxiv,
                Source::EuropePmc,
            ],
            Category::OpenSource => &[Source::Github],
            Category::Web => &[Source::Web],
            _ => &[],
        }
    }

    /// The category an adapter understands: the user-facing ids map back to the finer ones the
    /// adapters were written for (`Documents` is `Books` for Gutenberg and Open Library,
    /// `Academic` is `Papers`).
    pub fn for_adapter(self) -> Category {
        match self {
            Category::Documents => Category::Books,
            Category::Academic => Category::Papers,
            other => other,
        }
    }

    /// Stable id (the same string serde uses).
    pub fn id(self) -> &'static str {
        match self {
            Category::All => "all",
            Category::Papers => "papers",
            Category::Books => "books",
            Category::Music => "music",
            Category::Video => "videos",
            Category::Games => "games",
            Category::Software => "software",
            Category::Images => "images",
            Category::Datasets => "datasets",
            Category::Web => "web",
            Category::Academic => "academic",
            Category::Documents => "documents",
            Category::OpenSource => "open-source",
        }
    }
}

/// The sources. Each is open and legal: no paywalled or pirated content, no torrent indexes
/// (see decision D-research-1).
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
    /// Internet Archive items that are not access-restricted (texts, audio, films).
    InternetArchive,
    /// Wikimedia Commons files (images, audio, video).
    Commons,
    /// Assets of GitHub releases in public repositories (software and open-source games).
    Github,
    /// Data.gov datasets (CKAN).
    DataGov,
    /// Project Gutenberg through Gutendex (public-domain books).
    Gutenberg,
    /// Open Library: fully open Internet Archive scans.
    OpenLibrary,
    /// LibriVox public-domain audiobooks.
    LibriVox,
    /// Jamendo Creative Commons music (needs the user's free client id).
    Jamendo,
    /// Openverse: openly licensed audio and images.
    Openverse,
    /// The Blender Foundation's open movies.
    Blender,
    /// OpenGameArt: freely licensed game assets.
    OpenGameArt,
    /// NASA Image and Video Library.
    Nasa,
    /// General web search through a backend the user chose (Brave Search API or SearXNG).
    Web,
    /// Video pages found with the bundled yt-dlp (`ytsearch`); no key, and nothing is
    /// downloaded until the user adds a result.
    Youtube,
}

impl Source {
    /// Every source, in display order.
    pub const ALL: [Source; 18] = [
        Source::OpenAlex,
        Source::Crossref,
        Source::Arxiv,
        Source::EuropePmc,
        Source::InternetArchive,
        Source::Commons,
        Source::Github,
        Source::DataGov,
        Source::Gutenberg,
        Source::OpenLibrary,
        Source::LibriVox,
        Source::Jamendo,
        Source::Openverse,
        Source::Blender,
        Source::OpenGameArt,
        Source::Nasa,
        Source::Web,
        Source::Youtube,
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
            Source::Gutenberg => "gutenberg",
            Source::OpenLibrary => "open-library",
            Source::LibriVox => "libri-vox",
            Source::Jamendo => "jamendo",
            Source::Openverse => "openverse",
            Source::Blender => "blender",
            Source::OpenGameArt => "open-game-art",
            Source::Nasa => "nasa",
            Source::Web => "web",
            Source::Youtube => "youtube",
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
            Source::Gutenberg => "Project Gutenberg",
            Source::OpenLibrary => "Open Library",
            Source::LibriVox => "LibriVox",
            Source::Jamendo => "Jamendo",
            Source::Openverse => "Openverse",
            Source::Blender => "Blender open movies",
            Source::OpenGameArt => "OpenGameArt",
            Source::Nasa => "NASA",
            Source::Web => "Web",
            Source::Youtube => "YouTube",
        }
    }

    /// The categories this source can answer for (besides [`Category::All`], which every
    /// source belongs to).
    pub fn categories(self) -> &'static [Category] {
        use Category::*;
        match self {
            Source::OpenAlex | Source::Crossref | Source::Arxiv => &[Papers, Academic],
            Source::EuropePmc => &[Papers, Academic, Documents],
            Source::InternetArchive => &[Books, Music, Video, Papers, Documents, Software],
            Source::Commons => &[Music, Video, Images],
            Source::Github => &[Software, Games, OpenSource],
            Source::DataGov => &[Datasets],
            Source::Gutenberg | Source::OpenLibrary => &[Books, Documents],
            Source::LibriVox => &[Books],
            Source::Jamendo => &[Music],
            Source::Openverse => &[Music, Images],
            Source::Blender => &[Video],
            Source::OpenGameArt => &[Games, Images, Music],
            Source::Nasa => &[Images, Video],
            Source::Web => &[Web],
            Source::Youtube => &[Video],
        }
    }

    /// Does this source take part in a search for `category`?
    pub fn serves(self, category: Category) -> bool {
        category == Category::All || self.categories().contains(&category)
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a hit is: a file Riptide can download, or a web page the user can look into.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HitKind {
    /// `download_url` is a direct file link.
    #[default]
    File,
    /// `download_url` is a web page (general web search); files on it are listed on request.
    Page,
}

/// How free the reported licence is, for the badge on a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RightsKind {
    /// Public domain or CC0.
    PublicDomain,
    /// A Creative Commons licence.
    Cc,
    /// An open-source software licence.
    OpenSource,
    /// The source did not say, or the licence is not recognised.
    Unknown,
}

/// The licence badge of one result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rights {
    /// The class (drives the badge colour and wording).
    pub kind: RightsKind,
    /// Short label such as `CC BY 4.0` or `GPL-3.0` (empty for unknown).
    pub label: String,
}

impl Rights {
    /// Classifies a licence exactly as an API reported it (a name, an SPDX id or a URL).
    pub fn classify(license: Option<&str>) -> Rights {
        let unknown = Rights {
            kind: RightsKind::Unknown,
            label: String::new(),
        };
        let Some(raw) = license.map(str::trim).filter(|l| !l.is_empty()) else {
            return unknown;
        };
        let low = raw.to_ascii_lowercase();
        let squashed: String = low
            .chars()
            .map(|c| if c == '_' || c == ' ' { '-' } else { c })
            .collect();
        if low.contains("publicdomain/zero")
            || squashed.starts_with("cc0")
            || squashed.starts_with("cc-0")
        {
            return Rights {
                kind: RightsKind::PublicDomain,
                label: "CC0".into(),
            };
        }
        if low.contains("public domain")
            || low.contains("publicdomain")
            || squashed == "pd"
            || squashed.starts_with("pd-")
            || squashed == "pdm"
            || low.contains("no known copyright")
        {
            return Rights {
                kind: RightsKind::PublicDomain,
                label: "Public domain".into(),
            };
        }
        if squashed.starts_with("oga-by") {
            return Rights {
                kind: RightsKind::Cc,
                label: tidy(raw, 40),
            };
        }
        if let Some(label) = cc_label(&low, &squashed) {
            return Rights {
                kind: RightsKind::Cc,
                label,
            };
        }
        // SPDX ids start with the family name (`GPL-3.0-only`, `Apache-2.0`, `MIT`)
        const OPEN: [&str; 14] = [
            "agpl",
            "lgpl",
            "gpl",
            "apache",
            "mit",
            "bsd",
            "mpl",
            "isc",
            "unlicense",
            "zlib",
            "epl",
            "artistic",
            "ofl",
            "bsl",
        ];
        let first = squashed.split(['-', '/', '(', ')']).next().unwrap_or("");
        if OPEN.contains(&first)
            || low.contains("general public license")
            || low.contains("apache license")
            || low.contains("mit license")
        {
            return Rights {
                kind: RightsKind::OpenSource,
                label: tidy(raw, 40),
            };
        }
        unknown
    }
}

/// `CC BY-SA 4.0` from `https://creativecommons.org/licenses/by-sa/4.0/` or `CC-BY-SA-4.0`.
fn cc_label(low: &str, squashed: &str) -> Option<String> {
    let (terms, version): (String, String) =
        if let Some(rest) = low.split("creativecommons.org/licenses/").nth(1) {
            let mut parts = rest.split('/').filter(|p| !p.is_empty());
            (
                parts.next()?.to_string(),
                parts.next().unwrap_or("").to_string(),
            )
        } else if squashed.starts_with("cc-by") || squashed.starts_with("cc-sa") {
            let rest = squashed.trim_start_matches("cc-");
            let mut ver = String::new();
            let mut t: Vec<&str> = Vec::new();
            for part in rest.split('-') {
                if part.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    ver = part.to_string();
                } else {
                    t.push(part);
                }
            }
            (t.join("-"), ver)
        } else {
            return None;
        };
    let terms = terms.to_ascii_uppercase();
    if !terms.starts_with("BY") && !terms.starts_with("SA") {
        return None;
    }
    Some(format!("CC {terms} {version}").trim().to_string())
}

/// One result: an item with a direct, open download link (or, for web search, a page).
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
    /// A file, or a web page to look into.
    #[serde(default)]
    pub kind: HitKind,
    /// Running time in seconds, only when the source reported one.
    #[serde(default)]
    pub duration_secs: Option<u64>,
    /// Thumbnail image address (http/https), only when the source offered one.
    #[serde(default)]
    pub thumbnail: Option<String>,
    /// Publisher, channel or owner, only when the source named one.
    #[serde(default)]
    pub publisher: Option<String>,
    /// Release or edition version, only when the source stated one (a release tag); never
    /// guessed from the title.
    #[serde(default)]
    pub version: Option<String>,
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
            kind: HitKind::File,
            duration_secs: None,
            thumbnail: None,
            publisher: None,
            version: None,
        }
    }

    /// The licence badge for this result.
    pub fn rights(&self) -> Rights {
        Rights::classify(self.license.as_deref())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn r(l: &str) -> (RightsKind, String) {
        let x = Rights::classify(Some(l));
        (x.kind, x.label)
    }

    #[test]
    fn licences_are_classified_for_the_badge() {
        assert_eq!(r("CC0 1.0").0, RightsKind::PublicDomain);
        assert_eq!(r("Public domain").0, RightsKind::PublicDomain);
        assert_eq!(
            r("https://creativecommons.org/publicdomain/mark/1.0/").0,
            RightsKind::PublicDomain
        );
        assert_eq!(r("PD-US").0, RightsKind::PublicDomain);
        assert_eq!(
            r("https://creativecommons.org/licenses/by-sa/4.0/"),
            (RightsKind::Cc, "CC BY-SA 4.0".into())
        );
        assert_eq!(r("CC BY 4.0"), (RightsKind::Cc, "CC BY 4.0".into()));
        assert_eq!(r("cc-by-nc-3.0").0, RightsKind::Cc);
        assert_eq!(r("GPL-3.0").0, RightsKind::OpenSource);
        assert_eq!(r("Apache-2.0").0, RightsKind::OpenSource);
        assert_eq!(r("MIT").0, RightsKind::OpenSource);
        assert_eq!(r("AGPL-3.0-or-later").0, RightsKind::OpenSource);
        assert_eq!(r("All rights reserved").0, RightsKind::Unknown);
        assert_eq!(r("").0, RightsKind::Unknown);
        assert_eq!(Rights::classify(None).kind, RightsKind::Unknown);
        assert_eq!(r("Permit-Only Commercial").0, RightsKind::Unknown);
    }

    #[test]
    fn every_source_serves_all_and_its_own_categories() {
        for s in Source::ALL {
            assert!(s.serves(Category::All));
            for c in s.categories() {
                assert!(s.serves(*c));
            }
        }
        assert!(Source::Web.serves(Category::Web));
        assert!(!Source::OpenAlex.serves(Category::Music));
        for c in Category::ALL {
            if c != Category::All {
                assert!(
                    Source::ALL.iter().any(|s| s.serves(c)),
                    "{c:?} has no source"
                );
            }
        }
    }

    #[test]
    fn the_seven_categories_map_to_sources_that_serve_them() {
        for c in Category::SEARCH {
            assert_eq!(c.canonical(), Some(c));
            assert!(!c.sources().is_empty(), "{c:?}");
            for s in c.sources() {
                assert!(s.serves(c), "{s:?} does not serve {c:?}");
            }
        }
        assert_eq!(Category::Papers.canonical(), Some(Category::Academic));
        assert_eq!(Category::Books.canonical(), Some(Category::Documents));
        assert_eq!(Category::All.canonical(), None);
        assert_eq!(Category::Music.canonical(), None);
        assert!(Category::Academic
            .sources()
            .iter()
            .all(|s| *s != Source::Github));
        assert_eq!(Category::Documents.for_adapter(), Category::Books);
    }

    #[test]
    fn old_stored_category_ids_still_deserialize() {
        let c = |s: &str| serde_json::from_str::<Category>(&format!("\"{s}\"")).unwrap();
        assert_eq!(c("video"), Category::Video);
        assert_eq!(c("videos"), Category::Video);
        assert_eq!(c("papers"), Category::Papers);
        assert_eq!(c("open-source"), Category::OpenSource);
        assert_eq!(
            serde_json::to_string(&Category::Video).unwrap(),
            "\"videos\""
        );
        for cat in Category::ALL {
            assert_eq!(c(cat.id()), cat);
        }
    }
}
