//! Batch URL patterns and URL lists: pure text in, URLs out. No IO, no URL crate.
//!
//! # Pattern syntax
//!
//! A pattern is an `http(s)` URL with any number of `[from-to]` or `[from-to:step]` groups:
//!
//! * numeric: `file[1-10].zip`, `file[001-120].zip` (a leading zero on either end keeps the
//!   width: `001`, `002`, ... `120`), `part[0-100:5]` (0, 5, 10, ...). `from` may be larger than
//!   `to` to count down.
//! * letters: `img_[a-z].jpg`, `img_[A-F].jpg` (one ASCII letter on each end, same case).
//! * several groups: the cartesian product, in reading order - the rightmost group changes
//!   fastest, like nested loops written left to right (`a[1-2]b[x-y]` gives `a1bx`, `a1by`,
//!   `a2bx`, `a2by`).
//! * a literal bracket is written `\[` (and `\]`, `\\` for a literal backslash).
//!
//! The expansion is capped at [`MAX_EXPANSION`] URLs; the size is checked before anything is
//! generated, so a pattern like `[1-99999999]` costs nothing to reject.
//!
//! Lists ([`parse_url_list`]) are the plain-text format used for import and export: one URL per
//! line, blank lines and `#` comments ignored.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::download::{CategoryId, DownloadRequest, Priority, QueueId};

/// The most URLs one pattern may expand to.
pub const MAX_EXPANSION: usize = 10_000;

