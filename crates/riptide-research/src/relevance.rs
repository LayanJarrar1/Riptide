//! Candidate validation: how well does a result match what was asked?
//!
//! For every [`Hit`] the engine computes, deterministically and without the network or an AI,
//! which constraints of the [`QueryIntent`] it satisfies, which it does not state, and which it
//! contradicts, and from that a [`MatchClass`]:
//!
//! | Class | Meaning |
//! |---|---|
//! | `Exact` | every title word is in the title / file name / by-line, in order, and a real part of the title; every stated constraint is verified |
//! | `Strong` | as `Exact`, but a soft constraint (year, file type) is not stated by the result |
//! | `Related` | most words match, but a constrained episode / season / version is not stated, the words are a small or reshuffled part of a longer title, or the match is partial |
//! | `Unrelated` | a stated value contradicts the query (episode 21 for "episode 22"), a quoted phrase or an excluded word decides it, or too few words match |
//!
//! `Unrelated` is never returned. `Related` is returned only when the mode allows it or when
//! nothing better exists, and always flagged. The optional AI may only reorder results *within*
//! a class and write a reason: [`confine_score`] squeezes its score into the class's band, so a
//! model can never promote a result.
//!
//! Facts come only from what the source returned (title, file name, URL path, year, version
//! field, summary). Nothing is estimated.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::intent::{
    extension_of, is_file_type, is_filler, normalize, same_type_family, tokens, Facts, QueryIntent,
};
use crate::modes::{Mode, ModeConfig, RelatedPolicy};
use crate::types::{Hit, HitKind, Source};

/// How well a result matches. Ordered best first (`Exact < Strong < Related < Unrelated`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchClass {
    /// Verified match.
    Exact,
    /// Very likely the thing, not fully verified.
    Strong,
    /// Same subject, but not verified to be the thing asked for.
    Related,
    /// Not what was asked for (never returned).
    Unrelated,
}

/// A constraint a query can state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Constraint {
    /// Episode number.
    Episode,
    /// Season number.
    Season,
    /// Year.
    Year,
    /// Version.
    Version,
    /// File type.
    FileType,
}

/// One fact behind a verdict. The page translates these; the English text is only for logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// Every title word is in the title or file name.
    TitleAllWords,
    /// Some title words are in the title or file name.
    TitlePartial {
        /// Words found.
        found: usize,
        /// Words asked for.
        of: usize,
    },
    /// The words appear only in the description, author or publisher.
    WordsOnlyInDescription,
    /// The words are a small part of a much longer title.
    LooseTitle,
    /// The words are all there but not in the order asked for.
    WordsOutOfOrder,
    /// A quoted phrase was found.
    PhraseMatched {
        /// The phrase.
        phrase: String,
    },
    /// A quoted phrase appears only outside the title.
    PhraseOnlyInDescription {
        /// The phrase.
        phrase: String,
    },
    /// A quoted phrase is not there.
    PhraseMissing {
        /// The phrase.
        phrase: String,
    },
    /// An excluded word is in the title.
    ExcludedWord {
        /// The word.
        word: String,
    },
    /// The result states the asked value.
    Matched {
        /// Which constraint.
        constraint: Constraint,
        /// The value.
        value: String,
    },
    /// The result does not say.
    Missing {
        /// Which constraint.
        constraint: Constraint,
    },
    /// The result states a different value.
    Conflict {
        /// Which constraint.
        constraint: Constraint,
        /// What was asked.
        wanted: String,
        /// What the result says.
        found: String,
    },
}

/// Where a result comes from, for the source finder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trust {
    /// The publisher's own page or repository (the owner or host carries the asked name).
    Official,
    /// A primary catalogue (arXiv, Europe PMC, Project Gutenberg, NASA...).
    Primary,
    /// Nothing known.
    Unknown,
    /// Looks like a mirror, re-upload or aggregator.
    Mirror,
}

/// The verdict on one result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    /// The class.
    pub class: MatchClass,
    /// Share of the title words found in the title / file name (0..=1).
    pub coverage: f32,
    /// 0-100, orders results *within* a class. Meaningless across classes.
    pub score: u8,
    /// What was checked.
    pub reasons: Vec<Reason>,
    /// Origin of the result.
    pub trust: Trust,
    /// The result looks like a mirror or aggregator copy.
    pub mirror: bool,
}

/// Is this source a primary catalogue (not a copy of somebody else's material)?
pub fn is_primary_source(source: Source) -> bool {
    matches!(
        source,
        Source::Arxiv
            | Source::EuropePmc
            | Source::Crossref
            | Source::OpenAlex
            | Source::Gutenberg
            | Source::Nasa
            | Source::Blender
            | Source::Github
    )
}

