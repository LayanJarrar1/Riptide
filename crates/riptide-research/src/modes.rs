//! Search modes as small configuration values: what each one retrieves, how strictly it
//! verifies, what the optional AI is told, and the budget it may spend. A mode never changes the
//! relevance rules themselves (see [`crate::relevance`]); it chooses how many sources and
//! queries are spent, whether weaker matches are shown, and what extra the page gets (source
//! comparison, mirror flags).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::types::{Category, Source};

/// The four modes. The serde ids are the wire ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Strict filters, few sources, no planning call: the fast default.
    #[default]
    QuickSearch,
    /// More query phrasings and sources, cross-source comparison and contradiction notes.
    DeepResearch,
    /// Videos only.
    VideoSearch,
    /// Find the official page or publisher of something and flag mirrors.
    SourceFinder,
}

/// How a mode gathers candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Retrieval {
    /// The user's own words and one cleaned variant; the fewest sources that serve the category.
    Strict,
    /// Every phrasing in the budget on every source that serves the category.
    Broad,
    /// The video catalogues only.
    Video,
    /// Publisher and primary catalogues first.
    OfficialFirst,
}

/// When weaker matches ([`crate::relevance::MatchClass::Related`]) are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RelatedPolicy {
    /// Only when no exact or strong result exists (always flagged as related).
    OnlyIfNothingBetter,
    /// Always, after the better ones (flagged).
    Always,
}

/// How candidates are verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    /// Release policy of related results.
    pub related: RelatedPolicy,
    /// Compare duplicates across sources (also listing which other sources have the item).
    pub compare_sources: bool,
    /// Report disagreements between sources (different year, version or size of one item).
    pub note_contradictions: bool,
    /// Flag results that look like mirrors or re-uploads.
    pub flag_mirrors: bool,
    /// Order official / publisher results first within their class.
    pub prefer_official: bool,
}

/// What a search may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    /// Most sources searched.
    pub max_sources: usize,
    /// Most query phrasings per source.
    pub max_queries: usize,
    /// Most AI tokens (planning plus ranking).
    pub max_ai_tokens: u32,
    /// Longest a single source may take for all its queries.
    #[serde(with = "secs")]
    pub deadline: Duration,
    /// Stop starting new query rounds once this many exact matches were found.
    pub stop_after_exact: usize,
    /// Most query rounds (a round is one phrasing on all sources).
    pub max_rounds: usize,
}

mod secs {
    use std::time::Duration;

    pub fn serialize<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_secs())
    }
}

/// Which AI calls the mode makes when the user turned the AI on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUse {
    /// Ask the model for extra query phrasings.
    pub plan: bool,
    /// Ask the model to order results within their class and add a "why".
    pub rank: bool,
}

/// One mode's complete configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeConfig {
    /// Which mode.
    pub id: Mode,
    /// Short instructions added to the system prompt of the optional AI plan/rank calls.
    pub system_instructions: &'static str,
    /// How candidates are gathered.
    pub retrieval: Retrieval,
    /// Hits asked of each source per query.
    pub per_source_cap: usize,
    /// Query phrasings allowed besides the user's own words (AI plan and deterministic
    /// variants together).
    pub query_variants: usize,
    /// Verification rules.
    pub verification: Verification,
    /// The fields a result card shows in this mode (hints for the page; the wire type always
    /// carries all of them).
    pub result_schema: &'static [&'static str],
    /// The budget.
    pub budget: Budget,
    /// AI calls made when the AI is on.
    pub ai: AiUse,
    /// Categories this mode accepts; empty means every one of [`Category::SEARCH`].
    pub only_categories: &'static [Category],
}

/// Quick search: strict, cheap, fast.
pub const QUICK_SEARCH: ModeConfig = ModeConfig {
    id: Mode::QuickSearch,
    system_instructions: "Quick search: the user wants the one exact item fast. Prefer results \
        whose title states the requested episode, season, version or year. Never rank a result \
        above one that matches the stated numbers better.",
    retrieval: Retrieval::Strict,
    per_source_cap: 6,
    query_variants: 1,
    verification: Verification {
        related: RelatedPolicy::OnlyIfNothingBetter,
        compare_sources: false,
        note_contradictions: false,
        flag_mirrors: false,
        prefer_official: false,
    },
    result_schema: &["title", "source", "confidence", "size", "year", "license"],
    budget: Budget {
        max_sources: 4,
        max_queries: 2,
        max_ai_tokens: 1200,
        deadline: Duration::from_secs(20),
        stop_after_exact: 5,
        max_rounds: 1,
    },
    ai: AiUse {
        plan: false,
        rank: true,
    },
    only_categories: &[],
};

