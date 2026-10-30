//! sapphire-ledger's app server: what `sapphire-ledger serve` runs, in either
//! edition. A library, not a binary of its own: the server is set up with the
//! same `sapphire-ledger` command that runs it (`sapphire-ledger-cli`), so
//! there is no separate server command to tell from the client's.
//!
//! The framework's [`AppServer`] owns the IPC socket, the current workspace
//! (`workspace init|select|show|move|remove`), signals and the log file.
//! This crate adds what is the ledger's own:
//!
//! - **sync**, through the host's `sapphire-bridge`. A bridge that is not
//!   running is not fatal: the server serves its workspace without sync and
//!   picks the bridge up once it starts, or again after it restarts.
//! - **the ledger's methods** (`sapphire-ledger.*`), which its MCP tools call:
//!   the framework serves those tools on stdio (`sapphire-ledger mcp`) and, when
//!   `config.toml`'s `[http.mcp]` turns it on, at `/mcp` on the server's HTTP
//!   listener.
//! - **the duplicate-id watch** ([`crate::watch`]) over that same workspace.
//!
//! The server restarts in place on `server.restart` (`config set --restart`):
//! [`run`] hands the framework [`build_serving`], which it calls again for each
//! round with the settings read afresh.

pub mod watch;

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use sapphire_framework::ipc::ManagedBy;
use sapphire_framework::server::{
    AppFiles, AppServer, AppSettings, BridgeConnector, FieldError, HttpConfig, SERVER_CONFIG_FILE,
    ServerConfig, SettingDef, Settings, Staged, StatusRow, SyncRuntime, WorkspaceHost,
    serve_restarting,
};
use sapphire_ledger_core::{LEDGER_CTX, LedgerState};

/// The editions' version, which this crate shares, reported in every handshake.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where the HTTP listener binds unless `config.toml`'s `[http] listen` says
/// otherwise: loopback, on the port this app has used since #16.
pub const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:3173";

/// The server the framework's one-shot verbs dispatch against: the ledger's
/// MCP tools and API version, nothing mounted.
///
/// Of the framework's verbs only `service`, `update` and `mcp` build one:
/// `service` reads the service spec from it, `update` the release source, and
/// `mcp` serves the ledger's tools on stdio, dialling the *running* server.
/// The rest talk to that server over IPC without building anything.
pub fn build_oneshot() -> AppServer {
    AppServer::new(&LEDGER_CTX, VERSION)
        .app_api(sapphire_ledger_api::API_VERSION)
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

/// The ledger's settings: no keys of its own, only [`check_exposure`] on the
/// framework's `[http]` table.
///
/// The framework runs it on the device's `config.toml` when the server starts
/// and on every `config set` / `config unset`, so a configuration that would
/// serve 403 to every `/mcp` client neither starts nor gets written.
#[derive(Clone)]
pub struct LedgerSettings;

impl AppSettings for LedgerSettings {
    fn defs(&self) -> Vec<SettingDef> {
        Vec::new()
    }

    fn validate(&self, files: &AppFiles, staged: &Staged) -> Result<(), Vec<FieldError>> {
        // A file that cannot be read or parsed, or an `[http]` table that does
        // not hold valid values, is the framework's to report.
        let Ok(Some(text)) = staged.read(files.device()) else {
            return Ok(());
        };
        let Ok(mut file) = text.parse::<toml::Table>() else {
            return Ok(());
        };
        let http = match file.remove("http") {
            Some(http) => match http.try_into::<HttpConfig>() {
                Ok(http) => http,
                Err(_) => return Ok(()),
            },
            None => HttpConfig::default(),
        };
        if !http.mcp.enabled {
            return Ok(());
        }
        let addr = http.listen.unwrap_or_else(default_http_addr);
        check_exposure(addr, &http.allowed_hosts).map_err(|err| {
            vec![FieldError {
                key: Some("http.allowed_hosts".to_owned()),
                file: Some(files.device().to_path_buf()),
                message: err.to_string(),
            }]
        })
    }
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

/// The ledger's server as `serve` runs it, for one round of
/// [`serve_restarting`]: [`build`] with sync through the host's bridge and the
/// `mcp` status row.
///
/// `exe` is this executable, which the sync runtime reports to the bridge.
/// The runtime is made afresh for each round: a server stops the one it was
/// given when it stops.
pub fn build_serving(settings: &Settings, exe: PathBuf) -> AppServer {
    // A value the framework does not take stops the round when the server
    // starts; until then the row reads as the defaults.
    let config = ServerConfig::from_settings(settings).unwrap_or_default();
    let http_addr = config.http.listen.unwrap_or_else(default_http_addr);
    let mcp_row = if config.http.mcp.enabled {
        format!("http://{http_addr}/mcp")
    } else {
        "off".to_owned()
    };

    // Sync through the host's bridge. The framework connects, and connects
    // again whenever the bridge restarts; a bridge that is not running yet
    // stops nothing but sync (spec §10). Any other failure — a bridge of
    // another version, say — stops the start.
    let runtime = SyncRuntime::connecting(
        &LEDGER_CTX,
        BridgeConnector::standard(LEDGER_CTX.app_name, VERSION),
        exe,
        ManagedBy::Service,
    );
    build()
        .sync(Arc::new(runtime))
        .status_rows(Arc::new(move || {
            vec![StatusRow {
                name: "mcp".to_owned(),
                value: mcp_row.clone(),
            }]
        }))
}

/// Run the server until SIGTERM or SIGINT, restarting it in place whenever
/// `server.restart` asks.
pub async fn run() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;

    // The watch outlives the rounds and follows the workspace of whichever
    // server is serving now.
    let current: Arc<Mutex<Option<Arc<WorkspaceHost>>>> = Arc::default();
    let watched = Arc::clone(&current);
    let watch = crate::watch::spawn(move || {
        let host = watched.lock().unwrap_or_else(|e| e.into_inner()).clone();
        host?.root()
    });

    let result = serve_restarting(&LEDGER_CTX, LedgerSettings, move |settings: &Settings| {
        let server = build_serving(settings, exe.clone());
        *current.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(server.host()));
        server
    })
    .await;
    watch.abort();
    Ok(result?)
}
