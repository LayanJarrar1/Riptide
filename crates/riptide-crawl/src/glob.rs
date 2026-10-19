//! A tiny glob matcher for URL paths: `*` (anything but `/`), `**` (anything, `/` included),
//! `?` (one character but `/`), everything else literal, case-insensitive.

use percent_encoding::percent_decode_str;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Lit(char),
    /// `*`
    Star,
    /// `**`
    DoubleStar,
    /// `**/`: zero or more whole directories
    Dirs,
    /// `?`
    Any,
}

/// A compiled pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    toks: Vec<Tok>,
    /// The pattern has a `/`: match the whole path; else only the file name.
    whole_path: bool,
}

impl Glob {
    /// Compiles `pattern`. A pattern with a `/` is matched against the whole URL path (leading
    /// slash optional: `docs/**` equals `/docs/**`); one without is matched against the last
    /// path segment, so `*.pdf` finds PDFs anywhere. `**/` matches zero or more directories.
    #[must_use]
    pub fn new(pattern: &str) -> Glob {
        let pattern = pattern.trim().to_lowercase();
        let whole_path = pattern.contains('/');
        let pattern = if whole_path {
            pattern.trim_start_matches('/')
        } else {
            &pattern
        };
        let chars: Vec<char> = pattern.chars().collect();
        let mut toks = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '*' if chars.get(i + 1) == Some(&'*') => {
                    if chars.get(i + 2) == Some(&'/') {
                        toks.push(Tok::Dirs);
                        i += 3;
                    } else {
                        toks.push(Tok::DoubleStar);
                        i += 2;
                    }
                }
                '*' => {
                    toks.push(Tok::Star);
                    i += 1;
                }
                '?' => {
                    toks.push(Tok::Any);
                    i += 1;
                }
                c => {
                    toks.push(Tok::Lit(c));
                    i += 1;
                }
            }
        }
        Glob { toks, whole_path }
    }

    /// Whether the (percent-decoded) path of `url` matches.
    #[must_use]
    pub fn matches_url(&self, url: &Url) -> bool {
        let path = percent_decode_str(url.path())
            .decode_utf8_lossy()
            .to_lowercase();
        let text = if self.whole_path {
            path.trim_start_matches('/')
        } else {
            path.rsplit('/').next().unwrap_or("")
        };
        self.matches_text(text)
    }

    /// Matches `text` (already lower-cased) against the pattern.
    fn matches_text(&self, text: &str) -> bool {
        let text: Vec<char> = text.chars().collect();
        let n = text.len();
        // reach[j]: the tokens consumed so far can end exactly at text[j]
        let mut reach = vec![false; n + 1];
        reach[0] = true;
        for tok in &self.toks {
            let mut next = vec![false; n + 1];
            for j in 0..=n {
                if !reach[j] {
                    continue;
                }
                match tok {
                    Tok::Lit(c) => {
                        if j < n && text[j] == *c {
                            next[j + 1] = true;
                        }
                    }
                    Tok::Any => {
                        if j < n && text[j] != '/' {
                            next[j + 1] = true;
                        }
                    }
                    Tok::Star => {
                        next[j] = true;
                        let mut k = j;
                        while k < n && text[k] != '/' {
                            k += 1;
                            next[k] = true;
                        }
                    }
                    Tok::DoubleStar => {
                        for slot in &mut next[j..] {
                            *slot = true;
                        }
                    }
                    Tok::Dirs => {
                        next[j] = true;
                        for k in j..n {
                            if text[k] == '/' {
                                next[k + 1] = true;
                            }
                        }
                    }
                }
            }
            reach = next;
        }
        reach[n]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, url: &str) -> bool {
        Glob::new(pattern).matches_url(&Url::parse(url).unwrap())
    }

    #[test]
    fn file_name_patterns() {
        assert!(m("*.pdf", "http://h/a/b/Report.PDF"));
        assert!(!m("*.pdf", "http://h/a/b/report.pdf.html"));
        assert!(m("report-?.pdf", "http://h/x/report-1.pdf"));
        assert!(!m("report-?.pdf", "http://h/x/report-12.pdf"));
        assert!(m("*.pdf", "http://h/%D9%85%D9%84%D9%81.pdf"));
        assert!(m("ملف*", "http://h/%D9%85%D9%84%D9%81.pdf"));
    }

    #[test]
    fn path_patterns() {
        assert!(m("/private/**", "http://h/private/a/b.zip"));
        assert!(m("private/**", "http://h/private/a/b.zip"));
        assert!(!m("/private/**", "http://h/public/private/b.zip"));
        assert!(m("**/private/**", "http://h/public/private/b.zip"));
        assert!(m("**/private/**", "http://h/private/b.zip"));
        assert!(m("/docs/*/index.html", "http://h/docs/en/index.html"));
        assert!(!m("/docs/*/index.html", "http://h/docs/en/x/index.html"));
        assert!(m("/docs/**/index.html", "http://h/docs/en/x/index.html"));
        assert!(m("/docs/**/index.html", "http://h/docs/index.html"));
        assert!(m("/a/**", "http://h/a/"));
    }

    #[test]
    fn query_is_not_part_of_the_match() {
        assert!(m("*.zip", "http://h/f.zip?token=1"));
    }

    #[test]
    fn pathological_patterns_stay_fast() {
        let g = Glob::new("*a*a*a*a*a*a*a*a*a*a*a*a*b");
        let url = Url::parse(&format!("http://h/{}", "a".repeat(500))).unwrap();
        assert!(!g.matches_url(&url));
    }
}
