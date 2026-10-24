//! The relevance evaluation harness: labelled cases, the same selection pipeline the app runs
//! (de-duplication, relevance gate, ordering) and the metrics reported by the offline benchmark
//! (`tests/relevance_benchmark.rs`) and the optional live run (`tests/live_relevance.rs`).
//!
//! Ground truth is decided by the case author from the query text alone; the engine never sees
//! the labels.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use serde::Deserialize;

use crate::intent::QueryIntent;
use crate::modes::Mode;
use crate::relevance::{order, Assessed, Gate, MatchClass};
use crate::search::Dedup;
use crate::types::{Category, Hit, HitKind, Source};

/// A candidate result in a labelled case (compact keys keep the fixtures readable).
#[derive(Debug, Clone, Deserialize)]
pub struct Candidate {
    /// Id within the case.
    pub id: String,
    /// Source.
    pub source: Source,
    /// Title.
    pub title: String,
    /// File name.
    #[serde(default)]
    pub f: Option<String>,
    /// Summary.
    #[serde(default)]
    pub s: Option<String>,
    /// Year.
    #[serde(default)]
    pub y: Option<u16>,
    /// Version.
    #[serde(default)]
    pub v: Option<String>,
    /// Publisher.
    #[serde(default)]
    pub p: Option<String>,
    /// Size in bytes.
    #[serde(default)]
    pub z: Option<u64>,
    /// Download URL (a distinct default is made up when absent).
    #[serde(default)]
    pub u: Option<String>,
    /// A page, not a file.
    #[serde(default)]
    pub pg: bool,
    /// DOI.
    #[serde(default)]
    pub d: Option<String>,
    /// arXiv id.
    #[serde(default)]
    pub ax: Option<String>,
    /// Duplicate group: candidates in one group are the same item.
    #[serde(default)]
    pub g: Option<String>,
    /// Authors.
    #[serde(default)]
    pub a: Vec<String>,
    /// Duration in seconds.
    #[serde(default)]
    pub dur: Option<u64>,
}

impl Candidate {
    /// The hit a source would have returned for this candidate.
    pub fn to_hit(&self) -> Hit {
        let url = self
            .u
            .clone()
            .unwrap_or_else(|| format!("https://example.test/{}/{}", self.source.id(), self.id));
        let mut h = Hit::new(self.source, &self.id, &self.title, &url);
        h.filename = self.f.clone();
        h.summary = self.s.clone();
        h.year = self.y;
        h.version = self.v.clone();
        h.publisher = self.p.clone();
        h.size = self.z;
        h.doi = self.d.clone();
        h.arxiv = self.ax.clone();
        h.authors = self.a.clone();
        h.duration_secs = self.dur;
        if self.pg {
            h.kind = HitKind::Page;
            h.page_url = Some(url);
        }
        h
    }
}

/// One labelled query.
#[derive(Debug, Clone, Deserialize)]
pub struct Case {
    /// Case id.
    pub id: String,
    /// The query as the user typed it.
    pub query: String,
    /// Category.
    pub category: Category,
    /// Mode.
    pub mode: Mode,
    /// `exact`, `multilingual`, `semantic`, `none` or `duplicates`.
    pub subset: String,
    /// Why the case is hard, when it is.
    #[serde(default)]
    pub note: Option<String>,
    /// What the sources returned, in order.
    pub candidates: Vec<Candidate>,
    /// Ids of the candidates that really are what was asked for.
    pub relevant: Vec<String>,
}

/// A fixture file.
#[derive(Debug, Deserialize)]
pub struct CaseFile {
    /// The cases.
    pub queries: Vec<Case>,
}

/// What the app would show for these hits: duplicates dropped, unrelated dropped, related held
/// back per the mode, best first.
pub fn select(query: &str, mode: Mode, hits: Vec<Hit>) -> Vec<Assessed> {
    let mut dedup = Dedup::new();
    let mut gate = Gate::new(QueryIntent::parse(query), mode);
    let fresh: Vec<Hit> = hits.into_iter().filter(|h| dedup.admit(h)).collect();
    let mut shown = gate.offer(fresh);
    shown.extend(gate.finish());
    order(&mut shown, mode.config().verification.prefer_official);
    shown
}