/// Deep research: broad, compares sources, bounded.
pub const DEEP_RESEARCH: ModeConfig = ModeConfig {
    id: Mode::DeepResearch,
    system_instructions: "Deep research: the user wants a thorough, well-sourced set of \
        results. Add phrasings that use other names, spellings and the terms experts use. When \
        results disagree (year, version, size) say so in the reason instead of hiding it. \
        Prefer primary sources over copies.",
    retrieval: Retrieval::Broad,
    per_source_cap: 10,
    query_variants: 3,
    verification: Verification {
        related: RelatedPolicy::Always,
        compare_sources: true,
        note_contradictions: true,
        flag_mirrors: true,
        prefer_official: true,
    },
    result_schema: &[
        "title",
        "source",
        "confidence",
        "reasons",
        "also_in",
        "year",
        "publisher",
        "version",
        "size",
        "license",
        "notes",
    ],
    budget: Budget {
        max_sources: 8,
        max_queries: 4,
        max_ai_tokens: 4000,
        deadline: Duration::from_secs(45),
        stop_after_exact: 12,
        max_rounds: 3,
    },
    ai: AiUse {
        plan: true,
        rank: true,
    },
    only_categories: &[],
};

/// Video search: videos only.
pub const VIDEO_SEARCH: ModeConfig = ModeConfig {
    id: Mode::VideoSearch,
    system_instructions: "Video search: the user wants a video, not an article about it. Prefer \
        the full episode or film over trailers, reviews, reactions and clips unless the user \
        asked for those. The episode and season numbers in the title decide the match.",
    retrieval: Retrieval::Video,
    per_source_cap: 8,
    query_variants: 2,
    verification: Verification {
        related: RelatedPolicy::OnlyIfNothingBetter,
        compare_sources: false,
        note_contradictions: false,
        flag_mirrors: false,
        prefer_official: false,
    },
    result_schema: &[
        "title",
        "source",
        "confidence",
        "duration",
        "thumbnail",
        "publisher",
        "size",
    ],
    budget: Budget {
        max_sources: 5,
        max_queries: 3,
        max_ai_tokens: 1500,
        deadline: Duration::from_secs(40),
        stop_after_exact: 6,
        max_rounds: 2,
    },
    ai: AiUse {
        plan: false,
        rank: true,
    },
    only_categories: &[Category::Video],
};

/// Source finder: the official home of something.
pub const SOURCE_FINDER: ModeConfig = ModeConfig {
    id: Mode::SourceFinder,
    system_instructions: "Source finder: the user wants the official or publisher's own copy. \
        Prefer the publisher, project or author's own site or repository over aggregators, \
        mirrors and re-uploads, and say in the reason which one it is.",
    retrieval: Retrieval::OfficialFirst,
    per_source_cap: 8,
    query_variants: 2,
    verification: Verification {
        related: RelatedPolicy::OnlyIfNothingBetter,
        compare_sources: true,
        note_contradictions: true,
        flag_mirrors: true,
        prefer_official: true,
    },
    result_schema: &[
        "title",
        "source",
        "confidence",
        "publisher",
        "trust",
        "version",
        "license",
    ],
    budget: Budget {
        max_sources: 6,
        max_queries: 3,
        max_ai_tokens: 2000,
        deadline: Duration::from_secs(30),
        stop_after_exact: 5,
        max_rounds: 2,
    },
    ai: AiUse {
        plan: true,
        rank: true,
    },
    only_categories: &[],
};

impl Mode {
    /// Every mode.
    pub const ALL: [Mode; 4] = [
        Mode::QuickSearch,
        Mode::DeepResearch,
        Mode::VideoSearch,
        Mode::SourceFinder,
    ];

    /// The mode's configuration.
    pub fn config(self) -> &'static ModeConfig {
        match self {
            Mode::QuickSearch => &QUICK_SEARCH,
            Mode::DeepResearch => &DEEP_RESEARCH,
            Mode::VideoSearch => &VIDEO_SEARCH,
            Mode::SourceFinder => &SOURCE_FINDER,
        }
    }

    /// Stable id (the same string serde uses).
    pub fn id(self) -> &'static str {
        match self {
            Mode::QuickSearch => "quick-search",
            Mode::DeepResearch => "deep-research",
            Mode::VideoSearch => "video-search",
            Mode::SourceFinder => "source-finder",
        }
    }
}

impl ModeConfig {
    /// Does this mode accept `category`?
    pub fn accepts(&self, category: Category) -> bool {
        self.only_categories.is_empty() || self.only_categories.contains(&category)
    }

    /// The sources this mode searches for `category`: the category's sources in priority order,
    /// cut to the budget (and, for [`Retrieval::OfficialFirst`], with primary catalogues first).
    /// `only` narrows to the sources the user picked.
    pub fn pick_sources(&self, category: Category, only: Option<&[Source]>) -> Vec<Source> {
        let mut list: Vec<Source> = category
            .sources()
            .iter()
            .copied()
            .filter(|s| only.is_none_or(|o| o.contains(s)))
            .collect();
        if self.retrieval == Retrieval::OfficialFirst {
            // stable: primary catalogues keep their relative order and move to the front
            list.sort_by_key(|s| u8::from(!crate::relevance::is_primary_source(*s)));
        }
        list.truncate(self.budget.max_sources);
        list
    }

