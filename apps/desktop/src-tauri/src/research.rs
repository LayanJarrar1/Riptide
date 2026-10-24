//! The Research page's backend: streaming open-source search (with optional AI help), the
//! "already downloaded" lookups and adding results to the download list.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `research_start` | `request: SearchRequest` | [`StartResponse`] (the search runs in the background) |
//! | `research_cancel` | `searchId` | - (stops everything, closes the AI stream) |
//! | `research_retry_source` | `searchId`, `source` | - (searches that one source again) |
//! | `research_preview` | `topic` | the exact messages the planning request would send |
//! | `research_add` | `request: AddRequest` | [`AddResponse`] |
//! | `research_settings` / `research_save_settings` | - / `settings` | [`SettingsView`] |
//! | `research_set_secret` / `research_remove_secret` | `name`, `value` / `name` | [`SettingsView`] |
//! | `seen_check` | `queries: [SeenQuery]` | one [`riptide_engine::SeenMatch`] or `null` per query |
//! | `seen_settings` / `seen_set_skip` / `seen_forget` | - / `on` / - | skip flag / - / records removed |
//!
//! # Streaming
//!
//! `research_start` returns at once. Everything else reaches the webview on the
//! `riptide://research` event as [`Batch`]es (events are gathered for 100 ms, so at most ten
//! batches a second): each source reports `searching`, its hits as soon as it has them, then
//! `done` or `failed`; when the AI is on, the query plan and then the ranking follow, one entry
//! at a time, each validated against the results (see `riptide_research::assist`). The page is
//! usable from the first batch and a search can be cancelled at any time.
//!
//! # Relevance
//!
//! Nothing reaches the page unchecked. The query is understood first
//! ([`riptide_research::QueryIntent`]: episode, season, version, year, file type, phrases,
//! exclusions; Arabic folded), then every result is classified `exact` / `strong` / `related` /
//! `unrelated` by [`riptide_research::relevance`]. `Unrelated` results (a different episode, a
//! different version...) are dropped, `related` ones are shown only when the mode allows it or when
//! nothing better exists, and each item carries its class (`confidence`) and the reasons. The AI
//! can only reorder within a class: its scores are squeezed into the class's band, so it can
//! never promote a result. A search names its category (one of seven, no "all") and a
//! [`Mode`] (quick search, deep research, video search, source finder) whose budget bounds the
//! sources, phrasings, rounds, AI tokens and time spent.
//!
//! The model never sees a URL: ranking works on ids of results the adapters returned. Titles
//! and snippets are third-party text; they are escaped by the UI and never executed.
//! Download-history titles are included in prompts only while *Let the AI see my download
//! history* is on.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use riptide_ai::Message;
use riptide_core::{Category as FileCategory, DownloadKind, DownloadRequest, QueueId};
use riptide_engine::{Engine, Error as EngineError, SeenHint, SeenMatch, SeenQuery};
use riptide_research::assist::{self, Ranked, MAX_HISTORY_TITLES, MAX_RANK_INPUT};
use riptide_research::modes::{RoundState, StopReason};
use riptide_research::relevance::{
    compare, confine_score, order, Assessed, Comparison, Gate, MatchClass, Reason, Trust,
};
use riptide_research::sources::jamendo::Jamendo;
use riptide_research::sources::websearch::{validate_searxng_base, WebBackend, WebSearch};
use riptide_research::sources::youtube::{ProcessRunner, Youtube};
use riptide_research::{
    default_source, queries, search_stream, Category, Dedup, Hit, HitKind, Mode, QueryIntent,
    Rights, SearchEvent, SearchSource, Source,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::ai::{AiFail, AiService};

/// Webview event carrying [`Batch`]es.
pub const EVENT: &str = "riptide://research";
/// File (next to `ai-settings.json`) holding [`ResearchSettings`].
pub const SETTINGS_FILE: &str = "research-settings.json";
/// Names of the secrets this module stores in the Credential Manager.
pub const SECRET_NAMES: [&str; 2] = ["brave", "jamendo"];

/// Most results kept in memory for `research_add`.
const CACHE_MAX: usize = 1500;
/// Searches kept so a source can be retried; older ones are forgotten.
const KEEP_RUNS: usize = 3;
/// How long events are gathered before they are sent to the webview.
const BATCH_WINDOW: Duration = Duration::from_millis(100);
/// Most events in one batch.
const BATCH_MAX: usize = 512;

// ---------------------------------------------------------------- settings

/// Which general web search backend the user chose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WebChoice {
    /// No web search (the default).
    #[default]
    None,
    /// Brave Search API with the user's own token.
    Brave,
    /// A SearXNG instance the user names.
    Searxng,
}

/// What the user chose in Settings -> Research. Contains no secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ResearchSettings {
    /// The web search backend.
    pub web: WebChoice,
    /// Base URL of the SearXNG instance (validated).
    pub searxng_url: Option<String>,
}

/// Settings plus which secrets exist (masked hints only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// The saved settings.
    pub settings: ResearchSettings,
    /// Masked Brave token, when stored.
    pub brave_hint: Option<String>,
    /// Masked Jamendo client id, when stored.
    pub jamendo_hint: Option<String>,
    /// The web search is usable with these settings.
    pub web_ready: bool,
}