/// One shown result, judged.
#[derive(Debug, Clone)]
pub struct Shown {
    /// Candidate id.
    pub id: String,
    /// Class the engine gave it.
    pub class: MatchClass,
    /// Is it in the ground truth?
    pub relevant: bool,
    /// Duplicate group.
    pub group: Option<String>,
}

/// A case after the pipeline ran.
#[derive(Debug, Clone)]
pub struct CaseResult {
    /// Case id.
    pub id: String,
    /// Subset.
    pub subset: String,
    /// The query.
    pub query: String,
    /// What was shown, best first.
    pub shown: Vec<Shown>,
    /// Ids that were relevant.
    pub relevant: Vec<String>,
    /// Candidates offered.
    pub offered: usize,
}

/// Runs the pipeline on a labelled case.
pub fn run_case(case: &Case) -> CaseResult {
    let hits: Vec<Hit> = case.candidates.iter().map(Candidate::to_hit).collect();
    let by_native: HashMap<String, &Candidate> = case
        .candidates
        .iter()
        .map(|c| (format!("{}:{}", c.source.id(), c.id), c))
        .collect();
    let relevant: HashSet<&str> = case.relevant.iter().map(String::as_str).collect();
    let shown = select(&case.query, case.mode, hits)
        .into_iter()
        .map(|a| {
            let cand = by_native.get(&a.hit.id);
            let native = cand.map_or("", |c| c.id.as_str());
            Shown {
                id: native.to_string(),
                class: a.assessment.class,
                relevant: relevant.contains(native),
                group: cand.and_then(|c| c.g.clone()),
            }
        })
        .collect();
    CaseResult {
        id: case.id.clone(),
        subset: case.subset.clone(),
        query: case.query.clone(),
        shown,
        relevant: case.relevant.clone(),
        offered: case.candidates.len(),
    }
}

/// The numbers reported for a set of cases.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Metrics {
    /// Cases in the set.
    pub queries: usize,
    /// Mean precision of the first five results over cases that have a relevant result.
    pub precision_at_5: f64,
    /// Mean share of the relevant results found in the first ten.
    pub recall_at_10: f64,
    /// Share of cases whose first result is relevant.
    pub top1_accuracy: f64,
    /// Share of shown exact/strong results that are not relevant (all cases).
    pub false_positive_rate: f64,
    /// Share of shown results that repeat an earlier shown item.
    pub duplicate_rate: f64,
    /// Over cases with no relevant result: share that correctly show no exact/strong result.
    pub abstention: f64,
    /// Cases that have no relevant result.
    pub no_answer_cases: usize,
    /// Cases that have at least one relevant result.
    pub answer_cases: usize,
}

fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Computes the metrics of `results`.
pub fn metrics(results: &[&CaseResult]) -> Metrics {
    let (mut p5, mut r10, mut top1, mut abst) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut confident, mut confident_wrong) = (0usize, 0usize);
    let (mut shown_total, mut dups) = (0usize, 0usize);
    for r in results {
        let has_answer = !r.relevant.is_empty();
        if has_answer {
            let top5 = &r.shown[..r.shown.len().min(5)];
            let hit5 = top5.iter().filter(|s| s.relevant).count();
            p5.push(if top5.is_empty() {
                0.0
            } else {
                hit5 as f64 / top5.len() as f64
            });
            let top10 = &r.shown[..r.shown.len().min(10)];
            r10.push(top10.iter().filter(|s| s.relevant).count() as f64 / r.relevant.len() as f64);
            top1.push(f64::from(u8::from(
                r.shown.first().is_some_and(|s| s.relevant),
            )));
        } else {
            let none_confident = !r
                .shown
                .iter()
                .any(|s| matches!(s.class, MatchClass::Exact | MatchClass::Strong));
            abst.push(f64::from(u8::from(none_confident)));
        }
        for s in &r.shown {
            if matches!(s.class, MatchClass::Exact | MatchClass::Strong) {
                confident += 1;
                if !s.relevant {
                    confident_wrong += 1;
                }
            }
        }
        let mut groups: HashSet<&str> = HashSet::new();
        for s in &r.shown {
            shown_total += 1;
            if let Some(g) = s.group.as_deref() {
                if !groups.insert(g) {
                    dups += 1;
                }
            }
        }
    }
    let ratio = |a: usize, b: usize| if b == 0 { 0.0 } else { a as f64 / b as f64 };
    Metrics {
        queries: results.len(),
        precision_at_5: mean(&p5),
        recall_at_10: mean(&r10),
        top1_accuracy: mean(&top1),
        false_positive_rate: ratio(confident_wrong, confident),
        duplicate_rate: ratio(dups, shown_total),
        abstention: mean(&abst),
        no_answer_cases: abst.len(),
        answer_cases: p5.len(),
    }
}

