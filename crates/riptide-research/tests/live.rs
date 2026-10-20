//! Live checks against the real services. All `#[ignore]`d: run with
//! `cargo test -p riptide-research --test live -- --ignored --test-threads=1 --nocapture`.
//!
//! The AI test needs `RIPTIDE_AI_KEY_FILE` (path of a file holding a DeepInfra key; the key is
//! read into memory and never printed) and optionally `RIPTIDE_AI_MODEL`.
#![allow(clippy::unwrap_used)]

use riptide_ai::{ApiKey, ProviderKind, ProviderOptions};
use riptide_research::assist;
use riptide_research::sources::{
    arxiv::Arxiv, commons::Commons, crossref::Crossref, datagov::DataGov, europepmc::EuropePmc,
    github::Github, internet_archive::InternetArchive, openalex::OpenAlex,
};
use riptide_research::{search_all, Hit, SearchSource};

async fn check(adapter: impl SearchSource, query: &str) -> Vec<Hit> {
    let start = std::time::Instant::now();
    let hits = adapter.search(query, 5).await.unwrap_or_else(|e| {
        panic!("{} failed: {e}", adapter.source().label());
    });
    println!(
        "LIVE {:<18} {} hit(s) in {} ms; first: {}",
        adapter.source().label(),
        hits.len(),
        start.elapsed().as_millis(),
        hits.first()
            .map(|h| format!("{} -> {}", h.title, h.download_url))
            .unwrap_or_default()
    );
    assert!(
        !hits.is_empty(),
        "{} returned nothing",
        adapter.source().label()
    );
    for h in &hits {
        assert!(h.download_url.starts_with("http"), "{h:?}");
        assert!(!h.title.is_empty());
    }
    hits
}

#[tokio::test]
#[ignore = "live network"]
async fn openalex() {
    check(OpenAlex::new(), "bittorrent piece selection").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn crossref() {
    check(Crossref::new(), "bittorrent").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn arxiv() {
    check(Arxiv::new(), "bittorrent piece selection").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn europepmc() {
    check(EuropePmc::new(), "peer-to-peer network").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn internet_archive() {
    check(InternetArchive::new(), "public domain history").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn commons() {
    check(Commons::new(), "bittorrent logo").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn github() {
    check(Github::new(), "bittorrent client").await;
}

#[tokio::test]
#[ignore = "live network"]
async fn datagov() {
    check(DataGov::new(), "climate").await;
}

/// End to end with a real provider: connection test, query planning, real adapters, ranking
/// with citation validation, and a file-name suggestion. Prints counts and timings only.
#[tokio::test]
#[ignore = "live network + API key file"]
async fn ai_end_to_end_with_deepinfra() {
    let key_path = std::env::var("RIPTIDE_AI_KEY_FILE").expect("RIPTIDE_AI_KEY_FILE");
    let key = ApiKey::new(std::fs::read_to_string(key_path).unwrap()).unwrap();
    let model =
        std::env::var("RIPTIDE_AI_MODEL").unwrap_or_else(|_| "google/gemma-4-26B-A4B-it".into());
    let provider = riptide_ai::build_provider(
        ProviderKind::DeepInfra,
        key.clone(),
        &model,
        None,
        ProviderOptions::default(),
    )
    .unwrap();

    // 1. connection
    let report = provider.test_connection().await.unwrap();
    println!(
        "LIVE ai: connection ok, {} models, {} ms",
        report.model_count,
        report.latency.as_millis()
    );
    let models = provider.list_models().await.unwrap();
    let gemma4: Vec<&str> = models
        .iter()
        .map(|m| m.id.as_str())
        .filter(|id| id.to_lowercase().contains("gemma-4") || id.to_lowercase().contains("gemma4"))
        .collect();
    println!("LIVE ai: gemma-4 models listed by the provider: {gemma4:?}");

    // 2. plan
    let topic = "open access papers about BitTorrent piece selection";
    let plan = assist::plan_queries(provider.as_ref(), topic, None)
        .await
        .unwrap();
    println!(
        "LIVE ai: plan = {:?} ({} tokens)",
        plan.queries, plan.usage.total_tokens
    );
    assert!(!plan.queries.is_empty());

    // 3. real adapters
    let adapters: Vec<std::sync::Arc<dyn SearchSource>> = vec![
        std::sync::Arc::new(OpenAlex::new()),
        std::sync::Arc::new(Arxiv::new()),
        std::sync::Arc::new(EuropePmc::new()),
    ];
    let out = search_all(&adapters, &plan.queries, 6).await;
    println!(
        "LIVE ai: {} merged hits, failures: {:?}",
        out.hits.len(),
        out.failures
            .iter()
            .map(|f| (f.source.label(), f.code.clone()))
            .collect::<Vec<_>>()
    );
    assert!(!out.hits.is_empty());

    // 4. rank + citation validation
    let ranked = assist::rank(provider.as_ref(), topic, &out.hits, None, "en")
        .await
        .unwrap();
    println!(
        "LIVE ai: ranked {} entries, dropped {} by validation, {} tokens",
        ranked.ranked.len(),
        ranked.dropped,
        ranked.usage.total_tokens
    );
    assert!(!ranked.ranked.is_empty());
    for r in &ranked.ranked {
        let hit = out
            .hits
            .iter()
            .find(|h| h.id == r.id)
            .expect("ranked id is a real hit");
        println!(
            "LIVE ai:   {:>3} {} | {}",
            r.score,
            hit.title.chars().take(60).collect::<String>(),
            r.why
        );
        assert!(!r.why.contains("http"));
    }

    // 5. Arabic reasons (UI language)
    let ar = assist::rank(
        provider.as_ref(),
        topic,
        &out.hits[..out.hits.len().min(5)],
        None,
        "ar",
    )
    .await
    .unwrap();
    println!("LIVE ai: arabic ranking entries = {}", ar.ranked.len());

    // 6. name suggestion
    let s = assist::suggest_name(
        provider.as_ref(),
        "cdn.example.org",
        "report_final_v2_(1)_9f8e7d6c.pdf",
        Some(1_234_567),
    )
    .await
    .unwrap();
    println!(
        "LIVE ai: name suggestion = {:?} category {:?}",
        s.filename, s.category
    );
    assert!(s.filename.ends_with(".pdf"));
}