const MIRROR_HOSTS: &[&str] = &[
    "softpedia.com",
    "uptodown.com",
    "filehippo.com",
    "softonic.com",
    "majorgeeks.com",
    "mediafire.com",
    "mega.nz",
    "4shared.com",
    "zippyshare.com",
    "libgen.is",
    "z-lib.org",
    "sci-hub.se",
    "annas-archive.org",
];

const MIRROR_WORDS: &[&str] = &["mirror", "unofficial", "reupload", "re-upload", "repost"];

// ---------------------------------------------------------------- token matching

/// A comparable form of a word: Arabic definite article and a trailing plural `s` removed.
fn stem(word: &str) -> String {
    let w = word
        .strip_prefix("ال")
        .filter(|r| r.chars().count() >= 3)
        .unwrap_or(word);
    if w.is_ascii() && w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") {
        w[..w.len() - 1].to_string()
    } else {
        w.to_string()
    }
}

fn token_eq(a: &str, b: &str) -> bool {
    a == b || stem(a) == stem(b)
}

/// First position of `term` among `words`.
fn position(words: &[String], term: &str) -> Option<usize> {
    words.iter().position(|w| token_eq(w, term))
}

fn contains_phrase(haystack_words: &[String], phrase: &str) -> bool {
    let p = tokens(phrase);
    if p.is_empty() {
        return true;
    }
    haystack_words
        .windows(p.len())
        .any(|w| w.iter().zip(&p).all(|(a, b)| token_eq(a, b)))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The last two path segments of a URL, decoded (a file name often carries the episode).
fn url_tail(u: &str) -> String {
    let Ok(url) = url::Url::parse(u) else {
        return String::new();
    };
    let segs: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let take = segs.len().saturating_sub(2);
    segs[take..]
        .iter()
        .map(|s| percent_decode(s))
        .collect::<Vec<_>>()
        .join(" ")
}

fn host_of(u: &str) -> Option<String> {
    url::Url::parse(u).ok().and_then(|u| {
        u.host_str()
            .map(|h| h.trim_start_matches("www.").to_ascii_lowercase())
    })
}

// ---------------------------------------------------------------- constraint checks

#[derive(Debug, Clone, PartialEq, Eq)]
enum Check {
    Matched(String),
    Missing,
    Conflict { wanted: String, found: String },
}

fn check_number(want: u32, stated: &[u32], supportive: &[u32]) -> Check {
    if stated.contains(&want) {
        Check::Matched(want.to_string())
    } else if !stated.is_empty() {
        Check::Conflict {
            wanted: want.to_string(),
            found: stated
                .iter()
                .take(3)
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        }
    } else if supportive.contains(&want) {
        // only the description says so: good enough to avoid a conflict, counts as stated
        Check::Matched(want.to_string())
    } else {
        Check::Missing
    }
}

fn strip_v(v: &str) -> &str {
    v.trim().trim_start_matches(['v', 'V']).trim_matches('.')
}

/// `have` satisfies `want` when equal, or when it is the same release with more parts
/// (`3.0.20.1` for `3.0.20`); `3.0` does not satisfy `3.0.20`.
fn version_ok(want: &str, have: &str) -> bool {
    let (w, h) = (strip_v(want), strip_v(have));
    h == w || h.strip_prefix(w).is_some_and(|rest| rest.starts_with('.'))
}

/// Versions a source gave in a free field such as a release tag (`v3.0.20`, `release-3.0.20`, `20`).
fn versions_of_field(field: &str) -> Vec<String> {
    let mut out = Facts::of(field).versions;
    let plain = strip_v(field);
    if !plain.is_empty()
        && plain
            .split('.')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        out.push(plain.to_string());
    }
    out
}

fn is_video_type(t: &str) -> bool {
    matches!(t, "mp4" | "mkv" | "webm" | "mov" | "avi")
}

// ---------------------------------------------------------------- assessment

/// Platform and packaging words in release titles and file names.
const RELEASE_NOISE: &[&str] = &[
    "win",
    "win32",
    "win64",
    "x64",
    "x86",
    "amd64",
    "arm64",
    "windows",
    "linux",
    "macos",
    "setup",
    "installer",
    "portable",
    "release",
    "bit",
    "64bit",
    "32bit",
];