/// A printable table: one row per subset plus the total.
pub fn table(results: &[CaseResult]) -> String {
    let mut subsets: Vec<&str> = results.iter().map(|r| r.subset.as_str()).collect();
    subsets.sort_unstable();
    subsets.dedup();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<14} {:>4} {:>7} {:>7} {:>7} {:>7} {:>7} {:>9}",
        "subset", "n", "P@5", "R@10", "top1", "FP", "dup", "abstain"
    );
    let mut row = |name: &str, m: &Metrics| {
        let abst = if m.no_answer_cases == 0 {
            "-".to_string()
        } else {
            format!("{:.2}", m.abstention)
        };
        let num = |v: f64| {
            if m.answer_cases == 0 {
                "-".to_string()
            } else {
                format!("{v:.3}")
            }
        };
        let _ = writeln!(
            out,
            "{:<14} {:>4} {:>7} {:>7} {:>7} {:>7.3} {:>7.3} {:>9}",
            name,
            m.queries,
            num(m.precision_at_5),
            num(m.recall_at_10),
            num(m.top1_accuracy),
            m.false_positive_rate,
            m.duplicate_rate,
            abst
        );
    };
    for s in &subsets {
        let set: Vec<&CaseResult> = results.iter().filter(|r| r.subset == *s).collect();
        row(s, &metrics(&set));
    }
    let all: Vec<&CaseResult> = results.iter().collect();
    row("ALL", &metrics(&all));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(id: &str, class: MatchClass, relevant: bool, group: Option<&str>) -> Shown {
        Shown {
            id: id.into(),
            class,
            relevant,
            group: group.map(str::to_string),
        }
    }

    #[test]
    fn metrics_follow_their_definitions() {
        let a = CaseResult {
            id: "a".into(),
            subset: "exact".into(),
            query: "q".into(),
            shown: vec![
                shown("1", MatchClass::Exact, true, Some("g")),
                shown("2", MatchClass::Strong, false, None),
                shown("3", MatchClass::Exact, true, Some("g")),
                shown("4", MatchClass::Related, false, None),
            ],
            relevant: vec!["1".into(), "3".into(), "9".into()],
            offered: 6,
        };
        let none = CaseResult {
            id: "b".into(),
            subset: "none".into(),
            query: "q".into(),
            shown: vec![shown("1", MatchClass::Related, false, None)],
            relevant: vec![],
            offered: 3,
        };
        let m = metrics(&[&a, &none]);
        assert!((m.precision_at_5 - 0.5).abs() < 1e-9);
        assert!((m.recall_at_10 - 2.0 / 3.0).abs() < 1e-9);
        assert!((m.top1_accuracy - 1.0).abs() < 1e-9);
        // 3 confident results, 1 wrong
        assert!((m.false_positive_rate - 1.0 / 3.0).abs() < 1e-9);
        // 5 shown, one repeats group g
        assert!((m.duplicate_rate - 1.0 / 5.0).abs() < 1e-9);
        assert!((m.abstention - 1.0).abs() < 1e-9);
        assert!(table(&[a, none]).contains("ALL"));
    }
}
