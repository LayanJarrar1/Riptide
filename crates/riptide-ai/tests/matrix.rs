//! Mock-server matrix: every adapter against request shape, success, errors, retries, budget,
//! timeouts, and key redaction. Loopback only; no real network.
#![allow(missing_docs)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use riptide_ai::{AiError, ProviderKind, RetryPolicy, TokenBudget};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ALL: [ProviderKind; 10] = ProviderKind::ALL;

fn body_of(req: &wiremock::Request) -> Value {
    serde_json::from_slice(&req.body).expect("json body")
}

#[tokio::test]
async fn request_shape_and_success_for_every_provider() {
    for kind in ALL {
        let server = MockServer::start().await;
        let sp = spec(kind);
        Mock::given(method("POST"))
            .and(path(sp.complete_path.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(kind, "ready", true)))
            .expect(1)
            .mount(&server)
            .await;
        let p = provider(kind, &server, fast_opts());
        let c = p
            .complete(simple_request())
            .await
            .unwrap_or_else(|e| panic!("{kind}: {e}"));
        assert_eq!(c.text, "ready", "{kind}");
        assert_eq!(c.usage.prompt_tokens, 11, "{kind}");
        assert_eq!(c.usage.completion_tokens, 4, "{kind}");
        assert_eq!(c.usage.total_tokens, 15, "{kind}");
        assert!(!c.usage.estimated, "{kind}");

        let reqs = server.received_requests().await.expect("recorded");
        assert_eq!(reqs.len(), 1, "{kind}");
        let r = &reqs[0];
        let h = |n: &str| {
            r.headers
                .get(n)
                .map(|v| v.to_str().expect("ascii").to_string())
        };
        assert!(
            r.url.query().is_none(),
            "{kind}: key must not travel in the URL"
        );
        assert!(!r.url.as_str().contains("LEAKCANARY"), "{kind}");
        assert_eq!(
            h("content-type").as_deref(),
            Some("application/json"),
            "{kind}"
        );
        let b = body_of(r);
        match kind {
            ProviderKind::Anthropic => {
                assert_eq!(h("x-api-key").as_deref(), Some(KEY));
                assert_eq!(h("anthropic-version").as_deref(), Some("2023-06-01"));
                assert!(h("authorization").is_none());
                assert_eq!(b["system"], "You are terse.");
                assert_eq!(b["max_tokens"], 32);
                assert_eq!(b["messages"].as_array().expect("arr").len(), 1);
                assert_eq!(b["messages"][0]["role"], "user");
            }
            ProviderKind::Gemini => {
                assert_eq!(h("x-goog-api-key").as_deref(), Some(KEY));
                assert!(h("authorization").is_none());
                assert_eq!(b["systemInstruction"]["parts"][0]["text"], "You are terse.");
                assert_eq!(b["contents"].as_array().expect("arr").len(), 1);
                assert_eq!(b["contents"][0]["role"], "user");
                assert_eq!(b["generationConfig"]["maxOutputTokens"], 32);
            }
            _ => {
                assert_eq!(
                    h("authorization").as_deref(),
                    Some(format!("Bearer {KEY}").as_str())
                );
                assert_eq!(b["model"], MODEL);
                assert_eq!(b["messages"][0]["role"], "system");
                assert_eq!(b["messages"][1]["role"], "user");
                let tok = if kind == ProviderKind::OpenAi {
                    "max_completion_tokens"
                } else {
                    "max_tokens"
                };
                assert_eq!(b[tok], 32, "{kind}");
                assert!(
                    b.get("temperature").is_none(),
                    "{kind}: temperature only when asked"
                );
                if kind == ProviderKind::OpenRouter {
                    assert!(h("http-referer").expect("referer").contains("Riptide"));
                    assert_eq!(h("x-title").as_deref(), Some("Riptide"));
                } else {
                    assert!(h("x-title").is_none(), "{kind}");
                }
            }
        }
    }
}

#[tokio::test]
async fn json_mode_and_temperature_shapes() {
    for kind in ALL {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(kind, "{}", true)))
            .mount(&server)
            .await;
        let p = provider(kind, &server, fast_opts());
        let mut req = simple_request();
        req.json_mode = true;
        req.temperature = Some(0.0);
        p.complete(req)
            .await
            .unwrap_or_else(|e| panic!("{kind}: {e}"));
        let b = body_of(&server.received_requests().await.expect("rec")[0]);
        match kind {
            ProviderKind::Anthropic => {
                assert!(!p.capabilities().json_mode);
                assert!(b["system"].as_str().expect("s").contains("JSON"));
                assert_eq!(b["temperature"], 0.0);
            }
            ProviderKind::Gemini => {
                assert_eq!(
                    b["generationConfig"]["responseMimeType"],
                    "application/json"
                );
                assert_eq!(b["generationConfig"]["temperature"], 0.0);
            }
            _ => {
                assert_eq!(b["response_format"]["type"], "json_object", "{kind}");
                assert_eq!(b["temperature"], 0.0, "{kind}");
            }
        }
    }
}