    /// Why a search should not start another round, if it should not.
    pub fn stop_reason(&self, state: &RoundState) -> Option<StopReason> {
        if state.rounds_done >= self.budget.max_rounds {
            Some(StopReason::MaxRounds)
        } else if state.exact_found >= self.budget.stop_after_exact {
            Some(StopReason::EnoughExact)
        } else if state.elapsed >= self.budget.deadline {
            Some(StopReason::Deadline)
        } else if state.ai_tokens >= self.budget.max_ai_tokens {
            Some(StopReason::TokenBudget)
        } else if state.queries_left == 0 {
            Some(StopReason::NoMoreQueries)
        } else {
            None
        }
    }
}

/// Where a search stands between two rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RoundState {
    /// Rounds already run.
    pub rounds_done: usize,
    /// Exact matches found so far.
    pub exact_found: usize,
    /// Time spent so far.
    pub elapsed: Duration,
    /// AI tokens used so far.
    pub ai_tokens: u32,
    /// Query phrasings not yet run.
    pub queries_left: usize,
}

/// Why a search ended its rounds early.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    /// The round limit of the mode was reached.
    MaxRounds,
    /// Enough exact matches were found.
    EnoughExact,
    /// The time budget ran out.
    Deadline,
    /// The AI token budget ran out.
    TokenBudget,
    /// Every phrasing had been run.
    NoMoreQueries,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_a_bounded_budget_and_instructions() {
        for m in Mode::ALL {
            let c = m.config();
            assert_eq!(c.id, m);
            assert!(!c.system_instructions.is_empty());
            assert!(c.budget.max_sources >= 1 && c.budget.max_sources <= 10);
            assert!(c.budget.max_queries >= 1 && c.budget.max_queries <= 6);
            assert!(c.budget.max_rounds >= 1);
            assert!(c.budget.max_ai_tokens <= 6000);
            assert!(c.budget.deadline <= Duration::from_secs(60));
            assert!(c.per_source_cap >= 1 && c.per_source_cap <= 20);
            assert!(!c.result_schema.is_empty());
        }
    }

    #[test]
    fn quick_is_cheaper_than_deep() {
        let (q, d) = (&QUICK_SEARCH, &DEEP_RESEARCH);
        assert!(q.budget.max_sources < d.budget.max_sources);
        assert!(q.budget.max_ai_tokens < d.budget.max_ai_tokens);
        assert!(q.budget.deadline < d.budget.deadline);
        // at most one AI call
        assert!(!(q.ai.plan && q.ai.rank));
        assert_eq!(q.verification.related, RelatedPolicy::OnlyIfNothingBetter);
        assert_eq!(d.verification.related, RelatedPolicy::Always);
        assert!(d.verification.compare_sources && d.verification.note_contradictions);
    }

    #[test]
    fn video_search_accepts_videos_only() {
        let v = Mode::VideoSearch.config();
        assert!(v.accepts(Category::Video));
        assert!(!v.accepts(Category::Documents));
        assert!(Mode::QuickSearch.config().accepts(Category::Documents));
    }

    #[test]
    fn sources_are_cut_to_the_budget_in_priority_order() {
        let quick = QUICK_SEARCH.pick_sources(Category::Video, None);
        assert_eq!(quick.len(), 4);
        assert_eq!(quick[0], Source::Youtube);
        let deep = DEEP_RESEARCH.pick_sources(Category::Video, None);
        assert_eq!(deep.len(), 5);
        let only = QUICK_SEARCH.pick_sources(Category::Video, Some(&[Source::Nasa]));
        assert_eq!(only, [Source::Nasa]);
        assert!(QUICK_SEARCH.pick_sources(Category::All, None).is_empty());
    }

    #[test]
    fn stop_conditions_are_explicit() {
        let c = Mode::DeepResearch.config();
        let base = RoundState {
            queries_left: 2,
            ..RoundState::default()
        };
        assert_eq!(c.stop_reason(&base), None);
        assert_eq!(
            c.stop_reason(&RoundState {
                rounds_done: 3,
                ..base
            }),
            Some(StopReason::MaxRounds)
        );
        assert_eq!(
            c.stop_reason(&RoundState {
                exact_found: 12,
                ..base
            }),
            Some(StopReason::EnoughExact)
        );
        assert_eq!(
            c.stop_reason(&RoundState {
                elapsed: Duration::from_secs(46),
                ..base
            }),
            Some(StopReason::Deadline)
        );
        assert_eq!(
            c.stop_reason(&RoundState {
                ai_tokens: 4000,
                ..base
            }),
            Some(StopReason::TokenBudget)
        );
        assert_eq!(
            c.stop_reason(&RoundState {
                queries_left: 0,
                ..base
            }),
            Some(StopReason::NoMoreQueries)
        );
    }

    #[test]
    fn modes_round_trip_through_their_wire_ids() {
        for m in Mode::ALL {
            let json = serde_json::to_string(&m).unwrap();
            assert_eq!(json, format!("\"{}\"", m.id()));
            assert_eq!(serde_json::from_str::<Mode>(&json).unwrap(), m);
        }
        assert_eq!(Mode::default(), Mode::QuickSearch);
    }
}
