//! "Have I downloaded this already?" keys: pure functions, no IO.
//!
//! A finished download is remembered under several **keys**, so the same thing is recognised
//! whichever way it is asked for again:
//!
//! | Key | Example | Catches |
//! |---|---|---|
//! | `url:` | `url:https://host/p?a=1` | the same address with another scheme/host case, default port, tracking parameters (`utm_*`, `fbclid`, `gclid`, ...), fragment or query order |
//! | `doi:` | `doi:10.1145/3366423.3380137` | the same paper from a publisher page, `doi.org` or a repository |
//! | `arxiv:` | `arxiv:2101.00001` | any version (`v1`, `v2`) of an arXiv paper, abs or pdf link |
//! | `pmid:` / `pmcid:` | `pmcid:PMC123456` | PubMed / PubMed Central records |
//! | `file:` | `file:report.pdf|1048576` | same file name and size from another URL |
//! | `sha256:` | `sha256:ab12...` | the same bytes under any name or URL (completed files only) |
//!
//! Keys are plain strings so the store can index them without knowing their meaning.

use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use url::Url;

/// Query parameters that only track the visitor and never change what is served.
const TRACKING_EXACT: &[&str] = &[
    "fbclid", "gclid", "gclsrc", "dclid", "msclkid", "yclid", "igshid", "mc_cid", "mc_eid", "_ga",
    "_gl", "twclid", "ttclid", "wbraid", "gbraid", "vero_id", "mkt_tok", "ref_src",
];

fn is_tracking_param(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("utm_") || TRACKING_EXACT.contains(&n.as_str())
}

/// Canonical form of an http(s) URL for comparison; `None` when it is not one.
///
/// Scheme and host are lower-cased, default ports (80/443) dropped, the fragment and any
/// credentials removed, tracking parameters (`utm_*`, `fbclid`, `gclid`, ...) deleted and the
/// remaining query pairs sorted. An empty path becomes `/`. Idempotent:
/// `normalize_url(normalize_url(x)) == normalize_url(x)`.
pub fn normalize_url(raw: &str) -> Option<String> {
    let mut url = Url::parse(raw.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_tracking_param(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    if pairs.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(&pairs);
    }
    Some(url.into())
}

fn doi_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\b(10\.\d{4,9}/[^\s"'<>?#]+)"#).expect("static regex"))
}

fn arxiv_new_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)arxiv(?:\.org/(?:abs|pdf)/|:)(\d{4}\.\d{4,5})(?:v\d+)?")
            .expect("static regex")
    })
}

fn arxiv_old_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)arxiv(?:\.org/(?:abs|pdf)/|:)([a-z\-]+(?:\.[a-z]{2})?/\d{7})(?:v\d+)?")
            .expect("static regex")
    })
}

fn pmcid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(PMC\d{3,9})\b").expect("static regex"))
}

fn pmid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(?:pubmed\.ncbi\.nlm\.nih\.gov/|europepmc\.org/article/MED/|pmid[:=\s]+)(\d{1,9})")
            .expect("static regex")
    })
}

/// Lower-cased DOI without a trailing sentence punctuation, or `None` when `s` has none.
pub fn extract_doi(s: &str) -> Option<String> {
    let decoded = s.replace("%2F", "/").replace("%2f", "/");
    let m = doi_re().captures(&decoded)?.get(1)?.as_str();
    let m = m.trim_end_matches(['.', ',', ';', ')', ']', '}']);
    // a DOI never ends in a file extension added by a link; keep it as published
    Some(m.to_ascii_lowercase())
}

/// The versionless arXiv id (`2101.00001`, or `hep-th/9901001`) mentioned in `s`.
pub fn extract_arxiv(s: &str) -> Option<String> {
    let s = s.trim_end_matches(".pdf");
    arxiv_new_re()
        .captures(s)
        .or_else(|| arxiv_old_re().captures(s))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_ascii_lowercase())
}

/// What a download or a search hit is known by. Every field is optional; empty means unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity<'a> {
    /// Address the file is fetched from.
    pub url: Option<&'a str>,
    /// Explicit DOI, if the source states one.
    pub doi: Option<&'a str>,
    /// Explicit arXiv id (any version).
    pub arxiv: Option<&'a str>,
    /// Explicit PubMed id.
    pub pmid: Option<&'a str>,
    /// Explicit PubMed Central id.
    pub pmcid: Option<&'a str>,
    /// File name as saved or offered.
    pub filename: Option<&'a str>,
    /// Size in bytes when known.
    pub size: Option<u64>,
    /// SHA-256 (lower-case hex) of the finished file.
    pub sha256: Option<&'a str>,
}

