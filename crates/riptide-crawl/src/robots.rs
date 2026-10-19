//! `robots.txt` (RFC 9309, the parts a download manager needs): `User-agent` groups,
//! `Allow` / `Disallow` with `*` and `$`, longest match wins (ties go to `Allow`), `Crawl-delay`.

use std::time::Duration;

/// The product token Riptide announces; a group addressed to it beats the `*` group.
pub const AGENT: &str = "riptide";

/// Longest `Crawl-delay` honoured; a hostile file cannot stall a crawl for hours.
pub const MAX_CRAWL_DELAY: Duration = Duration::from_secs(30);

/// Most bytes of a `robots.txt` that are read (RFC 9309 asks parsers to accept 500 KiB).
pub const MAX_ROBOTS_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    allow: bool,
    pattern: String,
}

/// The rules that apply to Riptide on one origin.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Robots {
    rules: Vec<Rule>,
    crawl_delay: Option<Duration>,
    deny_all: bool,
}

#[derive(Default)]
struct Group {
    agents: Vec<String>,
    rules: Vec<Rule>,
    delay: Option<Duration>,
}

impl Robots {
    /// No restrictions (a `404` for `robots.txt`).
    #[must_use]
    pub fn allow_all() -> Robots {
        Robots::default()
    }

    /// Everything is off limits (`robots.txt` unreachable or a server error, RFC 9309 §2.3.1.4).
    #[must_use]
    pub fn deny_all() -> Robots {
        Robots {
            deny_all: true,
            ..Robots::default()
        }
    }

    /// Parses `text` for the product token `agent` (case-insensitive). Groups naming a token
    /// that `agent` contains are merged and used; if there are none, the `*` groups are.
    #[must_use]
    pub fn parse(text: &str, agent: &str) -> Robots {
        let agent = agent.to_ascii_lowercase();
        let mut groups: Vec<Group> = Vec::new();
        let mut collecting_agents = false;
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            match key.as_str() {
                "user-agent" => {
                    if !collecting_agents || groups.is_empty() {
                        groups.push(Group::default());
                    }
                    collecting_agents = true;
                    if let Some(g) = groups.last_mut() {
                        g.agents.push(value.to_ascii_lowercase());
                    }
                }
                "allow" | "disallow" => {
                    collecting_agents = false;
                    // an empty Disallow allows everything: no rule
                    if value.is_empty() {
                        continue;
                    }
                    if let Some(g) = groups.last_mut() {
                        g.rules.push(Rule {
                            allow: key == "allow",
                            pattern: value.to_string(),
                        });
                    }
                }
                "crawl-delay" => {
                    collecting_agents = false;
                    if let (Some(g), Ok(secs)) = (groups.last_mut(), value.parse::<f64>()) {
                        if secs.is_finite() && secs >= 0.0 {
                            g.delay = Some(Duration::from_secs_f64(secs).min(MAX_CRAWL_DELAY));
                        }
                    }
                }
                _ => {}
            }
        }

        let specific = |g: &Group| {
            g.agents
                .iter()
                .any(|a| !a.is_empty() && a != "*" && agent.contains(a.as_str()))
        };
        let star = |g: &Group| g.agents.iter().any(|a| a == "*");
        let chosen: Vec<&Group> = if groups.iter().any(specific) {
            groups.iter().filter(|g| specific(g)).collect()
        } else {
            groups.iter().filter(|g| star(g)).collect()
        };
        Robots {
            rules: chosen.iter().flat_map(|g| g.rules.clone()).collect(),
            crawl_delay: chosen.iter().filter_map(|g| g.delay).max(),
            deny_all: false,
        }
    }

    /// `Crawl-delay`, capped at [`MAX_CRAWL_DELAY`].
    #[must_use]
    pub fn crawl_delay(&self) -> Option<Duration> {
        self.crawl_delay
    }

    /// Whether `path_and_query` (`/a/b?x=1`) may be fetched.
    #[must_use]
    pub fn is_allowed(&self, path_and_query: &str) -> bool {
        if self.deny_all {
            return false;
        }
        let mut best: Option<(usize, bool)> = None;
        for rule in &self.rules {
            if pattern_matches(&rule.pattern, path_and_query) {
                let len = rule.pattern.len();
                best = match best {
                    Some((l, a)) if l > len || (l == len && (a || !rule.allow)) => Some((l, a)),
                    _ => Some((len, rule.allow)),
                };
            }
        }
        best.is_none_or(|(_, allow)| allow)
    }
}

