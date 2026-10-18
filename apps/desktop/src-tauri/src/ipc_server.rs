//! The app's end of the local IPC endpoint (`riptide-ipc`): the Native Messaging host and the
//! CLI connect to `\\.\pipe\riptide.<user-sid>` (DACL: the current user only) and every
//! request is answered by [`dispatch`] against the [`Engine`].
//!
//! | Request | Engine call | Answer |
//! |---|---|---|
//! | `ping` | – | `pong` (app version, [`PROTOCOL_VERSION`]) |
//! | `probe` | [`Engine::probe`] (referrer, user agent and secrets ride along) | `probed` |
//! | `probe_media` | [`Engine::probe_media`] (yt-dlp; the cookies go to a temp file) | `media_probed` |
//! | `add` | [`Engine::add`] | `added`; the window comes to the front |
//! | `list` / `get` | [`Engine::list`] / [`Engine::get`] | `downloads` / `download` |
//! | `refresh` | [`Engine::refresh_url`] | `ok`, or `not_found` when there is no such download |
//! | anything else | – | `unsupported` (full CLI control comes later) |
//!
//! Error messages are redacted (no query strings, no credentials) before they leave the app.

use riptide_core::Secrets;
use riptide_engine::Engine;
use riptide_ipc::{
    ErrorCode, ProbeResult, Request, RequestBody, Response, ResponseBody, PROTOCOL_VERSION,
};

/// Answers one request.
pub async fn dispatch(engine: &Engine, request: Request) -> Response {
    let id = request.id;
    let body = match request.body {
        RequestBody::Ping => Ok(ResponseBody::Pong {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: PROTOCOL_VERSION,
        }),
        RequestBody::Probe {
            url,
            referrer,
            user_agent,
            secrets,
        } => {
            let mut secrets = secrets.unwrap_or_default();
            if let Some(ua) = user_agent {
                secrets.headers.push(("User-Agent".into(), ua));
            }
            let secrets: Option<Secrets> = (!secrets.is_empty()).then_some(secrets);
            engine
                .probe(&url, secrets.as_ref(), referrer.as_deref())
                .await
                .map(|p| ResponseBody::Probed {
                    result: ProbeResult {
                        final_url: p.final_url,
                        filename: p.filename,
                        size: p.size,
                        supports_ranges: p.supports_ranges,
                        content_type: p.content_type,
                    },
                })
        }
        RequestBody::ProbeMedia { url, cookies } => engine
            .probe_media(&url, cookies.as_deref())
            .await
            .map(|info| ResponseBody::MediaProbed { info }),
        RequestBody::Add { request } => engine
            .add(request)
            .await
            .map(|download_id| ResponseBody::Added { download_id }),
        RequestBody::List => engine
            .list()
            .await
            .map(|downloads| ResponseBody::Downloads { downloads }),
        RequestBody::Get { download_id } => match engine.get(download_id).await {
            Ok(Some(d)) => Ok(ResponseBody::Download {
                download: Box::new(d),
            }),
            Ok(None) => Err(riptide_engine::Error::NotFound(download_id)),
            Err(e) => Err(e),
        },
        RequestBody::Refresh {
            download_id,
            url,
            referrer,
            secrets,
        } => engine
            .refresh_url(download_id, &url, referrer.as_deref(), secrets)
            .await
            .map(|()| ResponseBody::Ok),
        _ => {
            return Response::error(
                id,
                ErrorCode::Unsupported,
                "not available over IPC in this build",
            )
        }
    };
    match body {
        Ok(body) => Response::new(id, body),
        Err(e) => engine_error(id, &e),
    }
}

