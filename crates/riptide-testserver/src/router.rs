//! The axum router shared by the binary and by [`crate::TestServer`].

use std::sync::Arc;

use axum::routing::{any, get};
use axum::Router;

use crate::handlers;
use crate::site;
use crate::state::ServerState;

/// Builds the router for one listener. `get` routes also answer `HEAD`.
pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/", get(handlers::index))
        .route("/ok", get(handlers::ok))
        .route("/file/{*name}", get(handlers::file))
        .route("/noranges", get(handlers::noranges))
        .route("/nohead", any(handlers::nohead))
        .route("/nolength", get(handlers::nolength))
        .route("/slow", get(handlers::slow))
        .route("/drop", get(handlers::drop_after))
        .route("/flaky", get(handlers::flaky))
        .route("/retry-after", get(handlers::retry_after))
        .route("/limit", get(handlers::limit))
        .route("/redirect", get(handlers::redirect))
        .route("/etag-changes", get(handlers::etag_changes))
        .route("/wrong-length", get(handlers::wrong_length))
        .route("/expires", get(handlers::expires))
        .route("/needs-cookie", get(handlers::needs_cookie))
        .route("/needs-referer", get(handlers::needs_referer))
        .route("/needs-auth/basic", get(handlers::needs_auth_basic))
        .route("/needs-auth/digest", get(handlers::needs_auth_digest))
        .route("/gzip", get(handlers::gzip))
        .route("/cd/arabic", get(handlers::cd_arabic))
        .route("/cd/evil", get(handlers::cd_evil))
        .route("/corrupt", get(handlers::corrupt))
        .route("/batch/{name}", get(handlers::batch_item))
        .route("/echo-headers", any(handlers::echo_headers))
        .route("/robots.txt", get(site::robots))
        .route("/site/", get(site::page))
        .route("/site/{*path}", get(site::page))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            handlers::log_request,
        ))
        .with_state(state)
}
