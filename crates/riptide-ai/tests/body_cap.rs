//! A provider that answers with a huge body must be cut off at the cap, not buffered in full
//! first (review round of alpha.5, P2-1). Fails if the body is read before it is checked.
#![allow(clippy::unwrap_used, missing_docs)]

mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use common::*;
use riptide_ai::{AiError, ProviderKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TOTAL: u64 = 200 * 1024 * 1024;

/// Serves one 200 MiB answer, with a `Content-Length` or chunked, and counts what it wrote.
async fn hostile_server(chunked: bool) -> (String, Arc<AtomicU64>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let written = Arc::new(AtomicU64::new(0));
    let w = written.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let w = w.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let mut seen = Vec::new();
                while !seen.windows(4).any(|x| x == b"\r\n\r\n") {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => seen.extend_from_slice(&buf[..n]),
                    }
                }
                let head = if chunked {
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n".to_string()
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {TOTAL}\r\n\r\n")
                };
                if s.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let block = vec![b' '; 64 * 1024];
                let mut sent = 0u64;
                while sent < TOTAL {
                    let res = if chunked {
                        let mut c = format!("{:x}\r\n", block.len()).into_bytes();
                        c.extend_from_slice(&block);
                        c.extend_from_slice(b"\r\n");
                        s.write_all(&c).await
                    } else {
                        s.write_all(&block).await
                    };
                    if res.is_err() {
                        return;
                    }
                    sent += block.len() as u64;
                    w.store(sent, Ordering::SeqCst);
                }
            });
        }
    });
    (url, written)
}

async fn run(chunked: bool) {
    let (url, written) = hostile_server(chunked).await;
    let p = build(&url);
    let err = p.list_models().await.unwrap_err();
    assert!(
        matches!(&err, AiError::InvalidResponse(m) if m.contains("too large")),
        "{err:?}"
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let sent = written.load(Ordering::SeqCst);
    assert!(
        sent < 48 * 1024 * 1024,
        "the server managed to write {} MiB before the client gave up",
        sent / (1024 * 1024)
    );
}

fn build(url: &str) -> Box<dyn riptide_ai::Provider> {
    let mut opts = fast_opts();
    opts.timeout = std::time::Duration::from_secs(20);
    riptide_ai::build_provider(ProviderKind::OpenAi, key(), MODEL, Some(url), opts).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_200_mib_body_is_refused_without_reading_it() {
    run(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_chunked_body_is_cut_at_the_cap() {
    run(true).await;
}
