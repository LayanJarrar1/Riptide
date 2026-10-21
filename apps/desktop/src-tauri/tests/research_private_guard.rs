//! A search result must not turn a click into a request to a private network (review round of
//! alpha.5, P2-6).
#![allow(clippy::unwrap_used, missing_docs)]

use riptide_desktop_lib::research::aims_at_private_network;
use riptide_engine::{Engine, EngineConfig};
use riptide_research::{Hit, Source};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hits_aimed_at_the_lan_or_localhost_are_refused_unless_the_private_searxng_gave_them() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig {
        db_path: dir.path().join("riptide.sqlite"),
        default_dir: dir.path().join("downloads"),
    })
    .await
    .unwrap();
    let hit = |source: Source, url: &str| Hit::new(source, "1", "t", url);
    for private in [
        "http://127.0.0.1:8080/admin/x.pdf",
        "http://localhost/x.pdf",
        "http://192.168.1.1/x.pdf",
        "http://10.0.0.5/x.pdf",
        "http://169.254.169.254/latest/meta-data/",
        "http://[::1]/x.pdf",
    ] {
        for source in [Source::Arxiv, Source::Github, Source::Web] {
            assert!(
                aims_at_private_network(&engine, &hit(source, private), false).await,
                "{source:?} {private}"
            );
        }
        // the user's own SearXNG on the LAN may name LAN files, nobody else may
        assert!(aims_at_private_network(&engine, &hit(Source::Arxiv, private), true).await);
        assert!(!aims_at_private_network(&engine, &hit(Source::Web, private), true).await);
    }
    // a public address is fine
    assert!(
        !aims_at_private_network(
            &engine,
            &hit(Source::Arxiv, "https://93.184.216.34/x.pdf"),
            false
        )
        .await
    );
    engine.shutdown().await;
}