/// How many distinct words of a hit title say something about the thing itself: no filler, no
/// number (version parts, episode numbers, years), no file-type, quality or platform words.
fn content_words(title_words: &[String], intent: &QueryIntent) -> usize {
    let asked_numbers: Vec<String> = [intent.episode, intent.season]
        .into_iter()
        .flatten()
        .map(|n| n.to_string())
        .chain(intent.year.map(|y| y.to_string()))
        .collect();
    let distinct: HashSet<String> = title_words
        .iter()
        .filter(|w| !is_filler(w) && !RELEASE_NOISE.contains(&w.as_str()))
        .filter(|w| !asked_numbers.contains(w))
        // numbers are version parts, years, episode numbers: not what the thing is called
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()) || intent.title_terms.contains(w))
        .filter(|w| !is_file_type(w) || intent.title_terms.contains(w))
        // a repeated word ("owner/owner-tool tool") is one word
        .map(|w| stem(w))
        .collect();
    distinct.len()
}

/// Judges one result against the query.
pub fn assess(intent: &QueryIntent, hit: &Hit) -> Assessment {
    let title_n = normalize(&hit.title);
    let name_n = hit.filename.as_deref().map(normalize).unwrap_or_default();
    let tail = [Some(&hit.download_url), hit.page_url.as_ref()]
        .into_iter()
        .flatten()
        .map(|u| url_tail(u))
        .collect::<Vec<_>>()
        .join(" ");
    let tail_n = normalize(&tail);
    let desc_n = normalize(&format!(
        "{} {} {}",
        hit.summary.as_deref().unwrap_or(""),
        hit.authors.join(" "),
        hit.publisher.as_deref().unwrap_or("")
    ));

    let title_words = tokens(&title_n);
    let byline = tokens(&normalize(&format!(
        "{} {}",
        hit.authors.join(" "),
        hit.publisher.as_deref().unwrap_or("")
    )));
    // title, file name, URL path and the by-line (authors, publisher) are what a result *is*;
    // the summary only supports
    let mut primary_words = title_words.clone();
    primary_words.extend(tokens(&name_n));
    primary_words.extend(tokens(&tail_n));
    primary_words.extend(byline.iter().cloned());
    let desc_words = tokens(&desc_n);
    let mut all_words = primary_words.clone();
    all_words.extend(desc_words.iter().cloned());

    let mut reasons: Vec<Reason> = Vec::new();
    let mut unrelated = false;

    // ---- exclusions (title, file name, authors, publisher: not the long summary)
    for word in &intent.exclusions {
        if position(&primary_words, word).is_some() {
            reasons.push(Reason::ExcludedWord { word: word.clone() });
            unrelated = true;
        }
    }

    // ---- phrases
    let mut phrase_only_desc = false;
    for phrase in &intent.phrases {
        if contains_phrase(&primary_words, phrase) {
            reasons.push(Reason::PhraseMatched {
                phrase: phrase.clone(),
            });
        } else if contains_phrase(&all_words, phrase) {
            phrase_only_desc = true;
            reasons.push(Reason::PhraseOnlyInDescription {
                phrase: phrase.clone(),
            });
        } else {
            reasons.push(Reason::PhraseMissing {
                phrase: phrase.clone(),
            });
            unrelated = true;
        }
    }

    // ---- title words
    let terms = &intent.title_terms;
    let found_primary: Vec<&String> = terms
        .iter()
        .filter(|t| position(&primary_words, t).is_some())
        .collect();
    let found_all = terms
        .iter()
        .filter(|t| position(&all_words, t).is_some())
        .count();
    let (cov_primary, cov_all) = if terms.is_empty() {
        // nothing but constraints ("episode 22"): too little to call anything better than related
        let c = if intent.phrases.is_empty() { 0.6 } else { 1.0 };
        (c, c)
    } else {
        (
            found_primary.len() as f32 / terms.len() as f32,
            found_all as f32 / terms.len() as f32,
        )
    };
    if !terms.is_empty() {
        if found_primary.len() == terms.len() {
            reasons.push(Reason::TitleAllWords);
        } else if found_all == terms.len() {
            reasons.push(Reason::WordsOnlyInDescription);
        } else {
            reasons.push(Reason::TitlePartial {
                found: found_all,
                of: terms.len(),
            });
        }
    }
    if cov_all < 0.6 {
        unrelated = true;
    }
    // looseness: the asked words must be a real part of the title, in the asked order
    let name_words = tokens(&name_n);
    let share = |words: &[String]| -> f32 {
        let found = terms
            .iter()
            .filter(|t| position(words, t).is_some())
            .count();
        found as f32 / content_words(words, intent).max(1) as f32
    };
    let in_order = |words: &[String]| -> bool {
        let pos: Vec<usize> = terms.iter().filter_map(|t| position(words, t)).collect();
        !pos.is_empty() && pos.windows(2).all(|w| w[0] <= w[1])
    };
    // "owner/name" (a repository): the owner is the publisher, not part of what the thing is called
    let core_words = match hit.title.split_once('/') {
        Some((owner, rest)) if !owner.trim().contains(' ') && !rest.trim().is_empty() => {
            tokens(&normalize(rest))
        }
        _ => title_words.clone(),
    };
    let tight_share = share(&core_words).max(share(&name_words)).min(1.0);
    let ordered = in_order(&core_words) || in_order(&name_words);

    // ---- constraints
    let name_facts = Facts::of(&format!("{} {} {}", hit.title, name_n, tail));
    let desc_facts = Facts::of(hit.summary.as_deref().unwrap_or(""));
    let mut identifying_missing = false;
    let mut identifying_matched = false;
    let mut soft_missing = false;
    let mut matched_constraints = 0usize;
    let mut apply = |constraint: Constraint, check: Check, identifying: bool| match check {
        Check::Matched(value) => {
            matched_constraints += 1;
            identifying_matched |= identifying;
            reasons.push(Reason::Matched { constraint, value });
        }
        Check::Missing => {
            if identifying {
                identifying_missing = true;
            } else {
                soft_missing = true;
            }
            reasons.push(Reason::Missing { constraint });
        }
        Check::Conflict { wanted, found } => {
            unrelated = true;
            reasons.push(Reason::Conflict {
                constraint,
                wanted,
                found,
            });
        }
    };
    if let Some(e) = intent.episode {
        apply(
            Constraint::Episode,
            check_number(e, &name_facts.episodes, &desc_facts.episodes),
            true,
        );
    }
    if let Some(s) = intent.season {
        apply(
            Constraint::Season,
            check_number(s, &name_facts.seasons, &desc_facts.seasons),
            true,
        );
    }
    if let Some(want) = &intent.version {
        let mut have: Vec<String> = hit
            .version
            .as_deref()
            .map(versions_of_field)
            .unwrap_or_default();
        have.extend(name_facts.versions.iter().cloned());
        let check = if have.iter().any(|h| version_ok(want, h)) {
            Check::Matched(want.clone())
        } else if let Some(first) = have.first() {
            Check::Conflict {
                wanted: want.clone(),
                found: first.clone(),
            }
        } else {
            Check::Missing
        };
        apply(Constraint::Version, check, true);
    }
    if let Some(y) = intent.year {
        // the source's own year field is authoritative; the title is the fallback
        let check = match (hit.year, name_facts.years.as_slice()) {
            (Some(h), _) if h == y => Check::Matched(y.to_string()),
            (Some(h), _) => Check::Conflict {
                wanted: y.to_string(),
                found: h.to_string(),
            },
            (None, years) if years.contains(&y) => Check::Matched(y.to_string()),
            (None, [first, ..]) => Check::Conflict {
                wanted: y.to_string(),
                found: first.to_string(),
            },
            (None, []) => Check::Missing,
        };
        apply(Constraint::Year, check, false);
    }
    if let Some(want) = &intent.file_type {
        let have = hit.filename.as_deref().and_then(extension_of).or_else(|| {
            (hit.kind == HitKind::File)
                .then(|| extension_of(&hit.download_url))
                .flatten()
        });
        let check = match have {
            Some(h) if same_type_family(want, &h) => Check::Matched(want.clone()),
            Some(h) => Check::Conflict {
                wanted: want.clone(),
                found: h,
            },
            // a video page is downloaded as a video file by the media pipeline
            None if hit.source == Source::Youtube && is_video_type(want) => {
                Check::Matched(want.clone())
            }
            None => Check::Missing,
        };
        apply(Constraint::FileType, check, false);
    }

    // ---- class
    let words_ok = terms.is_empty() || found_primary.len() == terms.len();
    let class = if unrelated {
        MatchClass::Unrelated
    } else {
        // a verified episode / season / version is strong evidence of identity, so a title that
        // also carries an episode name or release tags may be as long as it likes
        let loose = !terms.is_empty() && !identifying_matched && tight_share < 0.5;
        let scrambled = terms.len() >= 2 && !ordered && words_ok;
        if loose {
            reasons.push(Reason::LooseTitle);
        } else if scrambled {
            reasons.push(Reason::WordsOutOfOrder);
        }
        let verified_title =
            words_ok && !loose && !scrambled && (!terms.is_empty() || !intent.phrases.is_empty());
        if identifying_missing || phrase_only_desc || cov_primary < 1.0 - 1e-6 || loose || scrambled
        {
            // the asked words are a small or reshuffled part of the title: same subject at best
            MatchClass::Related
        } else if verified_title && !soft_missing {
            MatchClass::Exact
        } else {
            MatchClass::Strong
        }
    };

    // ---- trust
    let (mirror, trust) = trust_of(hit, intent, &all_words);

    let score = {
        let base =
            cov_primary * 60.0 + tight_share * 25.0 + (matched_constraints as f32 * 5.0).min(15.0);
        base.round().clamp(0.0, 100.0) as u8
    };
    Assessment {
        class,
        coverage: cov_primary,
        score,
        reasons,
        trust,
        mirror,
    }
}