/// Every key `id` is known by, sorted and de-duplicated. Identifiers found inside the URL
/// (an `arxiv.org/pdf/...` or `doi.org/...` link) count too.
pub fn keys(id: &Identity<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(u) = id.url {
        if let Some(n) = normalize_url(u) {
            out.push(format!("url:{n}"));
        }
        if let Some(d) = extract_doi(u) {
            out.push(format!("doi:{d}"));
        }
        if let Some(a) = extract_arxiv(u) {
            out.push(format!("arxiv:{a}"));
        }
        if let Some(c) = pmcid_re().captures(u).and_then(|c| c.get(1)) {
            out.push(format!("pmcid:{}", c.as_str().to_ascii_uppercase()));
        }
        if let Some(c) = pmid_re().captures(u).and_then(|c| c.get(1)) {
            out.push(format!("pmid:{}", c.as_str()));
        }
    }
    if let Some(d) = id.doi.and_then(extract_doi) {
        out.push(format!("doi:{d}"));
    }
    if let Some(a) = id.arxiv {
        let a = extract_arxiv(&format!("arxiv:{a}")).unwrap_or_else(|| a.to_ascii_lowercase());
        out.push(format!("arxiv:{a}"));
    }
    if let Some(p) = id.pmid.map(str::trim).filter(|p| !p.is_empty()) {
        out.push(format!("pmid:{p}"));
    }
    if let Some(p) = id.pmcid.map(str::trim).filter(|p| !p.is_empty()) {
        let p = p.to_ascii_uppercase();
        let p = if p.starts_with("PMC") {
            p
        } else {
            format!("PMC{p}")
        };
        out.push(format!("pmcid:{p}"));
    }
    if let (Some(name), Some(size)) = (id.filename, id.size) {
        let name: String = name.trim().nfc().collect::<String>().to_lowercase();
        if !name.is_empty() && size > 0 {
            out.push(format!("file:{name}|{size}"));
        }
    }
    if let Some(h) = id.sha256.map(str::trim).filter(|h| h.len() == 64) {
        out.push(format!("sha256:{}", h.to_ascii_lowercase()));
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn variants_of_one_url_normalise_alike() {
        let base = normalize_url("https://example.com/files/a.zip?x=1&y=2").unwrap();
        for v in [
            "HTTPS://Example.COM:443/files/a.zip?y=2&x=1",
            "https://example.com/files/a.zip?utm_source=n&x=1&y=2&fbclid=abc#frag",
            "https://user:pw@example.com/files/a.zip?x=1&y=2&gclid=zz",
            "  https://example.com:443/files/a.zip?x=1&y=2  ",
        ] {
            assert_eq!(normalize_url(v).unwrap(), base, "{v}");
        }
        assert_eq!(
            normalize_url("http://example.com:80").unwrap(),
            "http://example.com/"
        );
        assert_ne!(
            normalize_url("http://example.com:8080/").unwrap(),
            normalize_url("http://example.com/").unwrap()
        );
        assert!(normalize_url("ftp://x/y").is_none());
        assert!(normalize_url("not a url").is_none());
    }

    #[test]
    fn real_parameters_survive() {
        assert_eq!(
            normalize_url("https://h/p?id=7&utm_medium=x").unwrap(),
            "https://h/p?id=7"
        );
        assert_ne!(
            normalize_url("https://h/p?id=7").unwrap(),
            normalize_url("https://h/p?id=8").unwrap()
        );
    }

    #[test]
    fn identifiers_are_found_in_links() {
        assert_eq!(
            extract_doi("https://doi.org/10.1145/3366423.3380137").as_deref(),
            Some("10.1145/3366423.3380137")
        );
        assert_eq!(
            extract_doi("https://doi.org/10.1000/ABC.def.").as_deref(),
            Some("10.1000/abc.def")
        );
        assert_eq!(
            extract_arxiv("https://arxiv.org/pdf/2101.00001v3.pdf").as_deref(),
            Some("2101.00001")
        );
        assert_eq!(
            extract_arxiv("https://arxiv.org/abs/hep-th/9901001v2").as_deref(),
            Some("hep-th/9901001")
        );
        assert_eq!(extract_arxiv("https://example.com/2101.00001"), None);
    }

    #[test]
    fn arxiv_versions_share_a_key() {
        let a = keys(&Identity {
            url: Some("https://arxiv.org/pdf/2101.00001v1"),
            ..Identity::default()
        });
        let b = keys(&Identity {
            arxiv: Some("2101.00001v7"),
            ..Identity::default()
        });
        assert!(a.contains(&"arxiv:2101.00001".to_string()));
        assert!(b.contains(&"arxiv:2101.00001".to_string()));
    }

    #[test]
    fn file_and_hash_keys() {
        let k = keys(&Identity {
            filename: Some("Report.PDF"),
            size: Some(10),
            sha256: Some(&"AB".repeat(32)),
            pmcid: Some("1234567"),
            ..Identity::default()
        });
        assert!(k.contains(&"file:report.pdf|10".to_string()));
        assert!(k.contains(&format!("sha256:{}", "ab".repeat(32))));
        assert!(k.contains(&"pmcid:PMC1234567".to_string()));
        // no size, no file key (a name alone proves nothing)
        assert!(keys(&Identity {
            filename: Some("a.zip"),
            ..Identity::default()
        })
        .is_empty());
    }

    proptest! {
        #[test]
        fn normalising_is_idempotent(
            host in "[a-zA-Z]{1,8}\\.(com|org)",
            path in "(/[a-zA-Z0-9._~-]{0,6}){0,3}",
            q in proptest::collection::vec(("[a-z_]{1,6}", "[a-z0-9]{0,4}"), 0..5),
            frag in proptest::option::of("[a-z]{0,4}"),
        ) {
            let query = q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
            let mut raw = format!("https://{host}{path}");
            if !query.is_empty() { raw.push('?'); raw.push_str(&query); }
            if let Some(f) = frag { raw.push('#'); raw.push_str(&f); }
            if let Some(n) = normalize_url(&raw) {
                prop_assert_eq!(normalize_url(&n), Some(n.clone()));
                prop_assert!(!n.contains('#'));
            }
        }

        #[test]
        fn query_order_and_tracking_never_matter(
            q in proptest::collection::vec(("[a-z]{1,5}", "[a-z0-9]{0,4}"), 1..5),
        ) {
            prop_assume!(q.iter().all(|(k, _)| !is_tracking_param(k)));
            let fwd = q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
            let rev = q.iter().rev().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
            let a = normalize_url(&format!("https://h.com/p?{fwd}"));
            let b = normalize_url(&format!("HTTPS://H.com:443/p?utm_x=1&{rev}&fbclid=2#z"));
            prop_assert_eq!(a, b);
        }
    }
}