/// Maps an engine error to an `error` response with a redacted message.
pub fn engine_error(id: u64, e: &riptide_engine::Error) -> Response {
    use riptide_engine::Error as E;
    let code = match e {
        E::NotFound(_) => ErrorCode::NotFound,
        E::Core(riptide_core::Error::IllegalTransition { .. }) => ErrorCode::IllegalTransition,
        E::Core(riptide_core::Error::Invalid(_)) => ErrorCode::BadRequest,
        E::Net(riptide_net::Error::InvalidUrl { .. }) => ErrorCode::BadRequest,
        E::Media(riptide_media::Error::InvalidUrl | riptide_media::Error::InvalidFormat(_)) => {
            ErrorCode::BadRequest
        }
        E::Media(
            riptide_media::Error::Drm
            | riptide_media::Error::Live
            | riptide_media::Error::Playlist
            | riptide_media::Error::Unsupported
            | riptide_media::Error::ToolMissing(_),
        ) => ErrorCode::Unsupported,
        _ => ErrorCode::Internal,
    };
    Response::error(id, code, riptide_log::redact_line(&e.to_string()))
}

/// Binds `endpoint` and serves it until the process exits. `on_added` runs after every
/// successful `add` (the app brings its window to the front). Must run inside the async
/// runtime. Fails only if the endpoint cannot be created (another app instance owns it).
pub async fn serve<F>(endpoint: &str, engine: Engine, on_added: F) -> riptide_ipc::Result<()>
where
    F: Fn() + Clone + Send + Sync + 'static,
{
    let listener = riptide_ipc::Listener::bind(endpoint)?;
    tracing::info!("IPC endpoint ready");
    riptide_ipc::serve(listener, move |request: Request| {
        let engine = engine.clone();
        let on_added = on_added.clone();
        async move {
            let kind = request_kind(&request.body);
            let response = dispatch(&engine, request).await;
            tracing::debug!(request = kind, ok = !is_error(&response), "ipc request");
            if matches!(response.body, ResponseBody::Added { .. }) {
                on_added();
            }
            response
        }
    })
    .await
}

fn is_error(r: &Response) -> bool {
    matches!(r.body, ResponseBody::Error { .. })
}