/// Why a pattern could not be expanded. Serialized with a stable `code` so the UI can show a
/// localized message; positions are byte offsets into the trimmed pattern.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum BatchError {
    /// Nothing to expand.
    #[error("the pattern is empty")]
    Empty,
    /// The pattern does not start with `http://` or `https://` followed by a host.
    #[error("only http:// and https:// addresses can be downloaded")]
    NotHttp,
    /// Whitespace or a control character; URLs cannot contain them.
    #[error("the pattern contains a space or control character at {at}")]
    InvalidChar {
        /// Byte offset.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        at: usize,
    },
    /// A `[` without its `]`.
    #[error("the bracket at {at} is never closed (write \\[ for a literal bracket)")]
    Unclosed {
        /// Byte offset of the `[`.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        at: usize,
    },
    /// The group is not `[1-10]`, `[001-120]`, `[a-z]` or one of those with `:step`.
    #[error("the range at {at} is not valid")]
    InvalidRange {
        /// Byte offset of the `[`.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        at: usize,
    },
    /// A step of `0`.
    #[error("the step of the range at {at} must be at least 1")]
    ZeroStep {
        /// Byte offset of the `[`.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        at: usize,
    },
    /// A number with more than 18 digits.
    #[error("a number in the range at {at} is too large")]
    NumberTooLarge {
        /// Byte offset of the `[`.
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        at: usize,
    },
    /// The pattern expands to more than `limit` URLs.
    #[error("the pattern expands to more than {limit} addresses")]
    TooMany {
        /// The cap ([`MAX_EXPANSION`]).
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        limit: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Range {
    Number {
        start: u64,
        end: u64,
        step: u64,
        width: usize,
    },
    Letter {
        start: u8,
        end: u8,
        step: u64,
    },
}

impl Range {
    fn count(&self) -> u64 {
        let (a, b, step) = match *self {
            Range::Number {
                start, end, step, ..
            } => (start, end, step),
            Range::Letter { start, end, step } => (u64::from(start), u64::from(end), step),
        };
        a.abs_diff(b) / step + 1
    }

    /// Appends the `k`-th value, `k < count()`.
    fn push_value(&self, k: u64, out: &mut String) {
        match *self {
            Range::Number {
                start,
                end,
                step,
                width,
            } => {
                let v = if start <= end {
                    start + k * step
                } else {
                    start - k * step
                };
                out.push_str(&format!("{v:0width$}"));
            }
            Range::Letter { start, end, step } => {
                let v = if start <= end {
                    u64::from(start) + k * step
                } else {
                    u64::from(start) - k * step
                };
                out.push(char::from(u8::try_from(v).unwrap_or(b'?')));
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Range(Range),
}

fn parse_number(s: &str, at: usize) -> Result<u64, BatchError> {
    if s.len() > 18 {
        return Err(BatchError::NumberTooLarge { at });
    }
    s.parse().map_err(|_| BatchError::InvalidRange { at })
}

fn parse_range(body: &str, at: usize) -> Result<Range, BatchError> {
    let invalid = BatchError::InvalidRange { at };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (span, step) = match body.split_once(':') {
        Some((span, step)) => {
            if !digits(step) {
                return Err(invalid);
            }
            let step = parse_number(step, at)?;
            if step == 0 {
                return Err(BatchError::ZeroStep { at });
            }
            (span, step)
        }
        None => (body, 1),
    };
    let (from, to) = span.split_once('-').ok_or_else(|| invalid.clone())?;
    if digits(from) && digits(to) {
        let padded = |s: &str| s.len() > 1 && s.starts_with('0');
        let width = if padded(from) || padded(to) {
            from.len().max(to.len())
        } else {
            0
        };
        return Ok(Range::Number {
            start: parse_number(from, at)?,
            end: parse_number(to, at)?,
            step,
            width,
        });
    }
    match (from.as_bytes(), to.as_bytes()) {
        ([a], [b])
            if (a.is_ascii_lowercase() && b.is_ascii_lowercase())
                || (a.is_ascii_uppercase() && b.is_ascii_uppercase()) =>
        {
            Ok(Range::Letter {
                start: *a,
                end: *b,
                step,
            })
        }
        _ => Err(invalid),
    }
}

fn parse(pattern: &str) -> Result<Vec<Part>, BatchError> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = pattern.char_indices();
    let mut pending: Option<(usize, char)> = None;
    while let Some((at, c)) = pending.take().or_else(|| chars.next()) {
        match c {
            '\\' => match chars.next() {
                Some((_, next @ ('[' | ']' | '\\'))) => literal.push(next),
                other => {
                    literal.push('\\');
                    pending = other;
                }
            },
            '[' => {
                let rest = &pattern[at + 1..];
                let close = rest.find(']').ok_or(BatchError::Unclosed { at })?;
                let body = &rest[..close];
                if body.contains('[') {
                    return Err(BatchError::InvalidRange { at });
                }
                let range = parse_range(body, at)?;
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Range(range));
                // consume the body and the closing bracket
                for _ in 0..body.chars().count() + 1 {
                    chars.next();
                }
            }
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(parts)
}

fn check_shape(pattern: &str) -> Result<(), BatchError> {
    if pattern.is_empty() {
        return Err(BatchError::Empty);
    }
    if let Some((at, _)) = pattern
        .char_indices()
        .find(|(_, c)| c.is_whitespace() || c.is_control())
    {
        return Err(BatchError::InvalidChar { at });
    }
    Ok(())
}

fn total(parts: &[Part]) -> Result<usize, BatchError> {
    let mut n: u128 = 1;
    for part in parts {
        if let Part::Range(r) = part {
            n = n.saturating_mul(u128::from(r.count()));
            if n > MAX_EXPANSION as u128 {
                return Err(BatchError::TooMany {
                    limit: MAX_EXPANSION,
                });
            }
        }
    }
    Ok(n as usize)
}

/// How many URLs `pattern` expands to, without generating them.
pub fn count(pattern: &str) -> Result<usize, BatchError> {
    let pattern = pattern.trim();
    check_shape(pattern)?;
    total(&parse(pattern)?)
}

/// Expands `pattern` into its URLs, in order (see the module docs for the syntax). Every
/// returned string is a syntactically valid `http(s)` URL ([`is_http_url`]); a pattern whose
/// expansion would not be (for example a range inside the scheme) is rejected with
/// [`BatchError::NotHttp`].
pub fn expand(pattern: &str) -> Result<Vec<String>, BatchError> {
    let pattern = pattern.trim();
    check_shape(pattern)?;
    let parts = parse(pattern)?;
    let n = total(&parts)?;
    let counts: Vec<u64> = parts
        .iter()
        .map(|p| match p {
            Part::Range(r) => r.count(),
            Part::Literal(_) => 1,
        })
        .collect();
    let mut urls = Vec::with_capacity(n);
    for mut index in 0..n as u64 {
        // mixed radix, rightmost group fastest
        let mut picks = vec![0u64; parts.len()];
        for i in (0..parts.len()).rev() {
            picks[i] = index % counts[i];
            index /= counts[i];
        }
        let mut url = String::with_capacity(pattern.len() + 8);
        for (part, &k) in parts.iter().zip(&picks) {
            match part {
                Part::Literal(s) => url.push_str(s),
                Part::Range(r) => r.push_value(k, &mut url),
            }
        }
        if !is_http_url(&url) {
            return Err(BatchError::NotHttp);
        }
        urls.push(url);
    }
    Ok(urls)
}

/// Options shared by every item of a batch; [`BatchOptions::requests`] stamps them onto one
/// [`DownloadRequest`] per URL.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BatchOptions {
    /// Destination folder; `None` = by category.
    #[serde(default)]
    pub dir: Option<PathBuf>,
    /// Category override; `None` = by extension.
    #[serde(default)]
    pub category_id: Option<CategoryId>,
    /// Queue; `None` = *Main*.
    #[serde(default)]
    pub queue_id: Option<QueueId>,
    /// Priority; `None` = normal.
    #[serde(default)]
    pub priority: Option<Priority>,
    /// Connections per download; `None` = the settings default.
    #[serde(default)]
    pub connections: Option<u32>,
    /// Add as `Paused` ("start later") instead of `Queued`.
    #[serde(default)]
    pub start_paused: bool,
    /// Referrer page sent with every item.
    #[serde(default)]
    pub referrer: Option<String>,
}

impl BatchOptions {
    /// One request per URL, in order, all carrying these options.
    pub fn requests<S: AsRef<str>>(&self, urls: &[S]) -> Vec<DownloadRequest> {
        urls.iter()
            .map(|url| DownloadRequest {
                url: url.as_ref().to_owned(),
                dir: self.dir.clone(),
                queue_id: self.queue_id,
                category_id: self.category_id,
                priority: self.priority,
                connections: self.connections,
                start_paused: self.start_paused,
                referrer: self.referrer.clone(),
                ..DownloadRequest::default()
            })
            .collect()
    }
}

/// Whether `s` looks like an `http://` or `https://` URL with a host and no whitespace or
/// control characters. A syntactic check only; the engine parses properly when it adds.
pub fn is_http_url(s: &str) -> bool {
    let has_prefix = |p: &str| s.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p));
    let rest = if has_prefix("https://") {
        &s[8..]
    } else if has_prefix("http://") {
        &s[7..]
    } else {
        return false;
    };
    !rest.is_empty()
        && !rest.starts_with(['/', '?', '#'])
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// What [`import_url_list`] found in a pasted or loaded text.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UrlList {
    /// The accepted URLs, in input order, without repeats.
    pub urls: Vec<String>,
    /// Non-blank, non-comment lines that are not `http(s)` URLs.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub ignored: usize,
    /// Lines that repeated an earlier URL.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub duplicates: usize,
}

/// Reads a URL list: one URL per line; blank lines and lines starting with `#` are skipped;
/// only `http(s)` URLs are kept; repeats are dropped keeping the first. Counts what was
/// dropped so the UI can say so.
pub fn import_url_list(text: &str) -> UrlList {
    let mut seen = HashSet::new();
    let mut list = UrlList::default();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !is_http_url(line) {
            list.ignored += 1;
        } else if seen.insert(line.to_owned()) {
            list.urls.push(line.to_owned());
        } else {
            list.duplicates += 1;
        }
    }
    list
}