fn trust_of(hit: &Hit, intent: &QueryIntent, all_words: &[String]) -> (bool, Trust) {
    let hosts: Vec<String> = [Some(&hit.download_url), hit.page_url.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|u| host_of(u))
        .collect();
    let mirror_host = hosts.iter().any(|h| {
        MIRROR_HOSTS
            .iter()
            .any(|m| h == m || h.ends_with(&format!(".{m}")))
    });
    let mirror_word = MIRROR_WORDS
        .iter()
        .any(|w| all_words.iter().any(|a| a == w));
    let mirror = mirror_host || mirror_word;
    let named_by = |text: &str| -> bool {
        let n = normalize(text);
        intent
            .title_terms
            .iter()
            .any(|t| t.chars().count() >= 3 && n.contains(t.as_str()))
    };
    let owner_or_host = hit.publisher.as_deref().is_some_and(named_by)
        || (hit.source == Source::Github && named_by(&hit.title))
        || hosts.iter().any(|h| {
            // the registrable label ("videolan" in "download.videolan.org")
            let labels: Vec<&str> = h.split('.').collect();
            labels
                .len()
                .checked_sub(2)
                .and_then(|i| labels.get(i))
                .is_some_and(|l| named_by(l))
        });
    let trust = if mirror {
        Trust::Mirror
    } else if matches!(hit.source, Source::Github | Source::Web | Source::Youtube) && owner_or_host
    {
        Trust::Official
    } else if is_primary_source(hit.source) {
        Trust::Primary
    } else {
        Trust::Unknown
    };
    (mirror, trust)
}

