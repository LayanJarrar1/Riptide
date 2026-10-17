//! The real `riptide-host` binary, spawned the way a browser spawns it, against an in-process
//! fake app endpoint (`riptide-ipc` server).
#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use riptide_core::{DownloadId, DownloadRequest};
use riptide_host::{
    IntegrationSettings, CHROMIUM_EXTENSION_ID, DATA_DIR_ENV, ENDPOINT_ENV, FIREFOX_EXTENSION_ID,
    FORBIDDEN_MESSAGE, INTEGRATION_FILE, MAX_FRAME_BYTES, NOT_RUNNING_MESSAGE, NO_LAUNCH_ENV,
};
use riptide_ipc::{
    read_frame, serve_with_shutdown, test_endpoint, write_frame, ErrorCode, Listener, ProbeResult,
    Request, RequestBody, Response, ResponseBody, PROTOCOL_VERSION,
};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const CHROME_ORIGIN: &str = "chrome-extension://dcoeaodjhaibfchhcbfdkekpcacofnhm/";

/// The fake app: answers the browser types, records every request type it sees.
fn start_app(
    tag: &str,
) -> (
    String,
    Arc<Mutex<Vec<String>>>,
    tokio::sync::oneshot::Sender<()>,
) {
    let endpoint = test_endpoint(tag);
    let listener = Listener::bind(&endpoint).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = Arc::clone(&seen);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let handler = move |req: Request| {
        let log = Arc::clone(&log);
        async move {
            let kind = serde_json::to_value(&req.body).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string();
            log.lock().unwrap().push(kind);
            let body = match req.body {
                RequestBody::Ping => ResponseBody::Pong {
                    version: "fake".into(),
                    protocol: PROTOCOL_VERSION,
                },
                RequestBody::Probe { url, .. } => ResponseBody::Probed {
                    result: ProbeResult {
                        final_url: url,
                        filename: "file.zip".into(),
                        size: Some(42),
                        supports_ranges: true,
                        content_type: None,
                    },
                },
                RequestBody::Add { request } => ResponseBody::Added {
                    download_id: DownloadId(request.url.len() as i64),
                },
                RequestBody::Refresh {
                    download_id: DownloadId(1),
                    ..
                } => ResponseBody::Ok,
                RequestBody::Refresh { .. } => {
                    return Response::error(req.id, ErrorCode::NotFound, "no such download")
                }
                RequestBody::List => ResponseBody::Downloads {
                    downloads: Vec::new(),
                },
                _ => return Response::error(req.id, ErrorCode::Unsupported, "fake app"),
            };
            Response::new(req.id, body)
        }
    };
    tokio::spawn(async move {
        serve_with_shutdown(listener, handler, async {
            let _ = rx.await;
        })
        .await
        .unwrap();
    });
    (endpoint, seen, tx)
}

