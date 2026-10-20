//! Streaming completions against a scripted SSE server: chunk boundaries, [DONE], mid-stream
//! disconnects, cancellation, slow servers, retries before the first byte, redaction.
#![allow(missing_docs)]

mod common;

use std::time::{Duration, Instant};

use common::sse::{chunk, serve, Script, Step};
use common::*;
use riptide_ai::{
    build_provider, AiError, CompletionRequest, Message, Provider, ProviderKind, TokenBudget,
};

fn provider_at(
    kind: ProviderKind,
    url: &str,
    opts: riptide_ai::ProviderOptions,
) -> Box<dyn Provider> {
    build_provider(kind, key(), MODEL, Some(url), opts).expect("provider")
}

fn openai_frames(pieces: &[&str]) -> String {
    let mut s = String::new();
    for p in pieces {
        s.push_str(&format!(
            "data: {}\n\n",
            serde_json::json!({"model": MODEL, "choices": [{"delta": {"content": p}}]})
        ));
    }
    s
}

fn openai_tail() -> String {
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
     data: {\"choices\":[],\"usage\":{\"prompt_tokens\":9,\"completion_tokens\":3,\"total_tokens\":12}}\n\n\
     data: [DONE]\n\n"
        .to_string()
}

async fn run(p: &dyn Provider) -> (Result<riptide_ai::Completion, AiError>, Vec<String>) {
    let mut got = Vec::new();
    let r = p
        .complete_stream(simple_request(), &mut |d: String| got.push(d))
        .await;
    (r, got)
}

#[tokio::test]
async fn openai_stream_with_json_split_across_chunks_and_done() {
    let body = format!(
        "{}{}",
        openai_frames(&["Hel", "lo ", "wor", "ld"]),
        openai_tail()
    );
    // cut into 5-byte chunks: boundaries land inside JSON, inside "data:" and between \n\n
    let steps: Vec<Step> = body
        .as_bytes()
        .chunks(5)
        .map(|c| Step::Chunk(c.to_vec()))
        .collect();
    let srv = serve(vec![Script::ok(steps)]).await;
    let p = provider_at(ProviderKind::DeepInfra, &srv.url, fast_opts());
    let (r, got) = run(p.as_ref()).await;
    let c = r.expect("completion");
    assert_eq!(c.text, "Hello world");
    assert_eq!(got.concat(), "Hello world");
    assert!(got.len() >= 4, "deltas arrive piecewise: {got:?}");
    assert_eq!(c.usage.total_tokens, 12);
    assert!(!c.usage.estimated);
    assert_eq!(c.finish_reason.as_deref(), Some("stop"));
    let req = srv.seen.lock().unwrap().requests[0].clone();
    assert!(req.contains("\"stream\":true"), "{req}");
    assert!(req.contains("include_usage"), "{req}");
    assert!(req
        .to_ascii_lowercase()
        .contains("accept: text/event-stream"));
}

#[tokio::test]
async fn anthropic_stream_text_deltas_usage_and_stop() {
    let body = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-x\",\"usage\":{\"input_tokens\":21}}}\n\n\
event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: ping\ndata: {\"type\":\"ping\"}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi \"}}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"there\"}}\n\n\
event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":7}}\n\n\
event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
    let steps: Vec<Step> = body
        .as_bytes()
        .chunks(13)
        .map(|c| Step::Chunk(c.to_vec()))
        .collect();
    let srv = serve(vec![Script::ok(steps)]).await;
    let p = provider_at(ProviderKind::Anthropic, &srv.url, fast_opts());
    let (r, got) = run(p.as_ref()).await;
    let c = r.expect("completion");
    assert_eq!(c.text, "Hi there");
    assert_eq!(got, ["Hi ", "there"]);
    assert_eq!(c.usage.prompt_tokens, 21);
    assert_eq!(c.usage.completion_tokens, 7);
    assert_eq!(c.model, "claude-x");
    let req = srv.seen.lock().unwrap().requests[0].clone();
    assert!(req.contains("\"stream\":true"));
    assert!(req.contains("POST /messages"));
}

#[tokio::test]
async fn gemini_stream_uses_sse_endpoint_and_joins_parts() {
    let body = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Bon\"}]}}]}\r\n\r\n\
data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"jour\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":4,\"candidatesTokenCount\":2,\"totalTokenCount\":6},\"modelVersion\":\"g-1\"}\r\n\r\n";
    let steps: Vec<Step> = body
        .as_bytes()
        .chunks(9)
        .map(|c| Step::Chunk(c.to_vec()))
        .collect();
    let srv = serve(vec![Script::ok(steps)]).await;
    let p = provider_at(ProviderKind::Gemini, &srv.url, fast_opts());
    let (r, got) = run(p.as_ref()).await;
    let c = r.expect("completion");
    assert_eq!(c.text, "Bonjour");
    assert_eq!(got.concat(), "Bonjour");
    assert_eq!(c.usage.total_tokens, 6);
    let req = srv.seen.lock().unwrap().requests[0].clone();
    assert!(
        req.contains(&format!(
            "POST /models/{MODEL}:streamGenerateContent?alt=sse"
        )),
        "{req}"
    );
    // the key travels in a header, never in the URL
    assert!(!req.lines().next().unwrap().contains(KEY));
}