/// The band of the 0-100 `score` a class's results may carry on the wire.
pub fn score_band(class: MatchClass) -> (u8, u8) {
    match class {
        MatchClass::Exact => (75, 100),
        MatchClass::Strong => (50, 74),
        MatchClass::Related => (1, 49),
        MatchClass::Unrelated => (0, 0),
    }
}

/// Squeezes an AI score (0-100) into the band of the result's class. The model can order
/// results inside a class and nothing more: a "100" for a related result stays below a "1" for a
/// strong one.
pub fn confine_score(class: MatchClass, ai_score: u8) -> u8 {
    let (lo, hi) = score_band(class);
    let span = u32::from(hi - lo);
    lo + (u32::from(ai_score.min(100)) * span / 100) as u8
}

// ---------------------------------------------------------------- the gate

/// A result with its verdict.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessed {
    /// The result.
    pub hit: Hit,
    /// The verdict.
    pub assessment: Assessment,
}

/// Decides, batch by batch, which results are shown: drops `Unrelated`, shows `Exact` and
/// `Strong` at once, and shows `Related` according to the mode's policy (held back until the
/// search ends and only released when nothing better exists, or at once for deep research).
#[derive(Debug)]
pub struct Gate {
    intent: QueryIntent,
    mode: &'static ModeConfig,
    held: Vec<Assessed>,
    exact: usize,
    strong: usize,
    related_shown: usize,
    dropped: usize,
}

impl Gate {
    /// A gate for `intent` under `mode`.
    pub fn new(intent: QueryIntent, mode: Mode) -> Self {
        Gate {
            intent,
            mode: mode.config(),
            held: Vec::new(),
            exact: 0,
            strong: 0,
            related_shown: 0,
            dropped: 0,
        }
    }

    /// The understood query.
    pub fn intent(&self) -> &QueryIntent {
        &self.intent
    }

    /// Judges `hits`; returns the ones to show now, in the given order.
    pub fn offer(&mut self, hits: Vec<Hit>) -> Vec<Assessed> {
        let mut now = Vec::new();
        for hit in hits {
            let assessment = assess(&self.intent, &hit);
            let item = Assessed { hit, assessment };
            match item.assessment.class {
                MatchClass::Unrelated => self.dropped += 1,
                MatchClass::Exact => {
                    self.exact += 1;
                    now.push(item);
                }
                MatchClass::Strong => {
                    self.strong += 1;
                    now.push(item);
                }
                MatchClass::Related => {
                    if self.mode.verification.related == RelatedPolicy::Always {
                        self.related_shown += 1;
                        now.push(item);
                    } else {
                        self.held.push(item);
                    }
                }
            }
        }
        now
    }

