//! `sapphire-ledger`: the ledger's command line, server and MCP server.
//!
//! The ledger's own verbs (`init`, `check`, `account`, `tx`, `assertion`,
//! `price`) work on the ledger's files directly. The framework's verbs
//! (`serve`, `status`, `service`, `workspace`, `mcp`, …) run the app server
//! (`sapphire-ledger-server`) or talk to a running one.
//!
//! The modules are a library so the integration tests can drive them without a
//! process, and so both editions run the same commands ([`run`]): the headless
//! one (`sapphire-ledger-headless`) and the desktop app, which runs them when
//! given a subcommand. Both put `sapphire-ledger` on PATH.

pub mod cli;
pub mod commands;
pub mod posting;

use std::path::{Path, PathBuf};

use anyhow::Result;
use sapphire_framework::server::Settings;
use sapphire_ledger_core::{AppKind, LEDGER_CTX};

use crate::cli::{Command, FrameworkCommand, GlobalArgs};
use sapphire_ledger_server as serve;

/// Run one command, exiting the process with its code when a framework verb
/// fails with one.
pub fn run(command: Command, global: GlobalArgs) -> Result<()> {
    match command {
        // What the service manager starts.
        Command::Framework(FrameworkCommand::Serve) => {
            sapphire_ledger_core::init_app_context(AppKind::Server);
            runtime()?.block_on(serve::run())
        }
        // Warned first: `dispatch` exits when the status is not good.
        Command::Framework(FrameworkCommand::Status) => {
            warn_if_shadowed();
            dispatch(FrameworkCommand::Status)
        }
        // `mcp` among them: its stdout is the MCP stream, and the
        // framework's console log goes to stderr (sapphire-framework#232).
        Command::Framework(command) => dispatch(command),
        Command::Init {
            path,
            base_currency,
        } => {
            let target = path.unwrap_or_else(|| PathBuf::from("."));
            sapphire_ledger_core::init_workspace(&target, &base_currency)?;
            println!(
                "Initialized sapphire-ledger workspace at {} (base currency: {})",
                target.display(),
                base_currency
            );
            println!(
                "To serve it: sapphire-ledger workspace select {}",
                target.display()
            );
            Ok(())
        }
        Command::Check => {
            let root = resolve_root(global.ledger_dir)?;
            let workspace = sapphire_ledger_core::load_workspace(&root)?;
            let issues = workspace.validate();
            if issues.is_empty() {
                println!(
                    "OK: {} account(s), {} transaction(s), {} assertion(s), {} price(s)",
                    workspace.accounts.len(),
                    workspace.transactions.len(),
                    workspace.assertions.len(),
                    workspace.prices.len(),
                );
                Ok(())
            } else {
                for issue in &issues {
                    eprintln!("- {}", issue.message);
                }
                anyhow::bail!("validation failed with {} issue(s)", issues.len());
            }
        }
        Command::Migrate { dry_run } => {
            let root = resolve_root(global.ledger_dir)?;
            commands::migrate(&root, dry_run)
        }
        Command::Account(c) => commands::account(c, &resolve_root(global.ledger_dir)?),
        Command::Tx(c) => commands::tx(c, &resolve_root(global.ledger_dir)?),
        Command::Assertion(c) => commands::assertion(c, &resolve_root(global.ledger_dir)?),
        Command::Price(c) => commands::price(c, &resolve_root(global.ledger_dir)?),
        Command::Panel(c) => commands::panel(c, &resolve_root(global.ledger_dir)?),
    }
}

/// Run one of the framework's verbs other than `serve`, exiting with its code.
fn dispatch(command: FrameworkCommand) -> Result<()> {
    sapphire_ledger_core::init_app_context(AppKind::Cli);
    let code = runtime()?.block_on(command.dispatch(
        &LEDGER_CTX,
        serve::LedgerSettings,
        |_: &Settings| serve::build_oneshot(),
        serve::VERSION,
    ))?;
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

/// Find the ledger root: the given directory, or the nearest one above the
/// working directory that holds a `.sapphire-ledger/`.
///
/// `--ledger-dir` is resolved through the same upward search rather than taken
/// literally, so pointing it at a subdirectory of a ledger works the way `git`
/// does — and so that `check` and the write commands agree on what "this
/// ledger" means no matter where they were run from.
fn resolve_root(ledger_dir: Option<PathBuf>) -> Result<PathBuf> {
    let start = ledger_dir.unwrap_or_else(|| PathBuf::from("."));
    Ok(sapphire_ledger_core::find_workspace_root(&start)?)
}

/// The command both editions put on PATH.
const COMMAND: &str = "sapphire-ledger";

/// Warn when the `sapphire-ledger` on PATH is another executable than this
/// one: with both the headless edition and the desktop app installed, the one a
/// shell or an MCP client runs may not be the one the service was registered
/// with.
fn warn_if_shadowed() {
    let Ok(this) = std::env::current_exe().and_then(std::fs::canonicalize) else {
        return;
    };
    let Some(on_path) = std::env::var_os("PATH").and_then(|path| find_on_path(&path)) else {
        return;
    };
    if std::fs::canonicalize(&on_path).is_ok_and(|found| found != this) {
        eprintln!(
            "warning: `{COMMAND}` on PATH is {}, not this executable ({}); \
             keep only one of the headless edition and the desktop app installed",
            on_path.display(),
            this.display(),
        );
    }
}

/// The first `sapphire-ledger` executable in `path`, a `PATH` value.
fn find_on_path(path: &std::ffi::OsStr) -> Option<PathBuf> {
    let file = format!("{COMMAND}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(path)
        .map(|dir| dir.join(&file))
        .find(|candidate| Path::is_file(candidate))
}
