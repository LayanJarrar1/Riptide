//! Client ↔ server round trips over the real platform transport.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use riptide_core::{DownloadId, DownloadRequest};
use riptide_ipc::{
    connect, serve_with_shutdown, test_endpoint, ErrorCode, Listener, Request, RequestBody,
    Response, ResponseBody, MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use tokio::io::AsyncWriteExt;

/// The reference handler: answers ping, add and pause; everything else is `unsupported`.
async fn handler(req: Request) -> Response {
    match req.body {
        RequestBody::Ping => Response::new(
            req.id,
            ResponseBody::Pong {
                version: "test".into(),
                protocol: PROTOCOL_VERSION,
            },
        ),
        RequestBody::Add { request } => {
            // echo the URL length as the id so the test can check the payload arrived intact
            Response::new(
                req.id,
                ResponseBody::Added {
                    download_id: DownloadId(request.url.len() as i64),
                },
            )
        }
        RequestBody::Pause {
            download_id: DownloadId(404),
        } => Response::error(req.id, ErrorCode::NotFound, "no such download"),
        RequestBody::Pause { .. } => Response::new(req.id, ResponseBody::Ok),
        _ => Response::error(req.id, ErrorCode::Unsupported, "not in this test"),
    }
}

/// Starts a server on a fresh endpoint; returns the endpoint and a shutdown trigger.
fn start(tag: &str) -> (String, tokio::sync::oneshot::Sender<()>) {
    let endpoint = test_endpoint(tag);
    let listener = Listener::bind(&endpoint).unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        serve_with_shutdown(listener, handler, async {
            let _ = rx.await;
        })
        .await
        .unwrap();
    });
    (endpoint, tx)
}

#[tokio::test]
async fn ping_pong_round_trip() {
    let (endpoint, stop) = start("ping");
    let mut c = connect(&endpoint).await.unwrap();
    c.send(&Request::new(1, RequestBody::Ping)).await.unwrap();
    let r: Response = c.recv().await.unwrap().unwrap();
    assert_eq!(r.id, 1);
    assert!(matches!(
        r.body,
        ResponseBody::Pong {
            protocol: PROTOCOL_VERSION,
            ..
        }
    ));
    drop(stop);
}

#[tokio::test]
async fn payloads_and_errors_survive_the_wire() {
    let (endpoint, stop) = start("payload");
    let mut c = connect(&endpoint).await.unwrap();

    let url = "https://example.com/ملف.zip".to_string();
    let mut req = DownloadRequest::new(url.clone());
    req.start_paused = true;
    c.send(&Request::new(2, RequestBody::Add { request: req }))
        .await
        .unwrap();
    let r: Response = c.recv().await.unwrap().unwrap();
    assert_eq!(
        r,
        Response::new(
            2,
            ResponseBody::Added {
                download_id: DownloadId(url.len() as i64)
            }
        )
    );

    c.send(&Request::new(
        3,
        RequestBody::Pause {
            download_id: DownloadId(404),
        },
    ))
    .await
    .unwrap();
    let r: Response = c.recv().await.unwrap().unwrap();
    assert_eq!(r.id, 3);
    assert!(matches!(
        r.body,
        ResponseBody::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));

    c.send(&Request::new(4, RequestBody::List)).await.unwrap();
    let r: Response = c.recv().await.unwrap().unwrap();
    assert!(matches!(
        r.body,
        ResponseBody::Error {
            code: ErrorCode::Unsupported,
            ..
        }
    ));
    drop(stop);
}

#[tokio::test]
async fn many_requests_on_one_connection_keep_their_ids() {
    let (endpoint, stop) = start("many");
    let mut c = connect(&endpoint).await.unwrap();
    for i in 0..200u64 {
        c.send(&Request::new(i, RequestBody::Ping)).await.unwrap();
        let r: Response = c.recv().await.unwrap().unwrap();
        assert_eq!(r.id, i);
    }
    drop(stop);
}