#[tokio::test]
async fn missing_usage_is_estimated_and_flagged() {
    for kind in ALL {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(
                kind,
                "twelve chars",
                false,
            )))
            .mount(&server)
            .await;
        let c = provider(kind, &server, fast_opts())
            .complete(simple_request())
            .await
            .expect("ok");
        assert!(c.usage.estimated, "{kind}");
        assert!(
            c.usage.completion_tokens >= 3 && c.usage.total_tokens > 0,
            "{kind}"
        );
    }
}

#[tokio::test]
async fn model_list_parsing_and_connection_test() {
    for kind in ALL {
        let server = MockServer::start().await;
        let (body, expect) = models_body(kind);
        Mock::given(method("GET"))
            .and(path(spec(kind).models_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let p = provider(kind, &server, fast_opts());
        let ids: Vec<String> = p
            .list_models()
            .await
            .expect("models")
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, expect, "{kind}");
        let rep = p.test_connection().await.expect("test");
        assert_eq!(rep.model_count, expect.len(), "{kind}");
        let r = &server.received_requests().await.expect("rec")[0];
        assert!(!r.url.as_str().contains("LEAKCANARY"), "{kind}");
    }
}

#[tokio::test]
async fn http_errors_map_to_typed_errors() {
    for kind in ALL {
        for (status, check) in [
            (401u16, "unauthorized"),
            (403, "unauthorized"),
            (404, "notfound"),
            (400, "badrequest"),
            (500, "down"),
            (503, "down"),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                    "error": { "message": format!("nope, key was {KEY}") }
                })))
                .mount(&server)
                .await;
            let err = provider(kind, &server, fast_opts())
                .complete(simple_request())
                .await
                .expect_err("must fail");
            let ok = match (&err, check) {
                (AiError::Unauthorized, "unauthorized") => true,
                (AiError::ModelNotFound, "notfound") => true,
                (AiError::BadRequest { message }, "badrequest") => {
                    assert!(
                        message.contains("[REDACTED]"),
                        "{kind}: provider text kept, key scrubbed"
                    );
                    true
                }
                (AiError::ProviderDown { status: s }, "down") => *s == status,
                _ => false,
            };
            assert!(ok, "{kind} {status}: got {err:?}");
            assert_no_key(&error_strings(&err), "an error string");
            let n = server.received_requests().await.expect("rec").len();
            let expect = if check == "down" { 3 } else { 1 };
            assert_eq!(n, expect, "{kind} {status}: attempts");
        }
    }
}

#[tokio::test]
async fn gemini_bad_key_400_is_unauthorized() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "code": 400, "message": "API key not valid. Please pass a valid API key.",
                        "status": "INVALID_ARGUMENT" }
        })))
        .mount(&server)
        .await;
    let err = provider(ProviderKind::Gemini, &server, fast_opts())
        .complete(simple_request())
        .await
        .expect_err("fail");
    assert!(matches!(err, AiError::Unauthorized), "{err:?}");
}

#[tokio::test]
async fn rate_limit_then_success_retries() {
    for kind in ALL {
        let server = MockServer::start().await;
        let sp = spec(kind);
        Mock::given(method("POST"))
            .and(path(sp.complete_path.as_str()))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(sp.complete_path.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(kind, "ready", true)))
            .mount(&server)
            .await;
        let c = provider(kind, &server, fast_opts())
            .complete(simple_request())
            .await
            .expect("ok");
        assert_eq!(c.text, "ready", "{kind}");
        assert_eq!(
            server.received_requests().await.expect("rec").len(),
            2,
            "{kind}"
        );
    }
}

#[tokio::test]
async fn retry_after_is_honoured() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(
            ProviderKind::OpenAi,
            "ok",
            true,
        )))
        .mount(&server)
        .await;
    let start = Instant::now();
    provider(ProviderKind::OpenAi, &server, fast_opts())
        .complete(simple_request())
        .await
        .expect("ok");
    assert!(
        start.elapsed() >= Duration::from_millis(990),
        "waited only {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn huge_retry_after_is_not_waited_out() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "3600"))
        .mount(&server)
        .await;
    let start = Instant::now();
    let err = provider(ProviderKind::Groq, &server, fast_opts())
        .complete(simple_request())
        .await
        .expect_err("fail");
    assert!(
        matches!(err, AiError::RateLimited { retry_after: Some(d) } if d == Duration::from_secs(3600)),
        "{err:?}"
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(server.received_requests().await.expect("rec").len(), 1);
}

#[tokio::test]
async fn exhausted_429_returns_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&server)
        .await;
    let err = provider(ProviderKind::Mistral, &server, fast_opts())
        .complete(simple_request())
        .await
        .expect_err("fail");
    assert!(
        matches!(err, AiError::RateLimited { retry_after: None }),
        "{err:?}"
    );
    assert_eq!(server.received_requests().await.expect("rec").len(), 3);
}

#[tokio::test]
async fn server_error_then_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(
            ProviderKind::Together,
            "ok",
            true,
        )))
        .mount(&server)
        .await;
    let c = provider(ProviderKind::Together, &server, fast_opts())
        .complete(simple_request())
        .await
        .expect("ok");
    assert_eq!(c.text, "ok");
}