fn read_settings(ai: &AiService) -> ResearchSettings {
    std::fs::read_to_string(ai.data_dir().join(SETTINGS_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

fn view(ai: &AiService) -> SettingsView {
    let settings = read_settings(ai);
    let web_ready = web_backend(ai, &settings).is_some();
    SettingsView {
        brave_hint: ai.named_hint("brave"),
        jamendo_hint: ai.named_hint("jamendo"),
        settings,
        web_ready,
    }
}

/// The configured web backend, or `None` when the choice is "none" or something is missing.
fn web_backend(ai: &AiService, s: &ResearchSettings) -> Option<WebBackend> {
    match s.web {
        WebChoice::None => None,
        WebChoice::Brave => ai.named_secret("brave").map(|k| WebBackend::Brave {
            api_key: k.expose().to_string(),
        }),
        WebChoice::Searxng => s
            .searxng_url
            .as_deref()
            .and_then(|u| validate_searxng_base(u).ok())
            .map(|base_url| WebBackend::SearxNg { base_url }),
    }
}

/// Current Research settings.
#[tauri::command]
pub fn research_settings(ai: State<'_, AiService>) -> SettingsView {
    view(&ai)
}

/// Validates and saves the Research settings.
#[tauri::command]
pub fn research_save_settings(
    ai: State<'_, AiService>,
    mut settings: ResearchSettings,
) -> Result<SettingsView, String> {
    settings.searxng_url = settings
        .searxng_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty());
    if let Some(u) = &settings.searxng_url {
        settings.searxng_url = Some(validate_searxng_base(u).map_err(|e| e.to_string())?);
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    let path = ai.data_dir().join(SETTINGS_FILE);
    let tmp = path.with_extension("tmp");
    std::fs::create_dir_all(ai.data_dir())
        .and_then(|()| std::fs::write(&tmp, json))
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| format!("could not save: {e}"))?;
    Ok(view(&ai))
}

/// Stores a secret (`brave`: Brave Search API token, `jamendo`: Jamendo client id) in the
/// Credential Manager.
#[tauri::command]
pub fn research_set_secret(
    ai: State<'_, AiService>,
    name: String,
    value: String,
) -> Result<SettingsView, AiFail> {
    if !SECRET_NAMES.contains(&name.as_str()) {
        return Err(AiFail {
            code: "config",
            message: "unknown secret".into(),
        });
    }
    ai.set_named_secret(&name, value)?;
    Ok(view(&ai))
}

/// Removes a stored secret.
#[tauri::command]
pub fn research_remove_secret(
    ai: State<'_, AiService>,
    name: String,
) -> Result<SettingsView, AiFail> {
    if !SECRET_NAMES.contains(&name.as_str()) {
        return Err(AiFail {
            code: "config",
            message: "unknown secret".into(),
        });
    }
    ai.remove_named_secret(&name)?;
    Ok(view(&ai))
}

// ---------------------------------------------------------------- wire types

/// What the Research page sends.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    /// The user's topic or question.
    pub topic: String,
    /// Use the AI to plan queries and rank results (ignored when it is not configured).
    #[serde(default)]
    pub use_ai: bool,
    /// UI language (`ar` / `en`): the language of the "why this matches" lines.
    #[serde(default)]
    pub lang: String,
    /// What kind of thing to look for. Required: one of the seven search categories (the old
    /// ids `papers`, `books` and `games` still map to `academic`, `documents` and `open-source`;
    /// `all`, `music` and `datasets` are refused).
    pub category: Category,
    /// How to search (default: quick search).
    #[serde(default)]
    pub mode: Mode,
    /// Restrict to these sources; all that serve the category when absent.
    #[serde(default)]
    pub sources: Option<Vec<Source>>,
    /// Hits per source and query (default 6).
    #[serde(default)]
    pub per_source: Option<usize>,
}

/// One result card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultItem {
    /// The hit.
    #[serde(flatten)]
    pub hit: Hit,
    /// The licence badge.
    pub rights: Rights,
    /// Already downloaded?
    pub seen: Option<SeenMatch>,
    /// How well it matches the query: `exact`, `strong` or `related` (a related result is
    /// flagged as such; `unrelated` is never sent).
    pub confidence: MatchClass,
    /// Share (0-1) of the title words found in the title, file name or by-line.
    pub coverage: f32,
    /// 0-100: orders results *within* a confidence class (the AI's score is confined to the same
    /// band, see [`Ranked`] events).
    pub match_score: u8,
    /// What was verified: matched, missing and conflicting constraints, word coverage.
    pub reasons: Vec<Reason>,
    /// `official`, `primary`, `unknown` or `mirror` (the source finder shows it).
    pub trust: Trust,
    /// Looks like a mirror or aggregator copy.
    pub mirror: bool,
}

impl ResultItem {
    fn new(a: Assessed) -> Self {
        let rights = a.hit.rights();
        ResultItem {
            hit: a.hit,
            rights,
            seen: None,
            confidence: a.assessment.class,
            coverage: a.assessment.coverage,
            match_score: a.assessment.score,
            reasons: a.assessment.reasons,
            trust: a.assessment.trust,
            mirror: a.assessment.mirror,
        }
    }
}

/// A ranking entry as the page receives it: the model's order and reason, with the score
/// confined to the band of the result's confidence class.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedItem {
    /// The hit id.
    pub id: String,
    /// 0-100, already confined to the class band: sorting by it never moves a result across a
    /// class.
    pub score: u8,
    /// One line on why it matches (no links).
    pub why: String,
    /// The result's confidence class (the one the program computed, not the model).
    pub confidence: MatchClass,
}

/// A source that cannot take part, and why.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Unavailable {
    /// Which source.
    pub source: Source,
    /// `needs_setup`: the user has not entered the key / address it needs yet.
    pub reason: &'static str,
}

/// What `research_start` answers with.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartResponse {
    /// Id carried by every event of this search.
    pub search_id: u64,
    /// The sources that will be searched.
    pub sources: Vec<Source>,
    /// Sources that serve the category but are not set up.
    pub unavailable: Vec<Unavailable>,
    /// The AI will plan and rank (it is on and was asked for).
    pub ai: bool,
    /// The mode that runs.
    pub mode: Mode,
    /// The category searched (always one of the seven).
    pub category: Category,
    /// What was understood of the query (episode, season, version, year, file type, phrases,
    /// exclusions, language), for the page to show as chips.
    pub intent: QueryIntent,
    /// The phrasings that run first (more may follow from the AI plan and later rounds).
    pub queries: Vec<String>,
}

/// What the AI did during a search.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiOutcome {
    /// The caller asked for AI help.
    pub requested: bool,
    /// The AI was configured and ran (at least the planning step).
    pub used: bool,
    /// The AI step that failed, if any; the search continued without it.
    pub error: Option<AiFail>,
    /// Tokens used by this search.
    pub tokens: u64,
    /// Entries of the model's answer dropped because they cited something not in the results.
    pub dropped: usize,
}