#[tokio::test]
async fn mid_stream_disconnect_is_an_error_and_keeps_what_arrived() {
    let mut steps = vec![chunk(&openai_frames(&["Part one, ", "part two"]))];
    steps.push(Step::Sleep(20));
    steps.push(Step::Drop);
    let srv = serve(vec![Script::ok(steps)]).await;
    let budget = TokenBudget::unlimited();
    let mut opts = fast_opts();
    opts.budget = budget.clone();
    let p = provider_at(ProviderKind::OpenAi, &srv.url, opts);
    let (r, got) = run(p.as_ref()).await;
    let err = r.expect_err("a broken stream is an error");
    assert!(matches!(err, AiError::Network(_)), "{err:?}");
    assert_eq!(got.concat(), "Part one, part two");
    // no retry once text was delivered: a second request would duplicate output
    assert_eq!(srv.seen.lock().unwrap().connections, 1);
    assert!(budget.used() > 0, "the tokens that did arrive are charged");
}

#[tokio::test]
async fn clean_end_without_done_or_finish_reason_is_an_error() {
    let srv = serve(vec![Script::ok(vec![chunk(&openai_frames(&["cut off"]))])]).await;
    let p = provider_at(ProviderKind::Groq, &srv.url, fast_opts());
    let (r, got) = run(p.as_ref()).await;
    assert!(matches!(r, Err(AiError::Network(_))), "{r:?}");
    assert_eq!(got.concat(), "cut off");
}

#[tokio::test]
async fn dropping_the_future_closes_the_connection() {
    // 30 slow deltas; we cancel after the first
    let mut steps = Vec::new();
    for _ in 0..30 {
        steps.push(chunk(&openai_frames(&["x"])));
        steps.push(Step::Sleep(50));
    }
    let srv = serve(vec![Script::ok(steps)]).await;
    let p = provider_at(ProviderKind::DeepInfra, &srv.url, fast_opts());
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let start = Instant::now();
    let mut sink = |d: String| {
        let _ = tx.send(d);
    };
    let fut = p.complete_stream(simple_request(), &mut sink);
    let mut fut = Box::pin(fut);
    tokio::select! {
        _ = &mut fut => panic!("stream should still be running"),
        first = rx.recv() => assert_eq!(first.as_deref(), Some("x")),
    }
    drop(fut); // cancel
    let cancelled_at = start.elapsed();
    // the server notices the closed socket on its next write (well before the 1.5 s script ends)
    let deadline = Instant::now() + Duration::from_millis(1200);
    loop {
        if srv.seen.lock().unwrap().write_failures > 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "server never saw the connection close"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        cancelled_at < Duration::from_millis(1000),
        "{cancelled_at:?}"
    );
}

#[tokio::test]
async fn a_slow_but_live_stream_is_not_cut_off_but_a_silent_one_times_out() {
    // 6 chunks, 150 ms apart = 0.9 s total, with a 400 ms idle timeout: fine
    let mut steps = Vec::new();
    for _ in 0..6 {
        steps.push(chunk(&openai_frames(&["s"])));
        steps.push(Step::Sleep(150));
    }
    steps.push(chunk(&openai_tail()));
    let srv = serve(vec![Script::ok(steps)]).await;
    let mut opts = fast_opts();
    opts.timeout = Duration::from_millis(400);
    let p = provider_at(ProviderKind::Together, &srv.url, opts.clone());
    let (r, got) = run(p.as_ref()).await;
    assert_eq!(r.expect("slow stream completes").text, "ssssss");
    assert_eq!(got.len(), 6);

    // now the server goes quiet after one delta
    let steps = vec![chunk(&openai_frames(&["a"])), Step::Sleep(3000)];
    let srv = serve(vec![Script::ok(steps)]).await;
    let p = provider_at(ProviderKind::Together, &srv.url, opts);
    let started = Instant::now();
    let (r, got) = run(p.as_ref()).await;
    assert!(matches!(r, Err(AiError::Timeout)), "{r:?}");
    assert_eq!(got, ["a"]);
    assert!(started.elapsed() < Duration::from_millis(1500));
}