struct Host {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl Host {
    fn spawn(args: &[&str], endpoint: &str, data_dir: &Path) -> Host {
        let mut child = Command::new(env!("CARGO_BIN_EXE_riptide-host"))
            .args(args)
            .env(ENDPOINT_ENV, endpoint)
            .env(NO_LAUNCH_ENV, "1")
            .env(DATA_DIR_ENV, data_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        Host {
            child,
            stdin,
            stdout,
        }
    }

    async fn ask(&mut self, frame: Value) -> Response {
        write_frame(&mut self.stdin, &frame).await.unwrap();
        self.answer().await
    }

    async fn answer(&mut self) -> Response {
        tokio::time::timeout(Duration::from_secs(20), read_frame(&mut self.stdout))
            .await
            .expect("host answered in time")
            .unwrap()
            .expect("a frame, not end of stream")
    }

    async fn exit_code(mut self) -> i32 {
        drop(self.stdin);
        let status = tokio::time::timeout(Duration::from_secs(20), self.child.wait())
            .await
            .expect("host exited in time")
            .unwrap();
        status.code().unwrap()
    }
}

fn error_of(r: &Response) -> (ErrorCode, String) {
    match &r.body {
        ResponseBody::Error { code, message } => (*code, message.clone()),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[tokio::test]
async fn every_browser_message_round_trips_and_nothing_else_is_forwarded() {
    let (endpoint, seen, stop) = start_app("host-roundtrip");
    let data = tempfile::tempdir().unwrap();
    let mut host = Host::spawn(
        &[CHROME_ORIGIN, "--parent-window=0"],
        &endpoint,
        data.path(),
    );

    let r = host.ask(json!({"id": 1, "type": "ping"})).await;
    assert_eq!(r.id, 1);
    assert!(matches!(r.body, ResponseBody::Pong { protocol, .. } if protocol == PROTOCOL_VERSION));

    let r = host
        .ask(json!({
            "id": 2, "type": "probe", "url": "https://example.test/a.zip",
            "referrer": "https://example.test/", "user_agent": "UA",
            "secrets": {"cookies": "a=b"}
        }))
        .await;
    assert_eq!(r.id, 2);
    assert!(
        matches!(r.body, ResponseBody::Probed { ref result } if result.final_url == "https://example.test/a.zip")
    );

    let mut request = DownloadRequest::new("https://example.test/archive.zip");
    request.referrer = Some("https://example.test/".into());
    request.filename = Some("archive.zip".into());
    let add = serde_json::to_value(Request::new(3, RequestBody::Add { request })).unwrap();
    let r = host.ask(add).await;
    assert_eq!(r.id, 3);
    assert_eq!(
        r.body,
        ResponseBody::Added {
            download_id: DownloadId("https://example.test/archive.zip".len() as i64)
        }
    );

    let r = host
        .ask(json!({"id": 4, "type": "refresh", "download_id": 1, "url": "https://example.test/new"}))
        .await;
    assert_eq!((r.id, r.body), (4, ResponseBody::Ok));
    let r = host
        .ask(json!({"id": 5, "type": "refresh", "download_id": 9, "url": "https://example.test/new"}))
        .await;
    assert_eq!(r.id, 5);
    assert_eq!(error_of(&r).0, ErrorCode::NotFound);

    let r = host.ask(json!({"id": 6, "type": "list"})).await;
    assert_eq!(
        (r.id, r.body),
        (
            6,
            ResponseBody::Downloads {
                downloads: Vec::new()
            }
        )
    );

    // CLI-only and unknown types are refused by the host itself, schema errors too
    for (id, frame) in [
        (7, json!({"id": 7, "type": "pause", "download_id": 1})),
        (8, json!({"id": 8, "type": "shutdown"})),
        (
            9,
            json!({"id": 9, "type": "remove", "download_id": 1, "delete_file": true}),
        ),
        (10, json!({"id": 10, "type": "exec", "cmd": "calc.exe"})),
        (
            11,
            json!({"id": 11, "type": "add", "request": {"no_url": true}}),
        ),
        (12, json!({"id": 12, "type": "probe"})),
    ] {
        let r = host.ask(frame).await;
        assert_eq!(r.id, id);
        assert_eq!(error_of(&r).0, ErrorCode::BadRequest, "frame {id}");
    }

    // a body that is not JSON is answered and the session goes on
    host.stdin.write_all(&3u32.to_le_bytes()).await.unwrap();
    host.stdin.write_all(b"{x!").await.unwrap();
    host.stdin.flush().await.unwrap();
    let r = host.answer().await;
    assert_eq!((r.id, error_of(&r).0), (0, ErrorCode::BadRequest));
    let r = host.ask(json!({"id": 13, "type": "ping"})).await;
    assert_eq!(r.id, 13);

    assert_eq!(host.exit_code().await, 0, "clean exit when the port closes");
    assert_eq!(
        *seen.lock().unwrap(),
        ["ping", "probe", "add", "refresh", "refresh", "list", "ping"],
        "only browser types reach the app"
    );
    let _ = stop.send(());
}

#[tokio::test]
async fn a_wrong_extension_id_is_refused_before_anything_is_forwarded() {
    let (endpoint, seen, stop) = start_app("host-wrong-id");
    let data = tempfile::tempdir().unwrap();
    for args in [
        vec!["chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/"],
        vec!["C:\\x\\manifest.json", "evil@example.com"],
        vec![],
    ] {
        let mut host = Host::spawn(&args, &endpoint, data.path());
        let _ = write_frame(&mut host.stdin, &json!({"id": 1, "type": "ping"})).await;
        let r = host.answer().await;
        assert_eq!(
            error_of(&r),
            (ErrorCode::BadRequest, FORBIDDEN_MESSAGE.into())
        );
        assert_eq!(host.exit_code().await, 2, "{args:?}");
    }
    assert!(seen.lock().unwrap().is_empty());
    let _ = stop.send(());
}

#[tokio::test]
async fn firefox_and_extra_ids_from_the_integration_file_are_accepted() {
    let (endpoint, _seen, stop) = start_app("host-firefox");
    let data = tempfile::tempdir().unwrap();
    let extra = "{01234567-89ab-cdef-0123-456789abcdef}";
    IntegrationSettings {
        browser_host: true,
        extra_extension_ids: vec![extra.into()],
    }
    .save(&data.path().join(INTEGRATION_FILE))
    .unwrap();
    for id in [FIREFOX_EXTENSION_ID, extra] {
        let mut host = Host::spawn(&["C:\\x\\manifest.json", id], &endpoint, data.path());
        let r = host.ask(json!({"id": 1, "type": "ping"})).await;
        assert!(matches!(r.body, ResponseBody::Pong { .. }), "{id}: {r:?}");
        assert_eq!(host.exit_code().await, 0);
    }
    let origin = format!("chrome-extension://{CHROMIUM_EXTENSION_ID}/");
    let mut host = Host::spawn(&[&origin], &endpoint, data.path());
    let r = host.ask(json!({"id": 1, "type": "ping"})).await;
    assert!(matches!(r.body, ResponseBody::Pong { .. }));
    assert_eq!(host.exit_code().await, 0);
    let _ = stop.send(());
}

#[tokio::test]
async fn an_oversize_frame_is_refused_and_ends_the_session() {
    let (endpoint, seen, stop) = start_app("host-oversize");
    let data = tempfile::tempdir().unwrap();
    let mut host = Host::spawn(&[CHROME_ORIGIN], &endpoint, data.path());
    let len = MAX_FRAME_BYTES as u32 + 1;
    host.stdin.write_all(&len.to_le_bytes()).await.unwrap();
    host.stdin.flush().await.unwrap();
    let r = host.answer().await;
    assert_eq!(r.id, 0);
    assert_eq!(error_of(&r).0, ErrorCode::BadRequest);
    assert_eq!(host.exit_code().await, 3);
    assert!(seen.lock().unwrap().is_empty());
    let _ = stop.send(());
}

#[tokio::test]
async fn app_not_running_is_an_error_answer_and_the_host_stays() {
    let endpoint = test_endpoint("host-nobody-listens");
    let data = tempfile::tempdir().unwrap();
    let mut host = Host::spawn(&[CHROME_ORIGIN], &endpoint, data.path());
    for id in [1u64, 2] {
        let r = host.ask(json!({"id": id, "type": "ping"})).await;
        assert_eq!(r.id, id);
        assert_eq!(
            error_of(&r),
            (ErrorCode::Internal, NOT_RUNNING_MESSAGE.into())
        );
    }
    let r = host
        .ask(json!({"id": 3, "type": "add", "request": {"url": "https://example.test/x"}}))
        .await;
    assert_eq!((r.id, error_of(&r).0), (3, ErrorCode::Internal));
    assert_eq!(host.exit_code().await, 0);
}

#[tokio::test]
async fn the_host_connects_once_the_app_comes_up() {
    let data = tempfile::tempdir().unwrap();
    let endpoint = test_endpoint("host-late-app");
    let mut host = Host::spawn(&[CHROME_ORIGIN], &endpoint, data.path());
    let r = host.ask(json!({"id": 1, "type": "ping"})).await;
    assert_eq!(error_of(&r).1, NOT_RUNNING_MESSAGE);

    // the app starts on the same endpoint; the same session now reaches it
    let (late, _seen, stop) = start_app("host-late-app");
    assert_eq!(late, endpoint);
    let r = host.ask(json!({"id": 2, "type": "ping"})).await;
    assert_eq!(r.id, 2);
    assert!(matches!(r.body, ResponseBody::Pong { .. }), "{r:?}");
    assert_eq!(host.exit_code().await, 0);
    let _ = stop.send(());
}