#[tokio::test]
async fn concurrent_clients_are_isolated() {
    let (endpoint, stop) = start("concurrent");
    let mut tasks = Vec::new();
    for client in 0..8u64 {
        let endpoint = endpoint.clone();
        tasks.push(tokio::spawn(async move {
            let mut c = connect(&endpoint).await.unwrap();
            for n in 0..25u64 {
                let id = client * 1000 + n;
                c.send(&Request::new(
                    id,
                    RequestBody::Pause {
                        download_id: DownloadId(1),
                    },
                ))
                .await
                .unwrap();
                let r: Response = c.recv().await.unwrap().unwrap();
                assert_eq!(r.id, id);
                assert_eq!(r.body, ResponseBody::Ok);
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    drop(stop);
}

#[tokio::test]
async fn sequential_clients_after_disconnect() {
    let (endpoint, stop) = start("sequential");
    for i in 0..5u64 {
        let mut c = connect(&endpoint).await.unwrap();
        c.send(&Request::new(i, RequestBody::Ping)).await.unwrap();
        let r: Response = c.recv().await.unwrap().unwrap();
        assert_eq!(r.id, i);
        // dropping `c` closes the connection; the next loop iteration needs a fresh instance
    }
    drop(stop);
}

#[tokio::test]
async fn malformed_frame_gets_an_error_and_closes_only_that_connection() {
    let (endpoint, stop) = start("malformed");

    let mut bad = connect(&endpoint).await.unwrap();
    bad.write_all(&[7, 0, 0, 0]).await.unwrap();
    bad.write_all(b"not{json").await.unwrap();
    bad.flush().await.unwrap();
    let r: Response = bad.recv().await.unwrap().unwrap();
    assert_eq!(r.id, 0);
    assert!(matches!(
        r.body,
        ResponseBody::Error {
            code: ErrorCode::BadRequest,
            ..
        }
    ));
    // server closed the connection: next recv is EOF or an error, never a hang
    let next = tokio::time::timeout(Duration::from_secs(5), bad.recv::<Response>())
        .await
        .unwrap();
    assert!(matches!(next, Ok(None) | Err(_)));

    // another client is unaffected
    let mut good = connect(&endpoint).await.unwrap();
    good.send(&Request::new(9, RequestBody::Ping))
        .await
        .unwrap();
    let r: Response = good.recv().await.unwrap().unwrap();
    assert_eq!(r.id, 9);
    drop(stop);
}

#[tokio::test]
async fn oversized_frame_is_rejected_by_the_server() {
    let (endpoint, stop) = start("oversize");
    let mut c = connect(&endpoint).await.unwrap();
    let too_big = u32::try_from(MAX_FRAME_BYTES + 1).unwrap();
    c.write_all(&too_big.to_le_bytes()).await.unwrap();
    c.flush().await.unwrap();
    let r: Response = c.recv().await.unwrap().unwrap();
    assert!(
        matches!(
            r.body,
            ResponseBody::Error {
                code: ErrorCode::BadRequest,
                ..
            }
        ),
        "{r:?}"
    );
    drop(stop);
}

#[tokio::test]
async fn shutdown_stops_accepting() {
    let (endpoint, stop) = start("shutdown");
    let mut c = connect(&endpoint).await.unwrap();
    c.send(&Request::new(1, RequestBody::Ping)).await.unwrap();
    let _: Response = c.recv().await.unwrap().unwrap();
    drop(c);
    stop.send(()).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    // after shutdown the endpoint is gone (or refuses); either way no Pong
    let outcome = tokio::time::timeout(Duration::from_secs(3), async {
        match connect(&endpoint).await {
            Err(_) => true,
            Ok(mut c) => {
                let _ = c.send(&Request::new(2, RequestBody::Ping)).await;
                !matches!(c.recv::<Response>().await, Ok(Some(_)))
            }
        }
    })
    .await
    .unwrap();
    assert!(outcome, "server must not answer after shutdown");
}