    /// Called when the search has ended: releases the held related results when no exact or
    /// strong result was found at all.
    pub fn finish(&mut self) -> Vec<Assessed> {
        let held = std::mem::take(&mut self.held);
        if self.exact + self.strong == 0 {
            self.related_shown += held.len();
            held
        } else {
            self.dropped += held.len();
            Vec::new()
        }
    }

    /// Exact matches shown so far.
    pub fn exact_count(&self) -> usize {
        self.exact
    }

    /// Strong matches shown so far.
    pub fn strong_count(&self) -> usize {
        self.strong
    }

    /// Results judged unrelated (and related ones that were never shown).
    pub fn dropped_count(&self) -> usize {
        self.dropped
    }
}

/// Orders results: class first, then (when the mode asks) trust, then the score, then the
/// original order (stable).
pub fn order(items: &mut [Assessed], prefer_official: bool) {
    items.sort_by(|a, b| {
        let (x, y) = (&a.assessment, &b.assessment);
        x.class
            .cmp(&y.class)
            .then_with(|| {
                if prefer_official {
                    x.trust.cmp(&y.trust)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| y.score.cmp(&x.score))
    });
}

// ---------------------------------------------------------------- cross-source comparison

/// Two sources disagree about the same item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// `year`, `version` or `size`.
    pub field: &'static str,
    /// The id of the result that was kept.
    pub id: String,
    /// What the kept result says and what the other source says, with the sources.
    pub values: Vec<(Source, String)>,
}

/// Which other sources also have a kept result, plus disagreements between them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// kept id -> the other sources that returned the same item.
    pub also_in: HashMap<String, Vec<Source>>,
    /// Disagreements.
    pub notes: Vec<Note>,
}

/// What a dropped duplicate said about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    /// The id of the result that was kept instead.
    pub kept: String,
    /// The duplicate's source.
    pub source: Source,
    /// Its year.
    pub year: Option<u16>,
    /// Its version.
    pub version: Option<String>,
    /// Its size.
    pub size: Option<u64>,
}