#[tokio::test]
async fn failures_before_the_first_byte_are_retried() {
    let ok = Script::ok(vec![chunk(&format!(
        "{}{}",
        openai_frames(&["ok"]),
        openai_tail()
    ))]);
    let mut busy = Script::status(503, "{\"error\":{\"message\":\"busy\"}}");
    busy.headers.push("retry-after: 0".into());
    let srv = serve(vec![busy, Script::status(429, "{}"), ok]).await;
    let p = provider_at(ProviderKind::Fireworks, &srv.url, fast_opts());
    let (r, got) = run(p.as_ref()).await;
    assert_eq!(r.expect("third attempt works").text, "ok");
    assert_eq!(got, ["ok"]);
    assert_eq!(srv.seen.lock().unwrap().connections, 3);
}

#[tokio::test]
async fn auth_errors_are_final_and_the_key_never_leaks() {
    let body = format!("{{\"error\":{{\"message\":\"bad key {KEY} given\"}}}}");
    let srv = serve(vec![Script::status(400, &body)]).await;
    let p = provider_at(ProviderKind::OpenAi, &srv.url, fast_opts());
    let (r, _) = run(p.as_ref()).await;
    let e = r.expect_err("400");
    assert!(matches!(e, AiError::BadRequest { .. }), "{e:?}");
    assert_no_key(&error_strings(&e), "the streaming error");
    assert_eq!(srv.seen.lock().unwrap().connections, 1);

    let srv = serve(vec![Script::status(401, "{}")]).await;
    let p = provider_at(ProviderKind::OpenAi, &srv.url, fast_opts());
    assert!(matches!(
        run(p.as_ref()).await.0,
        Err(AiError::Unauthorized)
    ));
}

#[tokio::test]
async fn an_error_object_inside_the_stream_is_reported_and_scrubbed() {
    let frame = format!(
        "data: {{\"error\":{{\"message\":\"nope {KEY}\",\"type\":\"invalid_request_error\"}}}}\n\n"
    );
    let srv = serve(vec![Script::ok(vec![
        chunk(&openai_frames(&["a"])),
        chunk(&frame),
    ])])
    .await;
    let p = provider_at(ProviderKind::DeepSeek, &srv.url, fast_opts());
    let (r, got) = run(p.as_ref()).await;
    let e = r.expect_err("in-stream error");
    assert_no_key(&error_strings(&e), "the in-stream error");
    assert_eq!(got, ["a"]);
}

#[tokio::test]
async fn the_budget_blocks_a_stream_before_anything_is_sent() {
    let srv = serve(vec![Script::ok(vec![])]).await;
    let budget = TokenBudget::new(Some(5));
    budget.record(5);
    let mut opts = fast_opts();
    opts.budget = budget;
    let p = provider_at(ProviderKind::OpenAi, &srv.url, opts);
    let (r, _) = run(p.as_ref()).await;
    assert!(matches!(r, Err(AiError::BudgetExceeded { .. })), "{r:?}");
    assert_eq!(srv.seen.lock().unwrap().connections, 0);
}

#[tokio::test]
async fn max_tokens_cap_applies_to_streams() {
    let srv = serve(vec![Script::ok(vec![chunk(&format!(
        "{}{}",
        openai_frames(&["x"]),
        openai_tail()
    ))])])
    .await;
    let mut opts = fast_opts();
    opts.max_tokens_cap = 17;
    let p = provider_at(ProviderKind::DeepInfra, &srv.url, opts);
    let mut req = CompletionRequest::new(vec![Message::user("hi")]);
    req.max_tokens = 100_000;
    let mut sink = |_d: String| {};
    p.complete_stream(req, &mut sink).await.expect("ok");
    let sent = srv.seen.lock().unwrap().requests[0].clone();
    assert!(sent.contains("\"max_tokens\":17"), "{sent}");
}

#[tokio::test]
async fn default_complete_stream_delivers_the_whole_text_once() {
    use async_trait::async_trait;
    use riptide_ai::{Capabilities, Completion, ModelInfo, Usage};

    #[derive(Debug)]
    struct Plain;
    #[async_trait]
    impl Provider for Plain {
        fn kind(&self) -> ProviderKind {
            ProviderKind::OpenAi
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                json_mode: false,
                tools: false,
                streaming: false,
            }
        }
        fn model(&self) -> &str {
            "plain"
        }
        async fn list_models(&self) -> riptide_ai::Result<Vec<ModelInfo>> {
            Ok(vec![])
        }
        async fn complete(&self, _r: CompletionRequest) -> riptide_ai::Result<Completion> {
            Ok(Completion {
                text: "whole".into(),
                model: "plain".into(),
                usage: Usage::default(),
                finish_reason: None,
            })
        }
    }
    let (r, got) = run(&Plain).await;
    assert_eq!(r.unwrap().text, "whole");
    assert_eq!(got, ["whole"]);
}
