//! Building and running the HTTP server.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use sapphire_framework::keys::{AuthConfig, BridgeVerifier, protect};
use tokio_util::sync::CancellationToken;

/// The authentication `/mcp` sits behind: the workgroup's external devices,
/// asked of the bridge.
///
/// A client presents a token minted by `external-device add` (on this binary
/// once it carries the framework's commands, or on `sapphire-bridge`), and the
/// bridge says whether it belongs to an external device allowed to use this
/// app. Nothing here holds a key: the ledger keeps only a hash, on the bridge's
/// side. With the bridge down every request is 503, not 401 — the client's
/// token may be fine.
pub fn bridge_auth() -> AuthConfig {
    AuthConfig::new(Arc::new(BridgeVerifier::new(
        sapphire_ledger_core::LEDGER_CTX.app_name,
        env!("CARGO_PKG_VERSION"),
    )))
}

/// Refuse a configuration that would serve 403 to every client.
///
/// rmcp's `Host` allowlist is loopback-only unless widened. Binding beyond
/// loopback without naming a host is not dangerous — it is useless, and it
/// presents as a client bug. Failing here turns that into an obvious
/// configuration error.
///
/// The journal's server only warns in this situation, and that is right
/// there: its `/rpc` keeps answering, so the process is still useful. This
/// one serves nothing but `/mcp`.
pub fn check_exposure(addr: SocketAddr, allowed_hosts: &[String]) -> anyhow::Result<()> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if allowed_hosts.iter().any(|h| !h.trim().is_empty()) {
        return Ok(());
    }
    anyhow::bail!(
        "binding to {addr} without --allowed-host would refuse every request: \
         rmcp's Host allowlist stays loopback-only, so a client reaching this \
         server by any other name gets 403. Pass --allowed-host <hostname> for \
         each name clients use, or bind to loopback."
    )
}

/// Build the whole router this process serves: `/mcp` from
/// `sapphire-ledger-mcp`, wrapped in the framework's `protect`.
///
/// Split out of [`run`] so the composition is testable. [`run`] binds a
/// socket, which a test cannot drive; everything above the socket -- that
/// `protect` is actually applied, that a valid bearer gets through, that
/// nothing but `/mcp` is mounted -- is the part worth pinning, and a `Router`
/// can be called directly with `tower::ServiceExt::oneshot`.
pub fn build_router(
    auth: AuthConfig,
    ledger_dir: &Path,
    allowed_hosts: &[String],
    cancel: CancellationToken,
) -> anyhow::Result<axum::Router> {
    let ledger_state = sapphire_ledger_mcp::server::prepare_state(Some(ledger_dir), false)?;
    let shared = Arc::new(std::sync::Mutex::new(ledger_state));
    let mcp = sapphire_ledger_mcp::http::mcp_router(shared, cancel, None, allowed_hosts);
    Ok(protect(auth, mcp))
}

/// Serve until Ctrl-C or SIGTERM, then drain.
pub async fn run(
    addr: SocketAddr,
    ledger_dir: &Path,
    allowed_hosts: &[String],
) -> anyhow::Result<()> {
    check_exposure(addr, allowed_hosts)?;

    let cancel = CancellationToken::new();
    let app = build_router(bridge_auth(), ledger_dir, allowed_hosts, cancel.clone())?;

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!("sapphire-ledger-server listening on http://{addr}/mcp");

    let watch = crate::watch::spawn(ledger_dir.to_path_buf());

    let shutdown = cancel.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown.cancel();
        })
        .await;

    watch.abort();

    result.context("server failed")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
