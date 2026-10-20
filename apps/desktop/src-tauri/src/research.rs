//! The Research page's backend: open-access search (with optional AI help), the "already
//! downloaded" lookups and adding results to the download list.
//!
//! | Command | Arguments | Answer |
//! |---|---|---|
//! | `research_search` | `request: SearchRequest` | [`SearchResponse`] |
//! | `research_preview` | `topic` | the exact messages the planning request would send |
//! | `research_add` | `request: AddRequest` | [`AddResponse`] |
//! | `seen_check` | `queries: [SeenQuery]` | one [`riptide_engine::SeenMatch`] or `null` per query |
//! | `seen_settings` / `seen_set_skip` / `seen_forget` | - / `on` / - | skip flag / - / records removed |
//!
//! The model never sees a URL: ranking works on ids of results the adapters returned, and the
//! answer is validated against them (see `riptide_research::assist`). Download-history titles
//! are included in prompts only while *Let the AI see my download history* is on.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use riptide_ai::Message;
use riptide_core::{Category, DownloadRequest, QueueId};
use riptide_engine::{Engine, Error as EngineError, SeenHint, SeenMatch, SeenQuery};
use riptide_research::assist::{self, MAX_HISTORY_TITLES};
use riptide_research::{default_source, search_all, Hit, SearchSource, Source, SourceFailure};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::{AiFail, AiService};

/// Most results kept in memory for `research_add`.
const CACHE_MAX: usize = 400;

/// The adapters (kept between searches so their rate limiting carries over) and the hits of
/// the latest searches, which are the only things `research_add` will download.
pub struct ResearchState {
    adapters: Vec<Arc<dyn SearchSource>>,
    cache: Mutex<HashMap<String, Hit>>,
}

impl Default for ResearchState {
    fn default() -> Self {
        Self {
            adapters: Source::ALL.iter().map(|s| default_source(*s)).collect(),
            cache: Mutex::new(HashMap::new()),
        }
    }
}

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
    /// Restrict to these sources; all when absent.
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
    /// 0-100 from the AI, when it ranked this one.
    pub score: Option<u8>,
    /// The AI's one-line reason (language of the UI), when it ranked this one.
    pub why: Option<String>,
    /// Already downloaded?
    pub seen: Option<SeenMatch>,
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

/// The answer to a search.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// The query phrasings that were searched.
    pub queries: Vec<String>,
    /// Ranked results first, the rest in source order.
    pub items: Vec<ResultItem>,
    /// Sources that did not answer.
    pub failures: Vec<SourceFailure>,
    /// What the AI did.
    pub ai: AiOutcome,
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

