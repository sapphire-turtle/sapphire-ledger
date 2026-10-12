//! `/mcp` over HTTP, the part of `serve` that remote MCP clients reach.
//!
//! The app server's own surface is its IPC socket, which only this OS user
//! can open. `/mcp` is for clients that are not on this host or not this
//! user — an agent elsewhere on the network — so it sits behind the
//! workgroup's external-device tokens.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use sapphire_framework::keys::{AuthConfig, BridgeVerifier, protect};
use sapphire_ledger_mcp::StateSource;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

/// The file `serve` reads its HTTP settings from, in this app's config
/// directory.
///
/// A file rather than flags: `serve` is what the service manager starts, and
/// it starts it with no arguments (`["serve"]`), so a setting that only a flag
/// could carry would be lost the moment the server is installed as a service.
pub const CONFIG_FILE: &str = "server.toml";

/// The default bind: loopback, on the port this app has used since #16.
pub const DEFAULT_ADDR: &str = "127.0.0.1:3173";

/// `server.toml`.
///
/// ```toml
/// [http]
/// enabled = true                 # false: serve no /mcp at all
/// addr = "127.0.0.1:3173"
/// allowed_hosts = ["ledger.example.net"]
/// ```
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default)]
    pub http: HttpConfig,
}

/// The `[http]` table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpConfig {
    /// Whether `serve` mounts `/mcp` at all.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Address to bind. Loopback by default; widening it requires
    /// `allowed_hosts`.
    ///
    /// This server speaks plain HTTP — TLS is out of scope. A bind past
    /// loopback puts bearer tokens on the wire in cleartext, so put it behind
    /// a reverse proxy or on an already-encrypted network.
    #[serde(default = "default_addr")]
    pub addr: SocketAddr,
    /// Hostnames clients use to reach this server. Loopback is always
    /// allowed and never needs listing.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            enabled: default_enabled(),
            addr: default_addr(),
            allowed_hosts: Vec::new(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_addr() -> SocketAddr {
    DEFAULT_ADDR.parse().expect("the default address parses")
}

impl ServerConfig {
    /// Read `path`; a missing file is the defaults.
    pub fn load(path: &Path) -> anyhow::Result<ServerConfig> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ServerConfig::default()),
            Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    /// Where this app keeps it: `<config dir>/server.toml`.
    ///
    /// Reads `LEDGER_CTX`, so `init_app_context` must have run.
    pub fn path() -> PathBuf {
        sapphire_ledger_core::LEDGER_CTX
            .config_dir()
            .join(CONFIG_FILE)
    }
}

/// The authentication `/mcp` sits behind: the workgroup's external devices,
/// asked of the bridge.
///
/// A client presents a token minted by `sapphire-ledger external-device add`
/// (or `sapphire-bridge external-device add --app sapphire-ledger`), and the
/// bridge says whether it belongs to an external device allowed to use this
/// app. Nothing here holds a key: the ledger keeps only a hash, on the
/// bridge's side. With the bridge down every request is 503, not 401 — the
/// client's token may be fine.
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
pub fn check_exposure(addr: SocketAddr, allowed_hosts: &[String]) -> anyhow::Result<()> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if allowed_hosts.iter().any(|h| !h.trim().is_empty()) {
        return Ok(());
    }
    anyhow::bail!(
        "binding to {addr} without allowed_hosts would refuse every request: \
         rmcp's Host allowlist stays loopback-only, so a client reaching this \
         server by any other name gets 403. List each name clients use in \
         [http] allowed_hosts of {CONFIG_FILE}, or bind to loopback."
    )
}

/// Build the router `/mcp` is served from: `sapphire-ledger-mcp`'s, wrapped
/// in the framework's `protect`.
///
/// Split out of [`serve`] so the composition is testable: everything above
/// the socket -- that `protect` is actually applied, that a valid bearer gets
/// through, that nothing but `/mcp` is mounted -- can be driven with
/// `tower::ServiceExt::oneshot`.
pub fn build_router(
    auth: AuthConfig,
    source: StateSource,
    allowed_hosts: &[String],
    cancel: CancellationToken,
) -> axum::Router {
    let mcp = sapphire_ledger_mcp::mcp_router_with(source, cancel, None, allowed_hosts);
    protect(auth, mcp)
}

/// Bind `config.addr` and serve `/mcp` until `cancel` fires.
///
/// The bind happens before this returns, so a taken port fails `serve` at
/// start-up rather than in a background task nobody watches.
pub async fn serve(
    config: &HttpConfig,
    source: StateSource,
    cancel: CancellationToken,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    check_exposure(config.addr, &config.allowed_hosts)?;
    let app = build_router(bridge_auth(), source, &config.allowed_hosts, cancel.clone());
    let listener = tokio::net::TcpListener::bind(config.addr)
        .await
        .with_context(|| format!("failed to bind {}", config.addr))?;
    tracing::info!("serving MCP on http://{}/mcp", config.addr);

    Ok(tokio::spawn(async move {
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move { cancel.cancelled().await })
            .await;
        if let Err(err) = result {
            tracing::error!("the /mcp listener failed: {err}");
        }
    }))
}