#[tokio::test]
async fn timeouts_are_typed_and_retried() {
    for kind in [
        ProviderKind::DeepInfra,
        ProviderKind::Anthropic,
        ProviderKind::Gemini,
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(ok_body(kind, "late", true))
                    .set_delay(Duration::from_millis(600)),
            )
            .mount(&server)
            .await;
        let mut o = fast_opts();
        o.timeout = Duration::from_millis(150);
        o.retry = RetryPolicy {
            max_attempts: 2,
            ..o.retry
        };
        let err = provider(kind, &server, o)
            .complete(simple_request())
            .await
            .expect_err("fail");
        assert!(matches!(err, AiError::Timeout), "{kind}: {err:?}");
        assert_eq!(
            server.received_requests().await.expect("rec").len(),
            2,
            "{kind}"
        );
    }
}

#[tokio::test]
async fn invalid_json_and_shapes_are_invalid_response() {
    for kind in ALL {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(spec(kind).complete_path.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .mount(&server)
            .await;
        let err = provider(kind, &server, fast_opts())
            .complete(simple_request())
            .await
            .expect_err("fail");
        assert!(
            matches!(err, AiError::InvalidResponse(_)),
            "{kind}: {err:?}"
        );

        // Valid JSON, wrong shape.
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "unexpected": true })))
            .mount(&server)
            .await;
        let err = provider(kind, &server, fast_opts())
            .complete(simple_request())
            .await
            .expect_err("fail");
        assert!(
            matches!(err, AiError::InvalidResponse(_)),
            "{kind}: {err:?}"
        );
        let err = provider(kind, &server, fast_opts())
            .list_models()
            .await
            .expect_err("fail");
        assert!(
            matches!(err, AiError::InvalidResponse(_)),
            "{kind}: {err:?}"
        );
    }
}

#[tokio::test]
async fn connection_refused_is_network_error() {
    // Bind then drop to get a port nobody listens on.
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    drop(l);
    // Windows takes ~2 s to report a refused connection, so allow it and try once.
    let mut o = fast_opts();
    o.timeout = Duration::from_secs(10);
    o.retry = RetryPolicy {
        max_attempts: 1,
        ..o.retry
    };
    let p = riptide_ai::build_provider(
        ProviderKind::OpenAi,
        key(),
        MODEL,
        Some(&format!("http://127.0.0.1:{port}")),
        o,
    )
    .expect("provider");
    let err = p.complete(simple_request()).await.expect_err("fail");
    assert!(matches!(err, AiError::Network(_)), "{err:?}");
    assert_no_key(&error_strings(&err), "network error");
}

#[tokio::test]
async fn budget_is_enforced_before_calling() {
    for kind in ALL {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(kind, "ok", true)))
            .expect(1)
            .mount(&server)
            .await;
        let mut o = fast_opts();
        o.budget = TokenBudget::new(Some(15)); // one call uses exactly 15
        let p = provider(kind, &server, o.clone());
        p.complete(simple_request())
            .await
            .expect("first call within budget");
        assert_eq!(o.budget.used(), 15, "{kind}");
        let err = p
            .complete(simple_request())
            .await
            .expect_err("second refused");
        assert!(
            matches!(
                err,
                AiError::BudgetExceeded {
                    used: 15,
                    limit: 15
                }
            ),
            "{kind}: {err:?}"
        );
        // The `.expect(1)` above verifies on drop that the server saw only one request.
    }
}

#[tokio::test]
async fn max_tokens_hard_cap_applies() {
    for kind in ALL {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(kind, "ok", true)))
            .mount(&server)
            .await;
        let mut o = fast_opts();
        o.max_tokens_cap = 50;
        let mut req = simple_request();
        req.max_tokens = 100_000;
        provider(kind, &server, o).complete(req).await.expect("ok");
        let b = body_of(&server.received_requests().await.expect("rec")[0]);
        let sent = b["max_tokens"]
            .as_u64()
            .or_else(|| b["max_completion_tokens"].as_u64())
            .or_else(|| b["generationConfig"]["maxOutputTokens"].as_u64());
        assert_eq!(sent, Some(50), "{kind}");
    }
}

#[tokio::test]
async fn empty_conversation_is_refused_locally() {
    let server = MockServer::start().await;
    let p = provider(ProviderKind::OpenAi, &server, fast_opts());
    let err = p
        .complete(riptide_ai::CompletionRequest::new(vec![]))
        .await
        .expect_err("fail");
    assert!(matches!(err, AiError::BadRequest { .. }));
    assert!(server.received_requests().await.expect("rec").is_empty());
}

#[test]
fn all_ten_kinds_parse_and_have_unique_ids() {
    let mut ids = std::collections::HashSet::new();
    for k in ProviderKind::ALL {
        assert_eq!(k.id().parse::<ProviderKind>().expect("parse"), k);
        assert!(ids.insert(k.id()));
        assert!(k.default_base_url().starts_with("https://"));
        assert!(!k.default_base_url().ends_with('/'));
    }
    assert_eq!(ids.len(), 10);
    assert!("nope".parse::<ProviderKind>().is_err());
}