/// Compares the kept results with their dropped duplicates (deep research / source finder).
pub fn compare(kept: &[Hit], duplicates: &[Duplicate]) -> Comparison {
    let by_id: HashMap<&str, &Hit> = kept.iter().map(|h| (h.id.as_str(), h)).collect();
    let mut out = Comparison::default();
    let mut seen_notes: HashSet<(String, &'static str, Source)> = HashSet::new();
    for d in duplicates {
        let Some(k) = by_id.get(d.kept.as_str()) else {
            continue;
        };
        if d.source != k.source {
            let list = out.also_in.entry(k.id.clone()).or_default();
            if !list.contains(&d.source) {
                list.push(d.source);
            }
        }
        let mut note = |field: &'static str, mine: String, theirs: String| {
            if seen_notes.insert((k.id.clone(), field, d.source)) {
                out.notes.push(Note {
                    field,
                    id: k.id.clone(),
                    values: vec![(k.source, mine), (d.source, theirs)],
                });
            }
        };
        if let (Some(a), Some(b)) = (k.year, d.year) {
            if a != b {
                note("year", a.to_string(), b.to_string());
            }
        }
        if let (Some(a), Some(b)) = (&k.version, &d.version) {
            if strip_v(a) != strip_v(b) {
                note("version", a.clone(), b.clone());
            }
        }
        if let (Some(a), Some(b)) = (k.size, d.size) {
            if a != b {
                note("size", a.to_string(), b.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(src: Source, id: &str, title: &str) -> Hit {
        Hit::new(src, id, title, &format!("https://x.example/{id}"))
    }

    fn class(query: &str, h: &Hit) -> MatchClass {
        assess(&QueryIntent::parse(query), h).class
    }

    #[test]
    fn the_wrong_episode_is_unrelated_and_the_right_one_exact() {
        let q = "naruto shippuden episode 22";
        let right = hit(
            Source::Youtube,
            "a",
            "Naruto Shippuden Episode 22 - The Jinchuriki",
        );
        let wrong = hit(Source::Youtube, "b", "Naruto Shippuden Episode 21");
        let wrong2 = hit(
            Source::Youtube,
            "c",
            "Naruto Shippuden - Episode 23 (English Dub)",
        );
        assert_eq!(class(q, &right), MatchClass::Exact);
        assert_eq!(class(q, &wrong), MatchClass::Unrelated);
        assert_eq!(class(q, &wrong2), MatchClass::Unrelated);
        let a = assess(&QueryIntent::parse(q), &wrong);
        assert!(a.reasons.iter().any(|r| matches!(
            r,
            Reason::Conflict { constraint: Constraint::Episode, wanted, found }
                if wanted == "22" && found == "21"
        )));
    }

    #[test]
    fn a_missing_episode_cannot_be_exact() {
        let q = "naruto shippuden episode 22";
        let series = hit(
            Source::InternetArchive,
            "s",
            "Naruto Shippuden complete series",
        );
        let c = class(q, &series);
        assert!(c > MatchClass::Strong, "{c:?}");
        assert_ne!(c, MatchClass::Unrelated);
        // a range that contains the episode counts as stating it
        let pack = hit(
            Source::InternetArchive,
            "p",
            "Naruto Shippuden Episodes 20-25",
        );
        assert_eq!(class(q, &pack), MatchClass::Exact);
        let out_of_range = hit(
            Source::InternetArchive,
            "p2",
            "Naruto Shippuden Episodes 1-10",
        );
        assert_eq!(class(q, &out_of_range), MatchClass::Unrelated);
    }

    #[test]
    fn arabic_titles_and_digits_match_across_spellings() {
        let q = "مسلسل قيامة عثمان الحلقة ٢٢";
        let right = hit(Source::Youtube, "a", "قيامة عثمان - الحلقة 22 - مسلسل");
        let wrong = hit(Source::Youtube, "b", "قيامة عثمان الحلقة ٢١");
        assert_eq!(class(q, &right), MatchClass::Exact);
        assert_eq!(class(q, &wrong), MatchClass::Unrelated);
    }

    #[test]
    fn version_and_year_and_type_are_hard_filters() {
        let q = "vlc v3.0.20";
        let mut ok = hit(
            Source::Github,
            "a",
            "videolan/vlc 3.0.20 - vlc-3.0.20-win64.exe",
        );
        ok.version = Some("3.0.20".into());
        let mut old = hit(
            Source::Github,
            "b",
            "videolan/vlc 3.0.18 - vlc-3.0.18-win64.exe",
        );
        old.version = Some("3.0.18".into());
        assert_eq!(class(q, &ok), MatchClass::Exact);
        assert_eq!(class(q, &old), MatchClass::Unrelated);

        let q = "attention is all you need pdf";
        let mut pdf = hit(Source::Arxiv, "p", "Attention Is All You Need");
        pdf.filename = Some("1706.03762.pdf".into());
        let mut epub = hit(Source::Gutenberg, "e", "Attention Is All You Need");
        epub.filename = Some("attention.epub".into());
        assert_eq!(class(q, &pdf), MatchClass::Exact);
        assert_eq!(class(q, &epub), MatchClass::Unrelated);

        let q = "dune 2021";
        let mut movie = hit(Source::InternetArchive, "m", "Dune");
        movie.year = Some(2021);
        let mut old = hit(Source::InternetArchive, "o", "Dune");
        old.year = Some(1984);
        assert_ne!(class(q, &movie), MatchClass::Unrelated);
        assert_eq!(class(q, &old), MatchClass::Unrelated);
    }

    #[test]
    fn a_missing_year_or_type_caps_at_strong_not_related() {
        let q = "climate report 2019";
        let h = hit(Source::Crossref, "a", "Climate Report");
        assert_eq!(class(q, &h), MatchClass::Strong);
    }

    #[test]
    fn exclusions_and_phrases_decide() {
        let q = r#""attention is all you need" -survey"#;
        let ok = hit(Source::Arxiv, "a", "Attention Is All You Need");
        let survey = hit(Source::Arxiv, "b", "Attention Is All You Need: a survey");
        let other = hit(Source::Arxiv, "c", "Attention and memory in transformers");
        assert_eq!(class(q, &ok), MatchClass::Exact);
        assert_eq!(class(q, &survey), MatchClass::Unrelated);
        assert_eq!(class(q, &other), MatchClass::Unrelated);
    }

    #[test]
    fn a_loose_or_reordered_title_is_not_exact() {
        let q = "attack on titan";
        let exact = hit(Source::InternetArchive, "a", "Attack on Titan");
        let reordered = hit(Source::InternetArchive, "b", "Titan Attack");
        let loose = hit(
            Source::InternetArchive,
            "c",
            "A long essay about the attack of the mighty colossal Titan figure in modern popular anime culture",
        );
        assert_eq!(class(q, &exact), MatchClass::Exact);
        // all the words, wrong order or lost in a long title: the same subject at best
        assert_eq!(class(q, &reordered), MatchClass::Related);
        assert_eq!(class(q, &loose), MatchClass::Related);
    }

    #[test]
    fn unrelated_words_are_unrelated() {
        assert_eq!(
            class(
                "quantum computing",
                &hit(Source::Arxiv, "a", "Frog populations of Peru")
            ),
            MatchClass::Unrelated
        );
    }

    #[test]
    fn ai_scores_cannot_cross_classes() {
        for ai in [0u8, 1, 50, 99, 100, 200] {
            let exact = confine_score(MatchClass::Exact, ai);
            let strong = confine_score(MatchClass::Strong, ai);
            let related = confine_score(MatchClass::Related, ai);
            assert!(related < strong, "{ai}");
            assert!(strong < exact, "{ai}");
            assert!((75..=100).contains(&exact));
            assert!((50..=74).contains(&strong));
            assert!((1..=49).contains(&related));
        }
        // order inside a class is kept
        assert!(confine_score(MatchClass::Exact, 90) > confine_score(MatchClass::Exact, 10));
        assert_eq!(confine_score(MatchClass::Unrelated, 100), 0);
    }

    #[test]
    fn the_gate_drops_unrelated_and_holds_related_back() {
        let q = "naruto shippuden episode 22";
        let mut gate = Gate::new(QueryIntent::parse(q), Mode::QuickSearch);
        let shown = gate.offer(vec![
            hit(Source::Youtube, "w", "Naruto Shippuden Episode 21"),
            hit(
                Source::InternetArchive,
                "s",
                "Naruto Shippuden complete series",
            ),
        ]);
        assert!(shown.is_empty());
        // nothing better turned up: the related one is released, flagged as related
        let rest = gate.finish();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].assessment.class, MatchClass::Related);
        assert_eq!(gate.dropped_count(), 1);

        let mut gate = Gate::new(QueryIntent::parse(q), Mode::QuickSearch);
        let shown = gate.offer(vec![
            hit(
                Source::InternetArchive,
                "s",
                "Naruto Shippuden complete series",
            ),
            hit(Source::Youtube, "r", "Naruto Shippuden Episode 22"),
        ]);
        assert_eq!(shown.len(), 1);
        assert_eq!(gate.exact_count(), 1);
        // an exact result exists: the related one is never shown
        assert!(gate.finish().is_empty());
    }

    #[test]
    fn deep_research_shows_related_flagged() {
        let q = "naruto shippuden episode 22";
        let mut gate = Gate::new(QueryIntent::parse(q), Mode::DeepResearch);
        let shown = gate.offer(vec![
            hit(
                Source::InternetArchive,
                "s",
                "Naruto Shippuden complete series",
            ),
            hit(Source::Youtube, "r", "Naruto Shippuden Episode 22"),
        ]);
        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].assessment.class, MatchClass::Related);
    }

    #[test]
    fn ordering_puts_class_first_then_score() {
        let q = "rust book";
        let i = QueryIntent::parse(q);
        let mut items: Vec<Assessed> = [
            hit(
                Source::InternetArchive,
                "a",
                "The big rust book of everything else in this world today",
            ),
            hit(Source::InternetArchive, "b", "Rust Book"),
        ]
        .into_iter()
        .map(|h| Assessed {
            assessment: assess(&i, &h),
            hit: h,
        })
        .collect();
        order(&mut items, false);
        assert_eq!(items[0].hit.id, "internet-archive:b");
    }

    #[test]
    fn source_finder_trust_flags_mirrors_and_prefers_the_owner() {
        let i = QueryIntent::parse("vlc");
        let mut official = hit(Source::Github, "a", "videolan/vlc");
        official.publisher = Some("videolan".into());
        official.page_url = Some("https://github.com/videolan/vlc".into());
        let mut mirror = hit(Source::Web, "b", "VLC media player download");
        mirror.download_url = "https://www.softpedia.com/get/vlc".into();
        let a = assess(&i, &official);
        let b = assess(&i, &mirror);
        assert_eq!(a.trust, Trust::Official);
        assert_eq!(b.trust, Trust::Mirror);
        assert!(b.mirror && !a.mirror);
    }

    #[test]
    fn comparison_lists_other_sources_and_disagreements() {
        let mut a = hit(Source::OpenAlex, "a", "Same paper");
        a.year = Some(2019);
        let dup = Duplicate {
            kept: a.id.clone(),
            source: Source::Crossref,
            year: Some(2020),
            version: None,
            size: None,
        };
        let same_source = Duplicate {
            kept: a.id.clone(),
            source: Source::OpenAlex,
            year: Some(2019),
            version: None,
            size: None,
        };
        let c = compare(&[a.clone()], &[dup, same_source]);
        assert_eq!(c.also_in[&a.id], [Source::Crossref]);
        assert_eq!(c.notes.len(), 1);
        assert_eq!(c.notes[0].field, "year");
        assert_eq!(
            c.notes[0].values,
            [
                (Source::OpenAlex, "2019".to_string()),
                (Source::Crossref, "2020".to_string())
            ]
        );
    }

    #[test]
    fn percent_decoding_is_safe_on_odd_input() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%D8%A7"), "ا");
    }
}
