//! The phrasings a search runs, without any AI: the user's own words (minus operators the
//! sources do not understand), the same words in the form catalogues index episodes in, and the
//! bare title as a fallback. Words keep their original spelling (Arabic is not folded here);
//! folding is for comparing results only.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::intent::{normalize, QueryIntent};
use crate::modes::{Mode, ModeConfig};

/// Longest phrasing sent to a source (the adapters refuse more than 300 characters).
const MAX_QUERY_CHARS: usize = 280;

fn quoted() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"[“"«„]([^”"»“]+)[”"»]"#).expect("static regex"))
}

fn is_operator(word: &str) -> bool {
    let lower = word.to_lowercase();
    let exclusion = lower
        .strip_prefix('-')
        .is_some_and(|w| w.chars().count() >= 2 && w.chars().any(char::is_alphabetic));
    let keyword = ["filetype:", "ext:", "format:", "type:"]
        .iter()
        .any(|k| lower.starts_with(k));
    exclusion || keyword
}

/// The user's words without exclusions and `filetype:` keywords (quotes are kept).
fn typed(raw: &str) -> String {
    // a quoted phrase may contain spaces and hyphens: protect it while splitting
    let mut out = Vec::new();
    let mut rest = raw.trim();
    while !rest.is_empty() {
        if let Some(m) = quoted().find(rest).filter(|m| m.start() == 0) {
            out.push(m.as_str().to_string());
            rest = rest[m.end()..].trim_start();
            continue;
        }
        let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
        let (word, tail) = rest.split_at(end);
        if !is_operator(word) {
            out.push(word.to_string());
        }
        rest = tail.trim_start();
    }
    out.join(" ")
}

/// The original spelling of the title words, in the order typed, plus quoted phrases.
fn core(raw: &str, intent: &QueryIntent) -> String {
    let mut wanted: Vec<&String> = intent.title_terms.iter().collect();
    let mut out: Vec<String> = quoted()
        .captures_iter(raw)
        .filter_map(|c| c.get(1).map(|m| format!("\"{}\"", m.as_str().trim())))
        .collect();
    let without_quotes = quoted().replace_all(raw, " ");
    for tok in without_quotes.split_whitespace() {
        let clean = tok.trim_matches(|c: char| !c.is_alphanumeric());
        if clean.is_empty() {
            continue;
        }
        let n = normalize(clean);
        if let Some(i) = wanted.iter().position(|w| **w == n) {
            wanted.remove(i);
            out.push(clean.to_string());
        }
    }
    out.join(" ")
}

/// The phrasings to run, best first: at most `1 + mode.query_variants`, and never more than the
/// mode's query budget.
pub fn variants(intent: &QueryIntent, mode: Mode) -> Vec<String> {
    let cfg: &ModeConfig = mode.config();
    let typed = typed(&intent.raw);
    let mut core = core(&intent.raw, intent);
    if core.is_empty() {
        core = typed.clone();
    }
    let mut list = vec![typed];
    match (intent.season, intent.episode) {
        _ if intent.title_terms.is_empty() => {}
        (Some(s), Some(e)) => list.push(format!("{core} S{s:02}E{e:02}")),
        (None, Some(e)) => list.push(format!("{core} episode {e}")),
        (Some(s), None) => list.push(format!("{core} season {s}")),
        (None, None) => {}
    }
    list.push(core.clone());
    if cfg.id == Mode::SourceFinder {
        list.push(format!("{core} official"));
    }
    let mut seen = HashSet::new();
    list.into_iter()
        .map(|q| {
            let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
            q.chars().take(MAX_QUERY_CHARS).collect::<String>()
        })
        .filter(|q| !q.is_empty() && seen.insert(normalize(q)))
        .take((1 + cfg.query_variants).min(cfg.budget.max_queries))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(q: &str, m: Mode) -> Vec<String> {
        variants(&QueryIntent::parse(q), m)
    }

    #[test]
    fn episode_queries_get_the_catalogue_forms_and_keep_their_spelling() {
        let q = v("Naruto Shippuden الحلقة 22", Mode::DeepResearch);
        assert_eq!(q[0], "Naruto Shippuden الحلقة 22");
        assert!(
            q.contains(&"Naruto Shippuden episode 22".to_string()),
            "{q:?}"
        );
        assert!(q.contains(&"Naruto Shippuden".to_string()));
        let q = v("Breaking Bad season 2 episode 5", Mode::DeepResearch);
        assert!(q.contains(&"Breaking Bad S02E05".to_string()), "{q:?}");
    }

    #[test]
    fn operators_are_not_sent_to_the_sources() {
        let q = v(
            r#""attention is all you need" transformer -survey filetype:pdf"#,
            Mode::QuickSearch,
        );
        assert_eq!(q[0], r#""attention is all you need" transformer"#);
        assert!(!q
            .iter()
            .any(|x| x.contains("survey") || x.contains("filetype")));
    }

    #[test]
    fn the_mode_caps_the_number_of_phrasings() {
        assert!(v("naruto episode 22", Mode::QuickSearch).len() <= 2);
        assert!(v("naruto episode 22", Mode::DeepResearch).len() <= 4);
        let finder = v("vlc", Mode::SourceFinder);
        assert!(finder.contains(&"vlc official".to_string()), "{finder:?}");
        let plain = v("vlc", Mode::QuickSearch);
        assert_eq!(plain, ["vlc"]);
    }

    #[test]
    fn nothing_is_dropped_when_only_constraints_were_typed() {
        let q = v("episode 22", Mode::QuickSearch);
        assert_eq!(q[0], "episode 22");
        assert!(v("", Mode::QuickSearch).is_empty());
    }

    #[test]
    fn very_long_input_is_cut() {
        let long = "word ".repeat(200);
        for q in v(&long, Mode::QuickSearch) {
            assert!(q.chars().count() <= MAX_QUERY_CHARS);
        }
    }
}