/// [`import_url_list`] without the counters.
pub fn parse_url_list(text: &str) -> Vec<String> {
    import_url_list(text).urls
}

/// The text form of a list: one URL per line, newline-terminated (empty for an empty list).
/// `parse_url_list(&export_url_list(&urls))` gives `urls` back.
pub fn export_url_list<S: AsRef<str>>(urls: &[S]) -> String {
    let mut out = String::new();
    for url in urls {
        out.push_str(url.as_ref());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ok(p: &str) -> Vec<String> {
        expand(p).unwrap()
    }

    #[test]
    fn zero_padded_range_keeps_its_width() {
        let urls = ok("https://h/file[001-120].zip");
        assert_eq!(urls.len(), 120);
        assert_eq!(urls[0], "https://h/file001.zip");
        assert_eq!(urls[9], "https://h/file010.zip");
        assert_eq!(urls[119], "https://h/file120.zip");
    }

    #[test]
    fn unpadded_descending_letters_and_step() {
        assert_eq!(
            ok("http://h/[8-10]"),
            ["http://h/8", "http://h/9", "http://h/10"]
        );
        assert_eq!(ok("http://h/[0-3]").len(), 4);
        assert_eq!(
            ok("http://h/[3-1]"),
            ["http://h/3", "http://h/2", "http://h/1"]
        );
        assert_eq!(ok("http://h/img_[a-d].jpg").len(), 4);
        assert_eq!(
            ok("http://h/[C-A]"),
            ["http://h/C", "http://h/B", "http://h/A"]
        );
        let stepped = ok("http://h/p[0-100:5]");
        assert_eq!(stepped.len(), 21);
        assert_eq!(stepped[1], "http://h/p5");
        assert_eq!(stepped[20], "http://h/p100");
        assert_eq!(
            ok("http://h/p[0-9:4]"),
            ["http://h/p0", "http://h/p4", "http://h/p8"]
        );
        assert_eq!(
            ok("http://h/[a-z:5]"),
            [
                "http://h/a",
                "http://h/f",
                "http://h/k",
                "http://h/p",
                "http://h/u",
                "http://h/z"
            ]
        );
    }

    #[test]
    fn several_ranges_are_a_cartesian_product_rightmost_fastest() {
        assert_eq!(
            ok("http://h/a[1-2]b[x-y]"),
            [
                "http://h/a1bx",
                "http://h/a1by",
                "http://h/a2bx",
                "http://h/a2by"
            ]
        );
        assert_eq!(ok("http://h[1-3].example/v[01-10]/f[a-c]").len(), 90);
    }

    #[test]
    fn a_literal_bracket_is_escaped() {
        assert_eq!(
            ok(r"http://h/a\[1\]-[1-2]"),
            ["http://h/a[1]-1", "http://h/a[1]-2"]
        );
        assert_eq!(ok("http://h/x]y"), ["http://h/x]y"]);
        assert_eq!(ok(r"http://h/a\\b"), [r"http://h/a\b"]);
        assert_eq!(ok(r"http://h/a\b[1-1]"), [r"http://h/a\b1"]);
    }

    #[test]
    fn no_range_is_a_single_url_and_whitespace_is_trimmed() {
        assert_eq!(ok("  http://h/a.zip \n"), ["http://h/a.zip"]);
    }

    #[test]
    fn errors_are_typed() {
        assert_eq!(expand("  "), Err(BatchError::Empty));
        assert_eq!(expand("ftp://h/[1-2]"), Err(BatchError::NotHttp));
        assert_eq!(expand("http:///[1-2]"), Err(BatchError::NotHttp));
        assert_eq!(expand("htt[p-q]://h/"), Err(BatchError::NotHttp));
        let at9 = |e: fn(usize) -> BatchError| Err(e(9));
        assert_eq!(
            expand("http://h/[1-2"),
            at9(|at| BatchError::Unclosed { at })
        );
        let invalid = at9(|at| BatchError::InvalidRange { at });
        for p in [
            "http://h/[1-]",
            "http://h/[a-5]",
            "http://h/[a-Z]",
            "http://h/[ab-cd]",
            "http://h/[1-5:x]",
            "http://h/[1-[2]",
            "http://h/[]",
            "http://h/[5]",
        ] {
            assert_eq!(expand(p), invalid, "{p}");
        }
        assert_eq!(
            expand("http://h/[1-5:0]"),
            at9(|at| BatchError::ZeroStep { at })
        );
        assert_eq!(
            expand("http://h/[1-1234567890123456789]"),
            at9(|at| BatchError::NumberTooLarge { at })
        );
        assert_eq!(
            expand("http://h/a b"),
            Err(BatchError::InvalidChar { at: 10 })
        );
    }

    #[test]
    fn the_cap_is_enforced_before_generating() {
        assert_eq!(ok("http://h/[1-10000]").len(), MAX_EXPANSION);
        let err = BatchError::TooMany {
            limit: MAX_EXPANSION,
        };
        let too_many = Err(err.clone());
        assert_eq!(expand("http://h/[1-10001]"), too_many);
        assert_eq!(count("http://h/[1-99999999999]"), Err(err.clone()));
        assert_eq!(expand("http://h/[1-100][1-101]"), too_many);
        assert_eq!(count("http://h/[1-100][1-100]"), Ok(10_000));
        assert_eq!(count("http://h/[0-999999999999999999][a-z][a-z]"), Err(err));
    }

    #[test]
    fn non_ascii_text_around_ranges_is_kept() {
        assert_eq!(
            ok("http://h/ملف[1-2].zip"),
            ["http://h/ملف1.zip", "http://h/ملف2.zip"]
        );
        assert_eq!(ok("http://h/ملف[1-2]é[a-b]").len(), 4);
    }

    #[test]
    fn url_list_import_filters_dedups_and_keeps_order() {
        let text = "\u{feff}# my list\n\nhttps://a/1.zip\r\n  http://b/2.zip  \nftp://c/3\nnot a url\nhttps://a/1.zip\nhttp://b/2.zip\n#https://d/4\nHTTPS://E/5\n";
        let list = import_url_list(text);
        assert_eq!(
            list.urls,
            ["https://a/1.zip", "http://b/2.zip", "HTTPS://E/5"]
        );
        assert_eq!(list.ignored, 2);
        assert_eq!(list.duplicates, 2);
        assert_eq!(parse_url_list(text), list.urls);
    }

    #[test]
    fn export_round_trips() {
        let urls = vec!["http://a/1".to_string(), "https://b/2".to_string()];
        let text = export_url_list(&urls);
        assert_eq!(text, "http://a/1\nhttps://b/2\n");
        assert_eq!(parse_url_list(&text), urls);
        assert_eq!(export_url_list::<String>(&[]), "");
    }

    #[test]
    fn options_are_copied_into_every_request() {
        let options = BatchOptions {
            dir: Some(PathBuf::from("D:/Batch")),
            connections: Some(4),
            start_paused: true,
            ..BatchOptions::default()
        };
        let requests = options.requests(&["http://a/1", "http://a/2"]);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].url, "http://a/2");
        assert!(requests.iter().all(|r| r.start_paused
            && r.connections == Some(4)
            && r.dir.as_deref() == Some(std::path::Path::new("D:/Batch"))
            && r.filename.is_none()));
    }

    #[test]
    fn is_http_url_basics() {
        assert!(is_http_url("http://h"));
        assert!(is_http_url("HTTPS://h/a?b#c"));
        assert!(!is_http_url("http://"));
        assert!(!is_http_url("http:///x"));
        assert!(!is_http_url("httpx://h"));
        assert!(!is_http_url("http://h/a b"));
        assert!(!is_http_url(""));
    }

    #[derive(Debug, Clone)]
    enum Spec {
        Num {
            a: u64,
            b: u64,
            step: u64,
            pad: usize,
        },
        Alpha {
            a: u8,
            b: u8,
            step: u64,
            upper: bool,
        },
    }

    fn spec() -> impl Strategy<Value = Spec> {
        prop_oneof![
            (0u64..20, 0u64..20, 1u64..7, 0usize..4).prop_map(|(a, b, step, pad)| Spec::Num {
                a,
                b,
                step,
                pad
            }),
            (0u8..14, 0u8..14, 1u64..5, any::<bool>())
                .prop_map(|(a, b, step, upper)| Spec::Alpha { a, b, step, upper }),
        ]
    }

    impl Spec {
        fn text(&self) -> String {
            match self {
                Spec::Num { a, b, step, pad } => {
                    format!("[{a:0pad$}-{b:0pad$}:{step}]", pad = *pad)
                }
                Spec::Alpha { a, b, step, upper } => {
                    let base = if *upper { b'A' } else { b'a' };
                    format!("[{}-{}:{step}]", char::from(base + a), char::from(base + b))
                }
            }
        }
        fn size(&self) -> usize {
            match self {
                Spec::Num { a, b, step, .. } => (a.abs_diff(*b) / step + 1) as usize,
                Spec::Alpha { a, b, step, .. } => (u64::from(a.abs_diff(*b)) / step + 1) as usize,
            }
        }
    }

    proptest! {
        #[test]
        fn count_is_the_product_of_range_sizes(specs in proptest::collection::vec(spec(), 0..4)) {
            let mut pattern = String::from("https://host.example/");
            let mut product = 1usize;
            for (i, s) in specs.iter().enumerate() {
                pattern.push_str(&format!("p{i}_{}", s.text()));
                product *= s.size();
            }
            let urls = expand(&pattern).unwrap();
            prop_assert_eq!(urls.len(), product);
            prop_assert_eq!(count(&pattern).unwrap(), product);
            prop_assert!(urls.iter().all(|u| is_http_url(u)));
            let unique: HashSet<_> = urls.iter().collect();
            prop_assert_eq!(unique.len(), urls.len());
            prop_assert_eq!(expand(&pattern).unwrap(), urls);
        }

        #[test]
        fn single_numeric_range_lists_exact_values(a in 0u64..500, b in 0u64..500, step in 1u64..9) {
            let urls = expand(&format!("http://h/[{a}-{b}:{step}]")).unwrap();
            let n = a.abs_diff(b) / step + 1;
            let expected: Vec<String> = (0..n)
                .map(|k| format!("http://h/{}", if a <= b { a + k * step } else { a - k * step }))
                .collect();
            prop_assert_eq!(urls, expected);
        }

        #[test]
        fn expand_never_panics_and_respects_the_cap(s in "\\PC{0,60}") {
            if let Ok(urls) = expand(&s) {
                prop_assert!(urls.len() <= MAX_EXPANSION);
                prop_assert!(urls.iter().all(|u| is_http_url(u)));
            }
        }

        #[test]
        fn list_round_trips(urls in proptest::collection::vec("https://[a-z]{1,8}\\.test/[a-z0-9]{0,8}", 0..20)) {
            let mut seen = HashSet::new();
            let unique: Vec<String> = urls.into_iter().filter(|u| seen.insert(u.clone())).collect();
            prop_assert_eq!(parse_url_list(&export_url_list(&unique)), unique);
        }
    }
}