/// One progress event of a search.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Ev {
    /// The search moved on: `searching`, `ranking`, `done` or `cancelled`.
    Phase {
        /// The phase.
        phase: &'static str,
    },
    /// One source's state: `searching`, `done` or `failed`.
    Source {
        /// Which source.
        source: Source,
        /// The state.
        state: &'static str,
        /// Hits it contributed so far.
        found: usize,
        /// Failure code (`timeout`, `network`, `rate_limited`, `http`, `parse`, `invalid`).
        code: Option<String>,
    },
    /// New results, in neutral order.
    #[serde(rename_all = "camelCase")]
    Hits {
        /// Which source.
        source: Source,
        /// The results.
        items: Vec<ResultItem>,
    },
    /// The query phrasings the AI added.
    Plan {
        /// The queries.
        queries: Vec<String>,
    },
    /// Validated ranking entries.
    Rank {
        /// The entries, best first within this batch.
        items: Vec<RankedItem>,
    },
    /// Which other sources returned the same items and where sources disagree (deep research
    /// and the source finder), sent once when the results are in.
    Compare {
        /// The comparison.
        comparison: Comparison,
    },
    /// How the search ended, sent once before `end`.
    #[serde(rename_all = "camelCase")]
    Summary {
        /// Exact matches shown.
        exact: usize,
        /// Strong matches shown.
        strong: usize,
        /// Results judged unrelated (or related and never shown): hidden from the page.
        hidden: usize,
        /// Why the later rounds were skipped, when they were.
        stop: Option<StopReason>,
    },
    /// The AI part is over.
    Ai {
        /// What happened.
        outcome: AiOutcome,
    },
    /// Nothing more will be sent for this search.
    End,
}

/// What reaches the webview: the events of one 100 ms window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    /// Which search.
    pub search_id: u64,
    /// The events, in order.
    pub events: Vec<Ev>,
}

// ---------------------------------------------------------------- state

/// One running or finished search (kept for a few searches so a source can be retried).
struct Run {
    id: u64,
    tx: mpsc::UnboundedSender<Ev>,
    token: CancellationToken,
    dedup: Mutex<Dedup>,
    topic: String,
    category: Category,
    mode: Mode,
    intent: QueryIntent,
    /// Classifies every result and decides which ones are shown.
    gate: Mutex<Gate>,
    /// The class the program gave each shown result (the AI's scores are confined to it).
    verdicts: Mutex<HashMap<String, MatchClass>>,
    per_source: usize,
    adapters: Vec<Arc<dyn SearchSource>>,
    queries: Mutex<Vec<String>>,
    /// The results that were shown (what the AI may rank).
    hits: Mutex<Vec<Hit>>,
    counts: Mutex<HashMap<Source, usize>>,
}

impl Run {
    fn send(&self, e: Ev) {
        let _ = self.tx.send(e);
    }

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// The results of this app run's searches by id (the only things `research_add` will download),
/// bounded: when full the oldest ids go first, so the cards on screen keep resolving.
struct HitCache {
    map: HashMap<String, Hit>,
    /// Ids in insertion order, oldest first.
    order: VecDeque<String>,
    max: usize,
}

impl HitCache {
    fn new(max: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            max,
        }
    }

    /// Stores `hits`. An id already there keeps its place in the order (its hit is refreshed).
    /// Then the oldest ids are evicted until the cache fits, never one of `hits` themselves.
    fn insert(&mut self, hits: &[Hit]) {
        for h in hits {
            if self.map.insert(h.id.clone(), h.clone()).is_none() {
                self.order.push_back(h.id.clone());
            }
        }
        let keep: HashSet<&str> = hits.iter().map(|h| h.id.as_str()).collect();
        while self.map.len() > self.max {
            let Some(i) = self.order.iter().position(|id| !keep.contains(id.as_str())) else {
                break;
            };
            if let Some(id) = self.order.remove(i) {
                self.map.remove(&id);
            }
        }
    }

    fn get(&self, id: &str) -> Option<&Hit> {
        self.map.get(id)
    }
}

/// The adapters (kept between searches so their rate limiting carries over), the results of
/// the searches of this app run (bounded by `CACHE_MAX`, oldest evicted first) and the recent
/// runs.
pub struct ResearchState {
    adapters: Vec<Arc<dyn SearchSource>>,
    cache: Mutex<HitCache>,
    runs: Mutex<Vec<Arc<Run>>>,
    next_id: AtomicU64,
}