/// Runs a search. Never fails because of the AI: a failed AI step is reported in `ai.error` and
/// the keyword results are returned anyway.
#[tauri::command]
pub async fn research_search(
    ai: State<'_, AiService>,
    engine: State<'_, Engine>,
    state: State<'_, ResearchState>,
    request: SearchRequest,
) -> Result<SearchResponse, String> {
    let topic = request
        .topic
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if topic.is_empty() {
        return Err("type a topic first".into());
    }
    let mut outcome = AiOutcome {
        requested: request.use_ai,
        ..AiOutcome::default()
    };
    let provider = if request.use_ai && ai.configured() {
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

    // 1. phrasings
    let mut queries = vec![topic.clone()];
    if let Some((p, s)) = &provider {
        ai.note_request(
            "research-plan",
            s,
            &assist::plan_messages(&topic, history.as_deref()),
        );
        match assist::plan_queries(p.as_ref(), &topic, history.as_deref()).await {
            Ok(plan) => {
                outcome.used = true;
                outcome.tokens += plan.usage.total_tokens;
                ai.note_usage(plan.usage.total_tokens);
                queries = plan.queries;
            }
            Err(e) => outcome.error = Some(ai.fail(&e)),
        }
    }

    // 2. the adapters
    let adapters: Vec<Arc<dyn SearchSource>> = match &request.sources {
        Some(wanted) if !wanted.is_empty() => state
            .adapters
            .iter()
            .filter(|a| wanted.contains(&a.source()))
            .cloned()
            .collect(),
        _ => state.adapters.clone(),
    };
    let per_source = request.per_source.unwrap_or(6).clamp(1, 20);
    let found = search_all(&adapters, &queries, per_source).await;

    // 3. rank
    let mut order: Vec<(String, u8, String)> = Vec::new();
    if let (Some((p, s)), true) = (&provider, outcome.used && !found.hits.is_empty()) {
        ai.note_request(
            "research-rank",
            s,
            &assist::rank_messages(&topic, &found.hits, history.as_deref(), &request.lang),
        );
        match assist::rank(
            p.as_ref(),
            &topic,
            &found.hits,
            history.as_deref(),
            &request.lang,
        )
        .await
        {
            Ok(r) => {
                outcome.tokens += r.usage.total_tokens;
                outcome.dropped = r.dropped;
                ai.note_usage(r.usage.total_tokens);
                order = r
                    .ranked
                    .into_iter()
                    .map(|x| (x.id, x.score, x.why))
                    .collect();
            }
            Err(e) => outcome.error = Some(ai.fail(&e)),
        }
    }

    // 4. order, remember, look up what is already downloaded
    let mut by_id: HashMap<String, Hit> = found
        .hits
        .iter()
        .map(|h| (h.id.clone(), h.clone()))
        .collect();
    let mut items: Vec<ResultItem> = Vec::with_capacity(found.hits.len());
    for (id, score, why) in order {
        if let Some(hit) = by_id.remove(&id) {
            items.push(ResultItem {
                hit,
                score: Some(score),
                why: Some(why).filter(|w| !w.is_empty()),
                seen: None,
            });
        }
    }
    for hit in &found.hits {
        if let Some(hit) = by_id.remove(&hit.id) {
            items.push(ResultItem {
                hit,
                score: None,
                why: None,
                seen: None,
            });
        }
    }
    {
        let mut cache = state.cache.lock().unwrap_or_else(|p| p.into_inner());
        if cache.len() + items.len() > CACHE_MAX {
            cache.clear();
        }
        for it in &items {
            cache.insert(it.hit.id.clone(), it.hit.clone());
        }
    }
    let seen = engine
        .seen_lookup(items.iter().map(|i| seen_query(&i.hit)).collect())
        .await
        .map_err(|e| e.to_string())?;
    for (item, s) in items.iter_mut().zip(seen) {
        item.seen = s;
    }
    Ok(SearchResponse {
        queries: found.queries,
        items,
        failures: found.failures,
        ai: outcome,
    })
}

/// The messages the planning request would send for `topic`, without sending anything.
#[tauri::command]
pub async fn research_preview(
    ai: State<'_, AiService>,
    engine: State<'_, Engine>,
    topic: String,
) -> Result<Vec<Message>, String> {
    let history = history_for_ai(&ai, &engine).await;
    Ok(assist::plan_messages(
        &topic.split_whitespace().collect::<Vec<_>>().join(" "),
        history.as_deref(),
    ))
}

/// Results to add.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    /// Hit ids from the latest searches.
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
    /// `unknown` (not from a recent search), `already_downloaded` or `failed`.
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

/// Adds results from the latest searches as normal downloads.
#[tauri::command]
pub async fn research_add(
    engine: State<'_, Engine>,
    state: State<'_, ResearchState>,
    request: AddRequest,
) -> Result<AddResponse, String> {
    let dir: Option<PathBuf> = request
        .category
        .as_deref()
        .and_then(Category::from_key)
        .map(|c| engine.settings().dir_for(c));
    let hits: Vec<(String, Option<Hit>)> = {
        let cache = state.cache.lock().unwrap_or_else(|p| p.into_inner());
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
        let req = DownloadRequest {
            url: hit.download_url.clone(),
            filename: hit.filename.as_deref().map(riptide_core::sanitize_filename),
            dir: dir.clone(),
            queue_id: request.queue_id.map(QueueId),
            start_paused: request.start_paused,
            referrer: hit.page_url.clone(),
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
