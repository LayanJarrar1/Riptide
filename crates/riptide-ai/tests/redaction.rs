//! Key redaction across every provider: Debug, Display, error chains and tracing output.
//! Lives in its own test binary because tracing caches callsite interest globally; a parallel
//! test without a subscriber would otherwise make the capture flaky.
#![allow(missing_docs)]

mod common;

use common::*;
use riptide_ai::ProviderKind;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const ALL: [ProviderKind; 10] = ProviderKind::ALL;

#[tokio::test]
async fn keys_never_leak_from_any_provider() {
    let logs = LogCapture::default();
    let _guard = logs.install();
    for kind in ALL {
        let server = MockServer::start().await;
        let p = provider(kind, &server, fast_opts());
        // Debug of the provider and of its pieces.
        assert_no_key(&format!("{p:?} {:#?}", p), "Debug of a provider");
        assert!(format!("{p:?}").contains("[REDACTED]"));
        assert_no_key(&format!("{:?} {}", key(), key()), "Debug/Display of ApiKey");

        // A success, then each failure mode with the key echoed back by the "provider".
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body(kind, "ok", true)))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let c = p.complete(simple_request()).await.expect("ok");
        assert_no_key(&format!("{c:?}"), "Debug of a Completion");
        for status in [400u16, 401, 404, 429, 500] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_string(format!(
                    "{{\"error\":{{\"message\":\"bad {KEY}\"}},\"detail\":\"{KEY}\"}}"
                )))
                .mount(&server)
                .await;
            let err = provider(kind, &server, fast_opts())
                .complete(simple_request())
                .await
                .expect_err("fail");
            assert_no_key(&error_strings(&err), "an error");
        }
        let err = p.list_models().await.expect_err("no mock for GET");
        assert_no_key(&error_strings(&err), "a list_models error");
    }
    assert!(
        !logs.text().is_empty(),
        "tracing capture should have recorded something"
    );
    assert_no_key(&logs.text(), "tracing output");
}