impl Default for ResearchState {
    fn default() -> Self {
        Self {
            adapters: Source::ALL
                .iter()
                .filter_map(|s| default_source(*s))
                .collect(),
            cache: Mutex::new(HitCache::new(CACHE_MAX)),
            runs: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }
}

/// Identifiers of a hit for the "already downloaded" lookup.
fn seen_query(h: &Hit) -> SeenQuery {
    SeenQuery {
        url: Some(h.download_url.clone()),
        doi: h.doi.clone(),
        arxiv: h.arxiv.clone(),
        pmid: h.pmid.clone(),
        pmcid: h.pmcid.clone(),
        filename: h.filename.clone().filter(|_| h.size.is_some()),
        size: h.size,
        sha256: None,
    }
}

/// The history titles for a prompt, only when the user allowed it.
async fn history_for_ai(ai: &AiService, engine: &Engine) -> Option<Vec<String>> {
    if !ai.settings().use_download_history {
        return None;
    }
    engine.seen_titles(MAX_HISTORY_TITLES).await.ok()
}

/// The adapters that take part in a search for `category` under `mode` (the category's sources in
/// priority order, cut to the mode's source budget), plus the ones that would but are not set up.
fn pick_adapters(
    state: &ResearchState,
    ai: &AiService,
    engine: &Engine,
    mode: Mode,
    category: Category,
    only: Option<&[Source]>,
) -> (Vec<Arc<dyn SearchSource>>, Vec<Unavailable>) {
    let mut out: Vec<Arc<dyn SearchSource>> = Vec::new();
    let mut missing = Vec::new();
    let mut need_setup = |source| {
        missing.push(Unavailable {
            source,
            reason: "needs_setup",
        });
    };
    for source in mode.config().pick_sources(category, only) {
        match source {
            Source::Jamendo => match ai.named_secret("jamendo") {
                Some(id) => out.push(Arc::new(Jamendo::new(id.expose()))),
                None => need_setup(source),
            },
            Source::Web => match web_backend(ai, &read_settings(ai)) {
                Some(b) => out.push(Arc::new(WebSearch::new(b))),
                None => need_setup(source),
            },
            // video search runs the bundled yt-dlp (the same copy the media downloads use)
            Source::Youtube => match engine.media_tools().filter(|t| t.is_available()) {
                Some(tool) => out.push(Arc::new(Youtube::new(Box::new(ProcessRunner::new(
                    tool.exe(),
                ))))),
                None => need_setup(source),
            },
            other => {
                if let Some(a) = state.adapters.iter().find(|a| a.source() == other) {
                    out.push(a.clone());
                }
            }
        }
    }
    (out, missing)
}

/// Up to `n` hits taken round-robin across sources, so one chatty source cannot crowd the
/// others out of the ranking.
fn rank_input(hits: &[Hit], n: usize) -> Vec<Hit> {
    let mut by_source: Vec<(Source, Vec<&Hit>)> = Vec::new();
    for h in hits {
        match by_source.iter_mut().find(|(s, _)| *s == h.source) {
            Some((_, v)) => v.push(h),
            None => by_source.push((h.source, vec![h])),
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < n {
        let mut any = false;
        for (_, v) in &by_source {
            if let Some(h) = v.get(i) {
                out.push((*h).clone());
                any = true;
                if out.len() >= n {
                    break;
                }
            }
        }
        if !any {
            break;
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- the pipeline

/// Gathers events for 100 ms, adds the "already downloaded" matches and sends them.
async fn emitter(app: AppHandle, engine: Engine, id: u64, mut rx: mpsc::UnboundedReceiver<Ev>) {
    while let Some(first) = rx.recv().await {
        let mut events = vec![first];
        tokio::time::sleep(BATCH_WINDOW).await;
        while events.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(e) => events.push(e),
                Err(_) => break,
            }
        }
        let queries: Vec<SeenQuery> = events
            .iter()
            .filter_map(|e| match e {
                Ev::Hits { items, .. } => Some(items.iter().map(|i| seen_query(&i.hit))),
                _ => None,
            })
            .flatten()
            .collect();
        if !queries.is_empty() {
            if let Ok(found) = engine.seen_lookup(queries).await {
                let mut found = found.into_iter();
                for e in &mut events {
                    if let Ev::Hits { items, .. } = e {
                        for it in items {
                            it.seen = found.next().flatten();
                        }
                    }
                }
            }
        }
        if let Err(e) = app.emit(
            EVENT,
            &Batch {
                search_id: id,
                events,
            },
        ) {
            tracing::debug!(error = %e, "could not emit research events");
        }
    }
}

/// Sends the results the gate let through: remembers them (for adding and ranking), counts them
/// per source and reports them best class first.
fn show(run: &Run, cache: &Mutex<HitCache>, mut shown: Vec<Assessed>) {
    if shown.is_empty() {
        return;
    }
    order(&mut shown, run.mode.config().verification.prefer_official);
    let hits: Vec<Hit> = shown.iter().map(|a| a.hit.clone()).collect();
    Run::lock(cache).insert(&hits);
    Run::lock(&run.hits).extend(hits.iter().cloned());
    {
        let mut verdicts = Run::lock(&run.verdicts);
        for a in &shown {
            verdicts.insert(a.hit.id.clone(), a.assessment.class);
        }
    }
    let mut by_source: Vec<(Source, Vec<ResultItem>)> = Vec::new();
    for a in shown {
        let source = a.hit.source;
        *Run::lock(&run.counts).entry(source).or_default() += 1;
        let item = ResultItem::new(a);
        match by_source.iter_mut().find(|(s, _)| *s == source) {
            Some((_, v)) => v.push(item),
            None => by_source.push((source, vec![item])),
        }
    }
    for (source, items) in by_source {
        run.send(Ev::Hits { source, items });
    }
}

/// Runs the given queries on the run's adapters and reports everything as events.
async fn run_round(
    run: &Run,
    cache: &Mutex<HitCache>,
    adapters: &[Arc<dyn SearchSource>],
    queries: &[String],
) {
    search_stream(
        adapters,
        queries,
        run.per_source,
        run.category,
        run.mode.config().budget.deadline,
        &run.dedup,
        |ev| match ev {
            SearchEvent::Started(source) => run.send(Ev::Source {
                source,
                state: "searching",
                found: Run::lock(&run.counts).get(&source).copied().unwrap_or(0),
                code: None,
            }),
            SearchEvent::Hits { hits, .. } => {
                // only results that match the query reach the page
                let shown = Run::lock(&run.gate).offer(hits);
                show(run, cache, shown);
            }
            SearchEvent::Finished { source, .. } => run.send(Ev::Source {
                source,
                state: "done",
                found: Run::lock(&run.counts).get(&source).copied().unwrap_or(0),
                code: None,
            }),
            SearchEvent::Failed(f) => {
                let found = Run::lock(&run.counts).get(&f.source).copied().unwrap_or(0);
                // a source that already gave results in an earlier round is not "failed"
                run.send(Ev::Source {
                    source: f.source,
                    state: if found > 0 { "done" } else { "failed" },
                    found,
                    code: (found == 0).then_some(f.code),
                });
            }
        },
    )
    .await;
}

/// Shows the related results that were held back when nothing better was found.
fn release_held(run: &Run, cache: &Mutex<HitCache>) {
    let released = Run::lock(&run.gate).finish();
    show(run, cache, released);
}

/// The AI's scores confined to the class the program gave each result. Entries for results that
/// were never shown are dropped.
fn confine(run: &Run, r: Ranked) -> Option<RankedItem> {
    let class = *Run::lock(&run.verdicts).get(&r.id)?;
    Some(RankedItem {
        score: confine_score(class, r.score),
        id: r.id,
        why: r.why,
        confidence: class,
    })
}

async fn run_search(app: AppHandle, run: Arc<Run>, use_ai: bool, lang: String) {
    let ai = app.state::<AiService>();
    let engine = app.state::<Engine>();
    let state = app.state::<ResearchState>();
    let cache = &state.cache;
    let topic = run.topic.clone();
    let cfg = run.mode.config();
    let started = Instant::now();
    let mut outcome = AiOutcome {
        requested: use_ai,
        ..AiOutcome::default()
    };
    let provider = if use_ai && ai.configured() {
        match ai.provider() {
            Ok(p) => Some(p),
            Err(e) => {
                outcome.error = Some(e);
                None
            }
        }
    } else {
        None
    };
    let history = if provider.is_some() {
        history_for_ai(&ai, &engine).await
    } else {
        None
    };
    run.send(Ev::Phase { phase: "searching" });

    // the deterministic phrasings, in rounds the mode's budget allows; the first round runs while
    // the AI (if the mode asks for it) plans extra phrasings, which are searched without waiting
    // for round 1 to end
    let phrasings = queries::variants(&run.intent, run.mode);
    let per_round = phrasings.len().div_ceil(cfg.budget.max_rounds).max(1);
    let mut rounds: Vec<Vec<String>> = phrasings
        .chunks(per_round)
        .map(<[String]>::to_vec)
        .collect();
    let first = if rounds.is_empty() {
        Vec::new()
    } else {
        rounds.remove(0)
    };
    *Run::lock(&run.queries) = first.clone();
    let mut queries_run = first.len();
    let round1 = run_round(&run, cache, &run.adapters, &first);
    let plan_budget = cfg.budget.max_ai_tokens;
    let planning = async {
        if !cfg.ai.plan {
            return None;
        }
        let (p, s) = provider.as_ref()?;
        let max_tokens = (plan_budget / 3).clamp(200, 600);
        let plan_messages = assist::plan_messages_with(
            &topic,
            history.as_deref(),
            run.category,
            cfg.system_instructions,
        );
        ai.note_request("research-plan", s, &plan_messages);
        let charge = ai.charge_if_dropped(&plan_messages);
        let planned = assist::plan_queries_with(
            p.as_ref(),
            &topic,
            history.as_deref(),
            run.category,
            cfg.system_instructions,
            max_tokens,
        )
        .await;
        charge.done();
        match planned {
            Ok(plan) => {
                let known: HashSet<String> = Run::lock(&run.queries)
                    .iter()
                    .map(|q| q.to_lowercase())
                    .collect();
                let room = cfg.budget.max_queries.saturating_sub(first.len());
                let extra: Vec<String> = plan
                    .queries
                    .iter()
                    .filter(|q| !known.contains(&q.to_lowercase()))
                    .take(room)
                    .cloned()
                    .collect();
                run.send(Ev::Plan {
                    queries: plan.queries.clone(),
                });
                // enough exact matches already: do not spend more of the user's sources
                let enough = Run::lock(&run.gate).exact_count() >= cfg.budget.stop_after_exact;
                if !extra.is_empty() && !enough {
                    Run::lock(&run.queries).extend(extra.iter().cloned());
                    run_round(&run, cache, &run.adapters, &extra).await;
                }
                Some(Ok((plan.usage.total_tokens, extra.len())))
            }
            Err(e) => Some(Err(ai.fail(&e))),
        }
    };
    let (_, planned) = tokio::join!(round1, planning);
    match planned {
        Some(Ok((tokens, extra))) => {
            outcome.used = true;
            outcome.tokens += tokens;
            queries_run += extra;
            ai.note_usage(tokens);
        }
        Some(Err(f)) => outcome.error = Some(f),
        None => {}
    }

    // later rounds, until a stop condition of the mode says enough
    let mut rounds_done = 1;
    let mut stop = None;
    for round in rounds {
        let reason = cfg.stop_reason(&RoundState {
            rounds_done,
            exact_found: Run::lock(&run.gate).exact_count(),
            elapsed: started.elapsed(),
            ai_tokens: u32::try_from(outcome.tokens).unwrap_or(u32::MAX),
            queries_left: round.len(),
        });
        if reason.is_some() {
            stop = reason;
            break;
        }
        Run::lock(&run.queries).extend(round.iter().cloned());
        queries_run += round.len();
        run_round(&run, cache, &run.adapters, &round).await;
        rounds_done += 1;
    }
    tracing::debug!(queries_run, rounds_done, "research rounds finished");
    release_held(&run, cache);

    // the comparison across sources (deep research, source finder)
    let verification = cfg.verification;
    if verification.compare_sources {
        let dups = Run::lock(&run.dedup).drain_duplicates();
        let comparison = compare(&Run::lock(&run.hits), &dups);
        let comparison = if verification.note_contradictions {
            comparison
        } else {
            Comparison {
                notes: Vec::new(),
                ..comparison
            }
        };
        if !comparison.also_in.is_empty() || !comparison.notes.is_empty() {
            run.send(Ev::Compare { comparison });
        }
    }
    {
        let gate = Run::lock(&run.gate);
        run.send(Ev::Summary {
            exact: gate.exact_count(),
            strong: gate.strong_count(),
            hidden: gate.dropped_count(),
            stop,
        });
    }

    // ranking streams in after all the results are in; the model only reorders inside a class
    if let (Some((p, s)), true) = (&provider, cfg.ai.rank) {
        let list = rank_input(&Run::lock(&run.hits), MAX_RANK_INPUT);
        let left = cfg
            .budget
            .max_ai_tokens
            .saturating_sub(u32::try_from(outcome.tokens).unwrap_or(u32::MAX));
        if list.len() >= 2 && left >= 300 {
            run.send(Ev::Phase { phase: "ranking" });
            let rank_messages = assist::rank_messages_with(
                &topic,
                &list,
                history.as_deref(),
                &lang,
                cfg.system_instructions,
            );
            ai.note_request("research-rank", s, &rank_messages);
            outcome.used = true;
            let charge = ai.charge_if_dropped(&rank_messages);
            let tx = run.tx.clone();
            let runref = run.clone();
            let mut sink = move |r: Ranked| {
                if let Some(item) = confine(&runref, r) {
                    let _ = tx.send(Ev::Rank { items: vec![item] });
                }
            };
            let done = assist::rank_stream_with(
                p.as_ref(),
                &topic,
                &list,
                history.as_deref(),
                &lang,
                cfg.system_instructions,
                left.min(1800),
                &mut sink,
            )
            .await;
            charge.done();
            outcome.tokens += done.usage.total_tokens;
            outcome.dropped = done.dropped;
            ai.note_usage(done.usage.total_tokens);
            if let Some(e) = done.error {
                outcome.error = Some(ai.fail(&e));
            }
        }
    }
    run.send(Ev::Ai { outcome });
}

async fn drive(app: AppHandle, run: Arc<Run>, use_ai: bool, lang: String) {
    tokio::select! {
        () = run.token.cancelled() => run.send(Ev::Phase { phase: "cancelled" }),
        () = run_search(app, run.clone(), use_ai, lang) => run.send(Ev::Phase { phase: "done" }),
    }
    run.send(Ev::End);
}

/// Starts a search and returns at once; the results stream in on [`EVENT`].
#[tauri::command]
pub async fn research_start(
    app: AppHandle,
    ai: State<'_, AiService>,
    engine: State<'_, Engine>,
    state: State<'_, ResearchState>,
    request: SearchRequest,
) -> Result<StartResponse, String> {
    let topic = request
        .topic
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if topic.is_empty() {
        return Err("type a topic first".into());
    }
    // no silent "all": the user names what they are looking for
    let category = request.category.canonical().ok_or(
        "choose a category: videos, software, documents, images, academic, open-source or web",
    )?;
    let mode = request.mode;
    if !mode.config().accepts(category) {
        return Err(format!("{} only searches videos", mode.id()));
    }
    let (adapters, unavailable) = pick_adapters(
        &state,
        &ai,
        &engine,
        mode,
        category,
        request.sources.as_deref().filter(|s| !s.is_empty()),
    );
    let use_ai = request.use_ai && ai.configured();
    let id = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::unbounded_channel();
    let intent = QueryIntent::parse(&topic);
    let cfg = mode.config();
    let run = Arc::new(Run {
        id,
        tx,
        token: CancellationToken::new(),
        dedup: Mutex::new(Dedup::new()),
        topic,
        category,
        mode,
        gate: Mutex::new(Gate::new(intent.clone(), mode)),
        intent,
        verdicts: Mutex::new(HashMap::new()),
        per_source: request
            .per_source
            .unwrap_or(cfg.per_source_cap)
            .clamp(1, cfg.per_source_cap),
        adapters,
        queries: Mutex::new(Vec::new()),
        hits: Mutex::new(Vec::new()),
        counts: Mutex::new(HashMap::new()),
    });
    {
        let mut runs = Run::lock(&state.runs);
        for old in runs.iter() {
            old.token.cancel();
        }
        runs.push(run.clone());
        let excess = runs.len().saturating_sub(KEEP_RUNS);
        runs.drain(..excess);
    }
    tauri::async_runtime::spawn(emitter(app.clone(), engine.inner().clone(), id, rx));
    let response = StartResponse {
        search_id: id,
        sources: run.adapters.iter().map(|a| a.source()).collect(),
        unavailable,
        ai: use_ai,
        mode,
        category,
        intent: run.intent.clone(),
        queries: queries::variants(&run.intent, mode),
    };
    if run.adapters.is_empty() {
        run.send(Ev::Phase { phase: "done" });
        run.send(Ev::End);
    } else {
        tauri::async_runtime::spawn(drive(app, run, use_ai, request.lang));
    }
    Ok(response)
}

fn find_run(state: &ResearchState, id: u64) -> Option<Arc<Run>> {
    Run::lock(&state.runs).iter().find(|r| r.id == id).cloned()
}

/// Stops a search: every request in flight is dropped, including the AI stream.
#[tauri::command]
pub fn research_cancel(state: State<'_, ResearchState>, search_id: u64) {
    if let Some(run) = find_run(&state, search_id) {
        run.token.cancel();
    }
}

/// Searches one source again (after it failed or timed out); new results join the page.
#[tauri::command]
pub fn research_retry_source(
    app: AppHandle,
    state: State<'_, ResearchState>,
    search_id: u64,
    source: Source,
) -> Result<(), String> {
    let run = find_run(&state, search_id).ok_or("that search is no longer available")?;
    let adapter = run
        .adapters
        .iter()
        .find(|a| a.source() == source)
        .cloned()
        .ok_or("that source is not part of this search")?;
    let queries = Run::lock(&run.queries).clone();
    let token = run.token.child_token();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<ResearchState>();
        let adapters = [adapter];
        tokio::select! {
            () = token.cancelled() => {}
            () = run_round(&run, &state.cache, &adapters, &queries) => release_held(&run, &state.cache),
        }
    });
    Ok(())
}

/// The messages the planning request would send for `topic`, without sending anything.
#[tauri::command]
pub async fn research_preview(
    ai: State<'_, AiService>,
    engine: State<'_, Engine>,
    topic: String,
    category: Option<Category>,
    mode: Option<Mode>,
) -> Result<Vec<Message>, String> {
    let history = history_for_ai(&ai, &engine).await;
    Ok(assist::plan_messages_with(
        &topic.split_whitespace().collect::<Vec<_>>().join(" "),
        history.as_deref(),
        category.unwrap_or(Category::All),
        mode.unwrap_or_default().config().system_instructions,
    ))
}

// ---------------------------------------------------------------- adding

/// Results to add.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    /// Hit ids from a search made during this app run.
    pub ids: Vec<String>,
    /// Queue to put them in; the Main queue when absent.
    #[serde(default)]
    pub queue_id: Option<i64>,
    /// Category key (`documents`, `compressed`, ...) whose folder to use; automatic when absent.
    #[serde(default)]
    pub category: Option<String>,
    /// Add as paused ("start later").
    #[serde(default)]
    pub start_paused: bool,
    /// Add even when it was downloaded before and "skip duplicates" is on.
    #[serde(default)]
    pub allow_duplicates: bool,
}

/// One id that was not added.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSkip {
    /// The hit id.
    pub id: String,
    /// `unknown` (not fetched by a search of this app run), `page` (a web page, not a file),
    /// `already_downloaded`, `private_address` (the file is on a private network) or `failed`.
    pub reason: &'static str,
}

