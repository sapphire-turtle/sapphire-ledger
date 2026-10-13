//! The app server: what `sapphire-ledger serve` (and a bare `sapphire-ledger`)
//! runs.
//!
//! The framework's [`AppServer`] owns the IPC socket, the current workspace
//! (`workspace init|select|show|move|remove|map`), signals and the log file.
//! This module adds what is the ledger's own:
//!
//! - **sync**, through the host's `sapphire-bridge` when one is running. A
//!   bridge that is not running is not fatal: the server serves its
//!   workspace without sync, and picks the bridge up on its next start.
//! - **the ledger's methods** (`sapphire-ledger.*`), which its MCP tools call:
//!   the framework serves those tools on stdio (`sapphire-ledger mcp`) and, when
//!   `server.toml` turns it on, at `/mcp` on the server's HTTP listener.
//! - **the duplicate-id watch** ([`crate::watch`]) over that same workspace.

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sapphire_bridge_client::BridgeClient;
use sapphire_framework::ipc::{self, ManagedBy};
use sapphire_framework::server::{
    AppServer, SERVER_CONFIG_FILE, ServerConfig, StatusRow, SyncRuntime, WorkspaceHost,
};
use sapphire_ledger_core::{LEDGER_CTX, LedgerState};

/// This binary's version, reported in every handshake.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the HTTP listener binds unless `server.toml`'s `[http] listen` says
/// otherwise: loopback, on the port this app has used since #16.
pub const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:3173";

/// The server the framework's one-shot verbs dispatch against: the ledger's
/// MCP tools and API version, nothing mounted.
///
/// `status`, `service`, `workspace`, `workgroup`, `device`, `embedding` and
/// `external-device` read only the app name and the service spec from it and
/// talk to a *running* server (or the bridge) over IPC themselves; `mcp`
/// serves the ledger's tools on stdio, dialling that server. Opening a
/// bridge here would make `status` on a host without one fail with a bridge
/// error instead of the framework's "no server is running".
pub fn build_oneshot() -> AppServer {
    AppServer::new(&LEDGER_CTX, VERSION)
        .app_api(sapphire_ledger_mcp::APP_API)
        .mcp(sapphire_ledger_mcp::service())
}

/// The ledger's server, without sync: the framework's server with the
/// ledger's methods, its MCP tools, and its HTTP port.
///
/// [`run`] adds the bridge; a test runs this as it is.
pub fn build() -> AppServer {
    let server = build_oneshot().http_listen(default_http_addr());
    let ledger = Arc::new(CurrentLedger::new(Arc::clone(server.host())));
    server
        .extend(move |router| sapphire_ledger_mcp::register(router, Arc::new(move || ledger.get())))
}

fn default_http_addr() -> SocketAddr {
    DEFAULT_HTTP_ADDR
        .parse()
        .expect("the default address parses")
}

/// Refuse a configuration that would serve 403 to every `/mcp` client.
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
         [http] allowed_hosts of {SERVER_CONFIG_FILE}, or bind to loopback."
    )
}

/// The ledger the server's current workspace holds, opened once per root.
///
/// MCP sessions share one [`LedgerState`] while the workspace stays the same;
/// a `workspace select` to another root makes the next session open that one.
pub struct CurrentLedger {
    host: Arc<WorkspaceHost>,
    open: Mutex<Option<(PathBuf, Arc<Mutex<LedgerState>>)>>,
}

impl CurrentLedger {
    pub fn new(host: Arc<WorkspaceHost>) -> CurrentLedger {
        CurrentLedger {
            host,
            open: Mutex::new(None),
        }
    }

    /// The current workspace's ledger, or why there is none.
    pub fn get(&self) -> io::Result<Arc<Mutex<LedgerState>>> {
        let root = self.host.root().ok_or_else(|| {
            io::Error::other(
                "this server has no workspace selected; run \
                 `sapphire-ledger workspace select <dir>`",
            )
        })?;
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((open_root, state)) = open.as_ref()
            && *open_root == root
        {
            return Ok(Arc::clone(state));
        }
        let state = LedgerState::open(&root).map_err(|e| {
            io::Error::other(format!(
                "{} is not a ledger yet ({e}); run `sapphire-ledger init {}`",
                root.display(),
                root.display()
            ))
        })?;
        let state = Arc::new(Mutex::new(state));
        *open = Some((root, Arc::clone(&state)));
        Ok(state)
    }
}

/// Run the server until SIGTERM or SIGINT.
pub async fn run() -> anyhow::Result<()> {
    // The framework reads `server.toml` again when it starts; this read is
    // for what the ledger adds on top: the exposure check and the status row.
    let config = ServerConfig::load_for_app(&LEDGER_CTX)?;
    let http_addr = config.http.listen.unwrap_or_else(default_http_addr);
    if config.mcp.http {
        check_exposure(http_addr, &config.http.allowed_hosts)?;
    }

    let mut server = build();
    match BridgeClient::connect(LEDGER_CTX.app_name, VERSION).await {
        Ok(bridge) => {
            let runtime = SyncRuntime::new(
                &LEDGER_CTX,
                Arc::new(bridge),
                std::env::current_exe()?,
                ManagedBy::Service,
            );
            server = server.sync(Arc::new(runtime));
        }
        // The server is useful without sync (spec §10: a missing bridge stops
        // sync, nothing else). Any other failure — a bridge of another
        // version, say — is a real problem and stops the start.
        Err(ipc::Error::NotRunning(reason)) => {
            tracing::warn!("{reason}; serving without sync until this server is restarted");
        }
        Err(err) => return Err(err.into()),
    }

    let host = Arc::clone(server.host());
    let mcp_row = if config.mcp.http {
        format!("http://{http_addr}/mcp")
    } else {
        "off".to_owned()
    };
    let server = server.status_rows(Arc::new(move || {
        vec![StatusRow {
            name: "mcp".to_owned(),
            value: mcp_row.clone(),
        }]
    }));

    let watch = crate::watch::spawn(move || host.root());
    let result = server.run().await;
    watch.abort();
    Ok(result?)
}
