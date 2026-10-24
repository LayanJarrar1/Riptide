//! Offline relevance benchmark: labelled queries with realistic candidates (including near-miss
//! decoys such as the neighbouring episodes) are run through the same de-duplication, relevance
//! gate and ordering the app uses. Fixtures: `tests/fixtures/benchmark/*.json`. No network.
//!
//! Run `cargo test -p riptide-research --test relevance_benchmark -- --nocapture` to see the table.

use std::path::Path;

use riptide_research::bench::{metrics, run_case, table, Case, CaseFile, CaseResult};
use riptide_research::MatchClass;

fn load() -> Vec<Case> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/benchmark");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixture dir")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
        .iter()
        .flat_map(|p| {
            let text = std::fs::read_to_string(p).expect("read fixture");
            serde_json::from_str::<CaseFile>(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
                .queries
        })
        .collect()
}

#[test]
fn fixtures_are_well_formed() {
    let cases = load();
    assert!(cases.len() >= 40, "{} queries", cases.len());
    let mut ids = std::collections::HashSet::new();
    for c in &cases {
        assert!(ids.insert(c.id.clone()), "duplicate case id {}", c.id);
        let cands: std::collections::HashSet<&str> =
            c.candidates.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(
            cands.len(),
            c.candidates.len(),
            "{}: duplicate candidate id",
            c.id
        );
        for r in &c.relevant {
            assert!(
                cands.contains(r.as_str()),
                "{}: unknown relevant id {r}",
                c.id
            );
        }
        assert!(c.candidates.len() >= 3, "{}", c.id);
        if c.subset == "none" {
            assert!(c.relevant.is_empty(), "{}", c.id);
        } else {
            assert!(!c.relevant.is_empty(), "{}", c.id);
        }
    }
    // the categories the task names are all covered
    for subset in ["exact", "multilingual", "semantic", "none", "duplicates"] {
        assert!(cases.iter().any(|c| c.subset == subset), "{subset}");
    }
}

#[test]
fn relevance_benchmark() {
    let cases = load();
    let results: Vec<CaseResult> = cases.iter().map(run_case).collect();
    println!("\n{}", table(&results));
    println!("per-query detail (P@5 below 1.0 or a missed answer):");
    for (case, r) in cases.iter().zip(&results) {
        let wrong: Vec<String> = r
            .shown
            .iter()
            .take(5)
            .filter(|s| !s.relevant)
            .map(|s| format!("{}[{:?}]", s.id, s.class))
            .collect();
        let missed: Vec<&String> = r
            .relevant
            .iter()
            .filter(|id| !r.shown.iter().any(|s| &s.id == *id))
            .collect();
        if !wrong.is_empty() || !missed.is_empty() {
            println!(
                "  {:<26} shown {:>2}/{:>2} wrong-in-top5 {:?} missed {:?}{}",
                r.id,
                r.shown.len(),
                r.offered,
                wrong,
                missed,
                case.note
                    .as_deref()
                    .map(|n| format!("  ({n})"))
                    .unwrap_or_default()
            );
        }
    }

    // the unrelated class is never shown
    for r in &results {
        assert!(r.shown.iter().all(|s| s.class != MatchClass::Unrelated));
    }

    let exact: Vec<&CaseResult> = results.iter().filter(|r| r.subset == "exact").collect();
    assert!(
        exact.len() >= 25,
        "exact-intent subset has {} queries",
        exact.len()
    );
    let m = metrics(&exact);
    assert!(
        m.precision_at_5 >= 0.95,
        "Precision@5 on the exact-intent subset is {:.3}",
        m.precision_at_5
    );
}