/// The `type` tag of a request, for logs (never the payload: it may carry secrets).
fn request_kind(body: &RequestBody) -> &'static str {
    match body {
        RequestBody::Ping => "ping",
        RequestBody::Probe { .. } => "probe",
        RequestBody::ProbeMedia { .. } => "probe_media",
        RequestBody::Add { .. } => "add",
        RequestBody::Refresh { .. } => "refresh",
        RequestBody::List => "list",
        RequestBody::Get { .. } => "get",
        RequestBody::Pause { .. } => "pause",
        RequestBody::Resume { .. } => "resume",
        RequestBody::Cancel { .. } => "cancel",
        RequestBody::Remove { .. } => "remove",
        RequestBody::Subscribe => "subscribe",
        RequestBody::Shutdown => "shutdown",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use riptide_core::{DownloadId, DownloadRequest, DownloadState};
    use riptide_engine::EngineConfig;

    use super::*;

    async fn engine(dir: &tempfile::TempDir) -> Engine {
        Engine::open(EngineConfig {
            db_path: dir.path().join("riptide.sqlite"),
            default_dir: dir.path().join("downloads"),
        })
        .await
        .unwrap()
    }

    fn paused(url: &str) -> DownloadRequest {
        let mut r = DownloadRequest::new(url);
        r.start_paused = true;
        r
    }

    fn code(r: &Response) -> ErrorCode {
        match r.body {
            ResponseBody::Error { code, .. } => code,
            ref other => panic!("expected an error, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_answers_every_browser_request() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(&dir).await;

        let r = dispatch(&engine, Request::new(1, RequestBody::Ping)).await;
        assert!(
            matches!(r.body, ResponseBody::Pong { protocol, .. } if protocol == PROTOCOL_VERSION)
        );
        assert_eq!(r.id, 1);

        let mut request = paused("http://127.0.0.1:9/file.zip?token=secret");
        request.referrer = Some("https://example.test/page".into());
        request.user_agent = Some("Browser/1".into());
        let r = dispatch(&engine, Request::new(2, RequestBody::Add { request })).await;
        let ResponseBody::Added { download_id } = r.body else {
            panic!("{r:?}")
        };

        let r = dispatch(&engine, Request::new(3, RequestBody::List)).await;
        assert!(
            matches!(r.body, ResponseBody::Downloads { ref downloads } if downloads.len() == 1)
        );

        let r = dispatch(&engine, Request::new(4, RequestBody::Get { download_id })).await;
        assert!(
            matches!(r.body, ResponseBody::Download { ref download } if download.state == DownloadState::Paused)
        );
        let r = dispatch(
            &engine,
            Request::new(
                5,
                RequestBody::Get {
                    download_id: DownloadId(999),
                },
            ),
        )
        .await;
        assert_eq!(code(&r), ErrorCode::NotFound);

        let refresh = |id: u64, download_id: DownloadId, url: &str| {
            Request::new(
                id,
                RequestBody::Refresh {
                    download_id,
                    url: url.into(),
                    referrer: None,
                    secrets: None,
                },
            )
        };
        let r = dispatch(
            &engine,
            refresh(6, download_id, "http://127.0.0.1:9/new.zip"),
        )
        .await;
        assert_eq!(r.body, ResponseBody::Ok);
        let d = engine.get(download_id).await.unwrap().unwrap();
        assert_eq!(d.url, "http://127.0.0.1:9/new.zip");
        assert_eq!(
            d.state,
            DownloadState::Paused,
            "a paused download stays paused"
        );
        let r = dispatch(&engine, refresh(7, DownloadId(999), "http://127.0.0.1:9/x")).await;
        assert_eq!(code(&r), ErrorCode::NotFound);
        let r = dispatch(
            &engine,
            refresh(8, download_id, "file:///C:/Windows/win.ini"),
        )
        .await;
        assert_eq!(code(&r), ErrorCode::BadRequest);

        let r = dispatch(
            &engine,
            Request::new(
                9,
                RequestBody::Add {
                    request: paused("javascript:alert(1)"),
                },
            ),
        )
        .await;
        assert_eq!(code(&r), ErrorCode::BadRequest);
        let r = dispatch(
            &engine,
            Request::new(
                10,
                RequestBody::Probe {
                    url: "ftp://example.test/x".into(),
                    referrer: None,
                    user_agent: Some("UA".into()),
                    secrets: None,
                },
            ),
        )
        .await;
        assert_eq!(code(&r), ErrorCode::BadRequest);

        let r = dispatch(
            &engine,
            Request::new(11, RequestBody::Pause { download_id }),
        )
        .await;
        assert_eq!(code(&r), ErrorCode::Unsupported);
        let r = dispatch(&engine, Request::new(12, RequestBody::Shutdown)).await;
        assert_eq!(code(&r), ErrorCode::Unsupported);
        engine.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_pipe_serves_the_engine_and_reports_adds() {
        let dir = tempfile::tempdir().unwrap();
        let engine = engine(&dir).await;
        let endpoint = riptide_ipc::test_endpoint("desktop-serve");
        let added = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&added);
        let server_engine = engine.clone();
        let ep = endpoint.clone();
        tokio::spawn(async move {
            let _ = serve(&ep, server_engine, move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })
            .await;
        });

        let mut conn = None;
        for _ in 0..50 {
            if let Ok(c) = riptide_ipc::connect(&endpoint).await {
                conn = Some(c);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let mut conn = conn.expect("endpoint came up");
        conn.send(&Request::new(1, RequestBody::Ping))
            .await
            .unwrap();
        let r: Response = conn.recv().await.unwrap().unwrap();
        assert!(matches!(r.body, ResponseBody::Pong { .. }));
        conn.send(&Request::new(
            2,
            RequestBody::Add {
                request: paused("http://127.0.0.1:9/a.bin"),
            },
        ))
        .await
        .unwrap();
        let r: Response = conn.recv().await.unwrap().unwrap();
        assert!(matches!(r.body, ResponseBody::Added { .. }), "{r:?}");
        assert_eq!(added.load(Ordering::SeqCst), 1);
        engine.shutdown().await;
    }

    #[test]
    fn error_messages_are_redacted() {
        let e =
            riptide_engine::Error::Internal("GET https://example.test/f?token=abc failed".into());
        let r = engine_error(1, &e);
        let ResponseBody::Error { message, code } = r.body else {
            panic!()
        };
        assert_eq!(code, ErrorCode::Internal);
        assert!(!message.contains("abc"), "{message}");
    }
}
