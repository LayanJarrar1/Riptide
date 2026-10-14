//! In-process server for tests.

use std::io;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::content::DEFAULT_SEED;
use crate::router::router;
use crate::state::ServerState;

/// A test server bound to two loopback listeners with OS-assigned ports.
///
/// The first listener is the base origin; the second is the "other origin" that
/// `/redirect?cross=1` hops to. Both serve the same router and share one [`ServerState`] (counters,
/// PRNG), and both stop when the value is dropped. Connections that are mid-response when the
/// server is dropped finish on their own.
pub struct TestServer {
    base_url: String,
    alt_url: String,
    state: Arc<ServerState>,
    tasks: Vec<JoinHandle<()>>,
}

impl TestServer {
    /// Binds two listeners on `127.0.0.1` with OS-assigned ports and serves until dropped.
    ///
    /// Must be called from inside a tokio runtime (`#[tokio::test]`). Panics if a loopback port
    /// cannot be bound; use [`TestServer::try_spawn`] to handle that yourself.
    pub async fn spawn() -> TestServer {
        match Self::try_spawn(DEFAULT_SEED).await {
            Ok(server) => server,
            Err(err) => panic!("riptide-testserver: cannot bind loopback listeners: {err}"),
        }
    }

    /// Like [`TestServer::spawn`] with an explicit server seed (drives `/flaky` and the digest
    /// nonce) and an error instead of a panic.
    pub async fn try_spawn(seed: u64) -> io::Result<TestServer> {
        let base_listener = TcpListener::bind("127.0.0.1:0").await?;
        let alt_listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", base_listener.local_addr()?);
        let alt_url = format!("http://{}", alt_listener.local_addr()?);

        let base_state = Arc::new(ServerState::new(seed, &base_url, &alt_url));
        let alt_state = Arc::new(base_state.sibling(&alt_url, &base_url));

        let tasks = vec![
            tokio::spawn(serve(base_listener, Arc::clone(&base_state))),
            tokio::spawn(serve(alt_listener, alt_state)),
        ];
        tracing::debug!(%base_url, %alt_url, "riptide-testserver spawned");
        Ok(TestServer {
            base_url,
            alt_url,
            state: base_state,
            tasks,
        })
    }

    /// Base origin, e.g. `http://127.0.0.1:51234` (no trailing slash).
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The second origin, used by cross-origin redirect tests.
    #[must_use]
    pub fn alt_url(&self) -> &str {
        &self.alt_url
    }

    /// `base_url` + `path_and_query` (which must start with `/`).
    #[must_use]
    pub fn url(&self, path_and_query: &str) -> String {
        format!("{}{}", self.base_url, path_and_query)
    }

    /// `alt_url` + `path_and_query` (which must start with `/`).
    #[must_use]
    pub fn url_on_alt(&self, path_and_query: &str) -> String {
        format!("{}{}", self.alt_url, path_and_query)
    }

    /// The state of the base origin (the alt origin shares its counters).
    #[must_use]
    pub fn state(&self) -> &Arc<ServerState> {
        &self.state
    }
}

async fn serve(listener: TcpListener, state: Arc<ServerState>) {
    if let Err(err) = axum::serve(listener, router(state)).await {
        tracing::error!(%err, "riptide-testserver listener stopped");
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
