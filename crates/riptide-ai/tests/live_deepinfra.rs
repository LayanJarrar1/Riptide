//! Live smoke test against DeepInfra. Ignored by default; costs a fraction of a cent.
//!
//! Run: `cargo test -p riptide-ai --test live_deepinfra -- --ignored --nocapture`
//!
//! The key is read from `RIPTIDE_TEST_DEEPINFRA_KEY`, or from the file
//! `%USERPROFILE%\.riptide-keys\deepinfra-test.key`. It is held in memory only and never printed.
#![allow(missing_docs)]

use std::time::Instant;

use riptide_ai::{
    build_provider, ApiKey, CompletionRequest, Message, ModelInfo, ProviderKind, ProviderOptions,
};

fn load_key() -> ApiKey {
    if let Ok(v) = std::env::var("RIPTIDE_TEST_DEEPINFRA_KEY") {
        return ApiKey::new(v).expect("env key format");
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .expect("home dir");
    let path = std::path::Path::new(&home)
        .join(".riptide-keys")
        .join("deepinfra-test.key");
    let text = std::fs::read_to_string(&path).expect("read the DeepInfra test key file");
    ApiKey::new(text).expect("key file format")
}

/// (version, size in billions, is-instruct) parsed from an id like `google/gemma-3-27b-it`.
fn gemma_rank(id: &str) -> (u32, u32, bool) {
    let low = id.to_ascii_lowercase();
    let after = low.split("gemma").nth(1).unwrap_or("");
    let version = after
        .trim_start_matches(['-', '_'])
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    let size = low
        .split(['-', '_', '/'])
        .find_map(|p| p.strip_suffix('b').and_then(|n| n.parse::<u32>().ok()))
        .unwrap_or(0);
    (
        version,
        size,
        low.contains("-it") || low.contains("instruct"),
    )
}

fn pick_gemma(models: &[ModelInfo]) -> Option<(String, bool)> {
    let gemma: Vec<&ModelInfo> = models
        .iter()
        .filter(|m| m.id.to_ascii_lowercase().contains("gemma"))
        .collect();
    println!("Gemma ids available ({}):", gemma.len());
    for m in &gemma {
        println!("  {}", m.id);
    }
    let gemma4: Vec<&&ModelInfo> = gemma
        .iter()
        .filter(|m| {
            let l = m.id.to_ascii_lowercase();
            l.contains("gemma-4") || l.contains("gemma4")
        })
        .collect();
    let (pool, is_gemma4): (Vec<&ModelInfo>, bool) = if gemma4.is_empty() {
        (gemma.clone(), false)
    } else {
        (gemma4.into_iter().copied().collect(), true)
    };
    // Newest version, instruct first, then the smallest size (cheapest for a smoke test).
    pool.into_iter()
        .max_by_key(|m| {
            let (v, s, it) = gemma_rank(&m.id);
            (v, it, std::cmp::Reverse(s.max(1)))
        })
        .map(|m| (m.id.clone(), is_gemma4))
}

#[tokio::test]
#[ignore = "live network call; needs the DeepInfra test key"]
async fn deepinfra_live_gemma_smoke() {
    let provider = build_provider(
        ProviderKind::DeepInfra,
        load_key(),
        "placeholder",
        None,
        ProviderOptions::default(),
    )
    .expect("provider");

    let t = Instant::now();
    let models = provider.list_models().await.expect("list_models");
    println!("list_models: {} models in {:?}", models.len(), t.elapsed());

    let (model, is_gemma4) = pick_gemma(&models).expect("no Gemma model found on DeepInfra");
    if is_gemma4 {
        println!("Found a Gemma 4 model: {model}");
    } else {
        println!(
            "NO gemma-4/gemma4 id exists; using the newest Gemma instruct model instead: {model}"
        );
    }

    let provider = build_provider(
        ProviderKind::DeepInfra,
        load_key(),
        &model,
        None,
        ProviderOptions::default(),
    )
    .expect("provider");
    let mut req = CompletionRequest::new(vec![Message::user("Reply with the single word: ready")]);
    req.max_tokens = 48;
    req.temperature = Some(0.0);
    let t = Instant::now();
    let c = provider.complete(req).await.expect("completion");
    let latency = t.elapsed();

    let reply: String = c.text.chars().take(60).collect();
    println!("model:   {model}");
    println!("latency: {latency:?}");
    println!(
        "usage:   prompt={} completion={} total={} estimated={}",
        c.usage.prompt_tokens, c.usage.completion_tokens, c.usage.total_tokens, c.usage.estimated
    );
    println!("reply:   {reply:?}");
    assert!(!c.text.trim().is_empty());
}

#[test]
fn gemma_ranking_prefers_newest_instruct() {
    let ms = |ids: &[&str]| -> Vec<ModelInfo> {
        ids.iter()
            .map(|i| ModelInfo {
                id: (*i).into(),
                display_name: None,
            })
            .collect()
    };
    let (m, g4) = pick_gemma(&ms(&[
        "google/gemma-2-9b-it",
        "google/gemma-3-27b-it",
        "google/gemma-3-4b-it",
        "x/llama",
    ]))
    .expect("p");
    assert_eq!(m, "google/gemma-3-4b-it");
    assert!(!g4);
    let (m, g4) = pick_gemma(&ms(&["google/gemma-3-27b-it", "google/gemma-4-31b-it"])).expect("p");
    assert_eq!(m, "google/gemma-4-31b-it");
    assert!(g4);
}