/// `*` matches any run of characters, a trailing `$` anchors the end; otherwise the pattern is a
/// prefix of the path.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(p) => (p, true),
        None => (pattern, false),
    };
    let parts: Vec<&str> = pattern.split('*').collect();
    let Some((first, rest)) = parts.split_first() else {
        return true;
    };
    let Some(mut remaining) = path.strip_prefix(first) else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        // no `*` at all
        return !anchored || remaining.is_empty();
    };
    for part in middle {
        match remaining.find(part) {
            Some(i) => remaining = &remaining[i + part.len()..],
            None => return false,
        }
    }
    if anchored {
        remaining.ends_with(last)
    } else {
        remaining.contains(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "\
# comment
User-agent: *
Disallow: /private/
Disallow: /tmp
Allow: /private/open/
Crawl-delay: 2

User-agent: badbot
Disallow: /

User-agent: Riptide
User-agent: other
Disallow: /riptide-only/
Disallow:
";

    #[test]
    fn star_group_longest_match_and_allow_override() {
        let r = Robots::parse(FILE.replace("Riptide", "nobody").as_str(), AGENT);
        assert!(!r.is_allowed("/private/a.zip"));
        assert!(r.is_allowed("/private/open/a.zip"), "longer Allow wins");
        assert!(!r.is_allowed("/tmpfile"), "prefix match");
        assert!(r.is_allowed("/public/a.zip"));
        assert_eq!(r.crawl_delay(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn specific_group_replaces_star() {
        let r = Robots::parse(FILE, AGENT);
        assert!(r.is_allowed("/private/a.zip"), "the * group is ignored");
        assert!(!r.is_allowed("/riptide-only/x"));
        assert_eq!(r.crawl_delay(), None);
        let bad = Robots::parse(FILE, "badbot/1.0");
        assert!(!bad.is_allowed("/anything"));
    }

    #[test]
    fn wildcards_and_anchors() {
        let r = Robots::parse(
            "User-agent: *\nDisallow: /*.pdf$\nDisallow: /a/*/b\nAllow: /a/x/b\n",
            AGENT,
        );
        assert!(!r.is_allowed("/docs/x.pdf"));
        assert!(r.is_allowed("/docs/x.pdf?download=1"), "anchored with $");
        assert!(r.is_allowed("/docs/x.pdfx"));
        assert!(!r.is_allowed("/a/q/b/c"));
        assert!(
            r.is_allowed("/a/x/b"),
            "equal specificity is not enough, longer Allow wins"
        );
    }

    #[test]
    fn equal_length_tie_goes_to_allow() {
        let r = Robots::parse("User-agent: *\nDisallow: /x\nAllow: /x\n", AGENT);
        assert!(r.is_allowed("/x"));
    }

    #[test]
    fn empty_and_garbage_files_allow_everything() {
        for text in ["", "garbage without colons", "Disallow: /x\n", "\u{feff}"] {
            let r = Robots::parse(text, AGENT);
            assert!(
                r.is_allowed("/x"),
                "{text:?}: rules outside a group are ignored"
            );
        }
        assert!(Robots::allow_all().is_allowed("/anything"));
        assert!(!Robots::deny_all().is_allowed("/"));
    }

    #[test]
    fn crawl_delay_is_capped_and_validated() {
        let r = Robots::parse("User-agent: *\nCrawl-delay: 3600\n", AGENT);
        assert_eq!(r.crawl_delay(), Some(MAX_CRAWL_DELAY));
        let r = Robots::parse("User-agent: *\nCrawl-delay: -1\n", AGENT);
        assert_eq!(r.crawl_delay(), None);
        let r = Robots::parse("User-agent: *\nCrawl-delay: 0.5\n", AGENT);
        assert_eq!(r.crawl_delay(), Some(Duration::from_millis(500)));
    }

    #[test]
    fn bom_and_case_are_tolerated() {
        let r = Robots::parse("\u{feff}USER-AGENT: *\nDISALLOW: /p/\n", AGENT);
        assert!(!r.is_allowed("/p/x"));
    }
}
