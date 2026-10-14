//! A small accept loop: one task per connection, one handler call per request.

use std::future::Future;

use crate::message::{ErrorCode, Request, Response};
use crate::transport::{Connection, Listener};
use crate::{Error, Result};

/// Serves `listener` forever: every request on every connection is passed to `handler` and its
/// response is written back. A malformed frame gets an `error` response with id `0` and closes
/// that connection; other connections are unaffected.
pub async fn serve<H, Fut>(listener: Listener, handler: H) -> Result<()>
where
    H: Fn(Request) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Response> + Send + 'static,
{
    serve_with_shutdown(listener, handler, std::future::pending::<()>()).await
}

/// Like [`serve`], but returns `Ok(())` once `shutdown` resolves. Connections already accepted
/// finish their current request and are then dropped with the runtime task.
pub async fn serve_with_shutdown<H, Fut, S>(
    mut listener: Listener,
    handler: H,
    shutdown: S,
) -> Result<()>
where
    H: Fn(Request) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Response> + Send + 'static,
    S: Future<Output = ()>,
{
    tokio::pin!(shutdown);
    loop {
        let conn = tokio::select! {
            biased;
            () = &mut shutdown => return Ok(()),
            accepted = listener.accept() => accepted?,
        };
        let handler = handler.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(conn, handler).await {
                tracing::debug!(error = %e, "ipc connection ended with error");
            }
        });
    }
}

async fn handle_connection<H, Fut>(mut conn: Connection, handler: H) -> Result<()>
where
    H: Fn(Request) -> Fut,
    Fut: Future<Output = Response>,
{
    loop {
        let request = match conn.recv::<Request>().await {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(()),
            Err(e @ (Error::Json(_) | Error::FrameTooLarge { .. })) => {
                let msg = match &e {
                    Error::FrameTooLarge { len, max } => {
                        format!("frame of {len} bytes exceeds the {max}-byte limit")
                    }
                    _ => "malformed request".to_string(),
                };
                let _ = conn
                    .send(&Response::error(0, ErrorCode::BadRequest, msg))
                    .await;
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        let response = handler(request).await;
        conn.send(&response).await?;
    }
}
