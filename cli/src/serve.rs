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
//! - **`/mcp` over HTTP** for remote clients ([`crate::http`]), following the
//!   server's current workspace.
//! - **the duplicate-id watch** ([`crate::watch`]) over that same workspace.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sapphire_bridge_client::BridgeClient;
use sapphire_framework::ipc::{self, ManagedBy};
use sapphire_framework::server::{AppServer, StatusRow, SyncRuntime, WorkspaceHost};
use sapphire_ledger_core::{LEDGER_CTX, LedgerState};
use tokio_util::sync::CancellationToken;

use crate::http::{self, ServerConfig};

/// This binary's version, reported in every handshake.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The server the framework's one-shot verbs dispatch against: nothing
/// mounted.
///
/// `status`, `service`, `workspace`, `workgroup`, `device`, `embedding` and
/// `external-device` read only the app name and the service spec from it and
/// talk to a *running* server (or the bridge) over IPC themselves. Opening a
/// bridge here would make `status` on a host without one fail with a bridge
/// error instead of the framework's "no server is running".
pub fn build_oneshot() -> AppServer {
    AppServer::new(&LEDGER_CTX, VERSION)
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
    let config = ServerConfig::load(&ServerConfig::path())?;

    let mut server = AppServer::new(&LEDGER_CTX, VERSION);
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
    let ledger = Arc::new(CurrentLedger::new(Arc::clone(&host)));

    let mcp_row = if config.http.enabled {
        format!("http://{}/mcp", config.http.addr)
    } else {
        "off".to_owned()
    };
    let server = server.status_rows(Arc::new(move || {
        vec![StatusRow {
            name: "mcp".to_owned(),
            value: mcp_row.clone(),
        }]
    }));

    let cancel = CancellationToken::new();
    let http = if config.http.enabled {
        let source = Arc::clone(&ledger);
        Some(http::serve(&config.http, Arc::new(move || source.get()), cancel.clone()).await?)
    } else {
        None
    };
    let watch = crate::watch::spawn(move || host.root());

    let result = server.run().await;

    cancel.cancel();
    if let Some(http) = http {
        let _ = http.await;
    }
    watch.abort();
    Ok(result?)
}
