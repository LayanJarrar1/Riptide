//! `riptide-testserver` binary: serves the test endpoints on two fixed ports.
#![cfg_attr(test, allow(clippy::unwrap_used))]

use std::sync::Arc;

use anyhow::Context as _;
use clap::Parser;
use riptide_testserver::{router, ServerState, DEFAULT_SEED};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

/// Local HTTP test server for Riptide with every failure mode the engine tests need.
#[derive(Parser, Debug)]
#[command(name = "riptide-testserver", version, about)]
struct Args {
    /// Address to bind both listeners to.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,

    /// Port of the base origin.
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Port of the second origin (target of `/redirect?cross=1`).
    #[arg(long, default_value_t = 8081)]
    alt_port: u16,

    /// Server seed: drives `/flaky` and the digest nonce (content seeds come from `?seed=`).
    #[arg(long, default_value_t = DEFAULT_SEED)]
    seed: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let base_listener = TcpListener::bind((args.bind.as_str(), args.port))
        .await
        .with_context(|| format!("binding {}:{}", args.bind, args.port))?;
    let alt_listener = TcpListener::bind((args.bind.as_str(), args.alt_port))
        .await
        .with_context(|| format!("binding {}:{}", args.bind, args.alt_port))?;
    let base_url = format!("http://{}", base_listener.local_addr()?);
    let alt_url = format!("http://{}", alt_listener.local_addr()?);

    let base_state = Arc::new(ServerState::new(args.seed, &base_url, &alt_url));
    let alt_state = Arc::new(base_state.sibling(&alt_url, &base_url));

    tracing::info!(url = %base_url, alt_url = %alt_url, "riptide-testserver listening");

    tokio::try_join!(
        async { axum::serve(base_listener, router(base_state)).await },
        async { axum::serve(alt_listener, router(alt_state)).await },
    )
    .context("serving")?;
    Ok(())
}
