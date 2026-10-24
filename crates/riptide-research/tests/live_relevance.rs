//! Live relevance benchmark (manual): a handful of real queries against the real sources, judged
//! by a regular expression per query that was written from the query text alone, then run through
//! the app's selection pipeline. Prints the same metrics as the offline benchmark plus the
//! precision of the raw interleaved source results for comparison.
//!
//! ```text
//! cargo test -p riptide-research --test live_relevance -- --ignored --nocapture
//! ```
//!
//! Needs the network. The video case also needs yt-dlp: set `RIPTIDE_YTDLP` to its path (it is
//! skipped otherwise). Results change as the services change; this is a smoke measurement, not
//! a gate.

use std::path::PathBuf;
use std::sync::Arc;

use regex::Regex;
use riptide_research::bench::{metrics, select, table, CaseResult, Shown};
use riptide_research::queries::variants;
use riptide_research::sources::youtube::{ProcessRunner, Youtube};
use riptide_research::{
    default_source, search_all_in, Category, Hit, Mode, QueryIntent, SearchSource, Source,
};

/// One live query and the pattern that says what a correct result looks like.
struct Live {
    query: &'static str,
    category: Category,
    mode: Mode,
    /// Matches `title + file name` of a result that is what the user asked for.
    correct: &'static str,
}

const CASES: &[Live] = &[
    Live {
        query: "attention is all you need pdf",
        category: Category::Academic,
        mode: Mode::QuickSearch,
        correct: r"(?i)^\s*attention is all you need\s*$",
    },
    Live {
        query: "big buck bunny",
        category: Category::Video,
        mode: Mode::VideoSearch,
        correct: r"(?i)\bbig buck bunny\b",
    },
    Live {
        query: "sintel 2010",
        category: Category::Video,
        mode: Mode::VideoSearch,
        correct: r"(?i)\bsintel\b",
    },
    Live {
        query: "pride and prejudice",
        category: Category::Documents,
        mode: Mode::QuickSearch,
        correct: r"(?i)^\s*pride and prejudice\s*$",
    },
    Live {
        query: "hubble deep field",
        category: Category::Images,
        mode: Mode::QuickSearch,
        correct: r"(?i)\bhubble deep field\b",
    },
    Live {
        query: "ripgrep",
        category: Category::OpenSource,
        mode: Mode::QuickSearch,
        correct: r"(?i)^\s*burntsushi/ripgrep\s*$",
    },
    Live {
        query: "naruto shippuden episode 22",
        category: Category::Video,
        mode: Mode::VideoSearch,
        correct: r"(?i)naruto.*shippuden.*\b(?:episode|ep)\.?\s*0*22\b",
    },
];

fn yt_dlp() -> Option<PathBuf> {
    std::env::var_os("RIPTIDE_YTDLP")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

fn adapters(category: Category) -> Vec<Arc<dyn SearchSource>> {
    category
        .sources()
        .iter()
        .filter_map(|s| match s {
            Source::Youtube => yt_dlp().map(|p| {
                Arc::new(Youtube::new(Box::new(ProcessRunner::new(p)))) as Arc<dyn SearchSource>
            }),
            other => default_source(*other),
        })
        .collect()
}

fn text_of(h: &Hit) -> String {
    format!("{} {}", h.title, h.filename.as_deref().unwrap_or(""))
}

#[tokio::test]
#[ignore = "needs the network"]
async fn live_relevance_benchmark() {
    let mut results: Vec<CaseResult> = Vec::new();
    let mut raw_precision: Vec<f64> = Vec::new();
    for case in CASES {
        let mode = case.mode.config();
        let pick = mode.pick_sources(case.category, None);
        let list: Vec<Arc<dyn SearchSource>> = adapters(case.category)
            .into_iter()
            .filter(|a| pick.contains(&a.source()))
            .collect();
        if list.is_empty() {
            println!(
                "skipped {:?}: no source available (yt-dlp missing?)",
                case.query
            );
            continue;
        }
        let intent = QueryIntent::parse(case.query);
        let queries = variants(&intent, case.mode);
        let outcome = search_all_in(&list, &queries, mode.per_source_cap, case.category).await;
        let correct = Regex::new(case.correct).expect("pattern");
        let raw: Vec<Hit> = outcome.hits.clone();
        let raw_top5: Vec<&Hit> = raw.iter().take(5).collect();
        if !raw_top5.is_empty() {
            raw_precision.push(
                raw_top5
                    .iter()
                    .filter(|h| correct.is_match(&text_of(h)))
                    .count() as f64
                    / raw_top5.len() as f64,
            );
        }
        let relevant: Vec<String> = raw
            .iter()
            .filter(|h| correct.is_match(&text_of(h)))
            .map(|h| h.id.clone())
            .collect();
        let shown = select(case.query, case.mode, raw.clone());
        println!(
            "\n{:?} [{} sources, {} raw hits, {} shown, {} failures]",
            case.query,
            list.len(),
            raw.len(),
            shown.len(),
            outcome.failures.len()
        );
        for f in &outcome.failures {
            println!("   source {} failed: {}", f.source.id(), f.code);
        }
        for a in shown.iter().take(5) {
            println!(
                "   {:<8} {} {}",
                format!("{:?}", a.assessment.class),
                if correct.is_match(&text_of(&a.hit)) {
                    "ok "
                } else {
                    "BAD"
                },
                a.hit.title
            );
        }
        results.push(CaseResult {
            id: case.query.into(),
            subset: "live".into(),
            query: case.query.into(),
            shown: shown
                .iter()
                .map(|a| Shown {
                    id: a.hit.id.clone(),
                    class: a.assessment.class,
                    relevant: correct.is_match(&text_of(&a.hit)),
                    group: None,
                })
                .collect(),
            relevant,
            offered: raw.len(),
        });
    }
    println!("\n{}", table(&results));
    let all: Vec<&CaseResult> = results.iter().collect();
    let m = metrics(&all);
    println!(
        "raw interleaved source results: mean Precision@5 {:.3} over {} queries; with the relevance gate {:.3}",
        if raw_precision.is_empty() {
            0.0
        } else {
            raw_precision.iter().sum::<f64>() / raw_precision.len() as f64
        },
        raw_precision.len(),
        m.precision_at_5
    );
}