/// What was added.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddResponse {
    /// Ids of the new downloads.
    pub added: Vec<i64>,
    /// What was not added.
    pub skipped: Vec<AddSkip>,
}

/// Whether a hit points at a private network address that the source itself does not account
/// for. The address of a result comes from a third party (or from a model-ranked list of them),
/// so a click must not turn into a request to `127.0.0.1`, the router or a cloud metadata
/// service. The one exception is a result of the user's own SearXNG when that instance is itself
/// on a private address (the owner searching their LAN).
pub async fn aims_at_private_network(engine: &Engine, hit: &Hit, searxng_is_private: bool) -> bool {
    if hit.source == riptide_research::Source::Web && searxng_is_private {
        return false;
    }
    engine.is_private_target(&hit.download_url).await
}

/// Adds results from any search of this app run as normal downloads (only ids the backend
/// itself fetched are accepted).
#[tauri::command]
pub async fn research_add(
    engine: State<'_, Engine>,
    state: State<'_, ResearchState>,
    ai: State<'_, AiService>,
    request: AddRequest,
) -> Result<AddResponse, String> {
    let searxng_is_private = match read_settings(&ai) {
        ResearchSettings {
            web: WebChoice::Searxng,
            searxng_url: Some(u),
            ..
        } => engine.is_private_target(&u).await,
        _ => false,
    };
    let dir: Option<PathBuf> = request
        .category
        .as_deref()
        .and_then(FileCategory::from_key)
        .map(|c| engine.settings().dir_for(c));
    let hits: Vec<(String, Option<Hit>)> = {
        let cache = Run::lock(&state.cache);
        request
            .ids
            .iter()
            .map(|id| (id.clone(), cache.get(id).cloned()))
            .collect()
    };
    let mut out = AddResponse {
        added: Vec::new(),
        skipped: Vec::new(),
    };
    for (id, hit) in hits {
        let Some(hit) = hit else {
            out.skipped.push(AddSkip {
                id,
                reason: "unknown",
            });
            continue;
        };
        // a video page found by the video search is handed to yt-dlp (the media flow); any
        // other page is not a file
        let is_video_page = hit.source == Source::Youtube && hit.kind == HitKind::Page;
        if (hit.kind == HitKind::Page && !is_video_page)
            || !riptide_core::batch::is_http_url(&hit.download_url)
        {
            out.skipped.push(AddSkip { id, reason: "page" });
            continue;
        }
        if aims_at_private_network(&engine, &hit, searxng_is_private).await {
            out.skipped.push(AddSkip {
                id,
                reason: "private_address",
            });
            continue;
        }
        let req = DownloadRequest {
            url: hit.download_url.clone(),
            filename: hit.filename.as_deref().map(riptide_core::sanitize_filename),
            dir: dir.clone(),
            queue_id: request.queue_id.map(QueueId),
            start_paused: request.start_paused,
            referrer: (!is_video_page).then(|| hit.page_url.clone()).flatten(),
            kind: if is_video_page {
                DownloadKind::Media
            } else {
                DownloadKind::Http
            },
            ..DownloadRequest::new(hit.download_url.clone())
        };
        let seen_hint = SeenHint {
            title: Some(hit.title.clone()),
            doi: hit.doi.clone(),
            arxiv: hit.arxiv.clone(),
            pmid: hit.pmid.clone(),
            pmcid: hit.pmcid.clone(),
        };
        match engine
            .add_hinted(req, seen_hint, request.allow_duplicates)
            .await
        {
            Ok(new) => out.added.push(new.0),
            Err(EngineError::AlreadyDownloaded(_)) if !request.allow_duplicates => {
                out.skipped.push(AddSkip {
                    id,
                    reason: "already_downloaded",
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "could not add a research result");
                out.skipped.push(AddSkip {
                    id,
                    reason: "failed",
                });
            }
        }
    }
    Ok(out)
}

/// "Already downloaded?" for any list of things (Add dialog, batch dialog, research cards).
#[tauri::command]
pub async fn seen_check(
    engine: State<'_, Engine>,
    queries: Vec<SeenQuery>,
) -> Result<Vec<Option<SeenMatch>>, String> {
    if queries.len() > 2000 {
        return Err("too many items".into());
    }
    engine.seen_lookup(queries).await.map_err(|e| e.to_string())
}

/// Whether adding something already downloaded is refused.
#[tauri::command]
pub async fn seen_settings(engine: State<'_, Engine>) -> Result<bool, String> {
    Ok(engine.skip_duplicates().await)
}

/// Turns "skip duplicates" on or off.
#[tauri::command]
pub async fn seen_set_skip(engine: State<'_, Engine>, on: bool) -> Result<(), String> {
    engine
        .set_skip_duplicates(on)
        .await
        .map_err(|e| e.to_string())
}

/// Forgets the "already downloaded" memory (files and the download list are untouched).
#[tauri::command]
pub async fn seen_forget(engine: State<'_, Engine>) -> Result<usize, String> {
    engine.seen_forget().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use riptide_research::relevance::assess;

    fn hit(src: Source, n: usize) -> Hit {
        Hit::new(
            src,
            &n.to_string(),
            &format!("t{n}"),
            &format!("https://x.example/{}/{n}", src.id()),
        )
    }

    #[test]
    fn cache_evicts_the_oldest_first_and_keeps_the_new_hits() {
        let mut c = HitCache::new(5);
        let batch =
            |r: std::ops::Range<usize>| -> Vec<Hit> { r.map(|n| hit(Source::Arxiv, n)).collect() };
        c.insert(&batch(0..4));
        assert_eq!(c.map.len(), 4);
        c.insert(&batch(4..7));
        assert_eq!(c.map.len(), 5);
        let ids = |r: std::ops::Range<usize>| -> Vec<String> {
            r.map(|n| hit(Source::Arxiv, n).id).collect()
        };
        assert!(c.get(&ids(0..1)[0]).is_none());
        assert!(c.get(&ids(1..2)[0]).is_none());
        for id in ids(2..7) {
            assert!(c.get(&id).is_some(), "{id}");
        }
        assert_eq!(c.order.len(), c.map.len());
    }

    #[test]
    fn cache_reinsert_keeps_the_place_and_never_grows() {
        let mut c = HitCache::new(3);
        c.insert(&[hit(Source::Arxiv, 0), hit(Source::Arxiv, 1)]);
        c.insert(&[hit(Source::Arxiv, 0)]);
        assert_eq!((c.map.len(), c.order.len()), (2, 2));
        // 0 is still the oldest, so it goes first
        c.insert(&[hit(Source::Arxiv, 2), hit(Source::Arxiv, 3)]);
        assert!(c.get(&hit(Source::Arxiv, 0).id).is_none());
        assert!(c.get(&hit(Source::Arxiv, 1).id).is_some());
        assert_eq!(c.map.len(), 3);
    }

    #[test]
    fn cache_never_evicts_a_batch_larger_than_its_capacity() {
        let mut c = HitCache::new(2);
        c.insert(&[hit(Source::Arxiv, 0)]);
        let big: Vec<Hit> = (1..5).map(|n| hit(Source::Arxiv, n)).collect();
        c.insert(&big);
        assert!(c.get(&hit(Source::Arxiv, 0).id).is_none());
        for h in &big {
            assert!(c.get(&h.id).is_some());
        }
        // the next small batch trims it back down
        c.insert(&[hit(Source::Arxiv, 9)]);
        assert_eq!(c.map.len(), 2);
        assert!(c.get(&hit(Source::Arxiv, 9).id).is_some());
    }

    #[test]
    fn rank_input_takes_sources_in_turn() {
        let mut hits = Vec::new();
        for n in 0..6 {
            hits.push(hit(Source::Arxiv, n));
        }
        hits.push(hit(Source::Github, 0));
        hits.push(hit(Source::Nasa, 0));
        let picked = rank_input(&hits, 5);
        let srcs: Vec<Source> = picked.iter().map(|h| h.source).collect();
        assert_eq!(
            srcs,
            [
                Source::Arxiv,
                Source::Github,
                Source::Nasa,
                Source::Arxiv,
                Source::Arxiv
            ]
        );
        assert_eq!(rank_input(&hits, 100).len(), 8);
        assert!(rank_input(&[], 5).is_empty());
    }

    #[test]
    fn settings_serialise_without_secrets_and_default_to_no_web_search() {
        let s = ResearchSettings::default();
        assert_eq!(s.web, WebChoice::None);
        let json = serde_json::to_string(&ResearchSettings {
            web: WebChoice::Searxng,
            searxng_url: Some("https://searx.example".into()),
        })
        .unwrap();
        assert!(json.contains("\"web\":\"searxng\""), "{json}");
        let back: ResearchSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.web, WebChoice::Searxng);
    }

    #[test]
    fn events_are_tagged_for_the_webview() {
        let e = Ev::Source {
            source: Source::LibriVox,
            state: "done",
            found: 3,
            code: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["t"], "source");
        assert_eq!(v["source"], "libri-vox");
        assert_eq!(v["found"], 3);
        let h = hit(Source::Arxiv, 1);
        let item = ResultItem::new(Assessed {
            assessment: assess(&QueryIntent::parse("t1"), &h),
            hit: h,
        });
        let v = serde_json::to_value(&Ev::Hits {
            source: Source::Arxiv,
            items: vec![item],
        })
        .unwrap();
        assert_eq!(v["t"], "hits");
        assert_eq!(v["items"][0]["rights"]["kind"], "unknown");
        assert_eq!(v["items"][0]["kind"], "file");
        // the verdict travels with the result
        assert_eq!(v["items"][0]["confidence"], "exact");
        assert!(v["items"][0]["reasons"].is_array());
        assert!(v["items"][0]["matchScore"].is_u64());
        assert_eq!(v["items"][0]["trust"], "primary");
        // fields only present when a source returned them
        assert!(v["items"][0]["durationSecs"].is_null());
        assert!(v["items"][0]["thumbnail"].is_null());
    }

    #[test]
    fn new_events_have_the_documented_shape() {
        let v = serde_json::to_value(Ev::Summary {
            exact: 2,
            strong: 1,
            hidden: 9,
            stop: Some(StopReason::EnoughExact),
        })
        .unwrap();
        assert_eq!(v["t"], "summary");
        assert_eq!(v["hidden"], 9);
        assert_eq!(v["stop"], "enough-exact");
        let v = serde_json::to_value(Ev::Rank {
            items: vec![RankedItem {
                id: "x".into(),
                score: 80,
                why: "w".into(),
                confidence: MatchClass::Strong,
            }],
        })
        .unwrap();
        assert_eq!(v["items"][0]["confidence"], "strong");
        let v = serde_json::to_value(Ev::Compare {
            comparison: Comparison::default(),
        })
        .unwrap();
        assert_eq!(v["t"], "compare");
    }

    #[test]
    fn a_search_must_name_its_category_and_old_ids_still_work() {
        // no silent "all": a request without a category does not even parse
        assert!(serde_json::from_str::<SearchRequest>(r#"{"topic":"x"}"#).is_err());
        let req = |c: &str| -> SearchRequest {
            serde_json::from_str(&format!(r#"{{"topic":"x","category":"{c}"}}"#)).unwrap()
        };
        assert_eq!(req("videos").mode, Mode::QuickSearch);
        // stored history ids map to the seven
        assert_eq!(req("video").category.canonical(), Some(Category::Video));
        assert_eq!(req("papers").category.canonical(), Some(Category::Academic));
        assert_eq!(req("books").category.canonical(), Some(Category::Documents));
        // the ones with no home are refused by research_start
        for c in ["all", "music", "datasets"] {
            assert_eq!(req(c).category.canonical(), None, "{c}");
        }
        let r: SearchRequest =
            serde_json::from_str(r#"{"topic":"x","category":"web","mode":"deep-research"}"#)
                .unwrap();
        assert_eq!(r.mode, Mode::DeepResearch);
    }
}
