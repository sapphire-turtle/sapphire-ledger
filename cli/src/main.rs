use std::path::PathBuf;

use anyhow::Result;
use clap::Parser as _;
use sapphire_ledger::cli::{Cli, Command, FrameworkCommand};
use sapphire_ledger::{commands, serve};
use sapphire_ledger_core::AppKind;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        // What the service manager starts.
        Command::Framework(FrameworkCommand::Serve) => {
            sapphire_ledger_core::init_app_context(AppKind::Server);
            runtime()?.block_on(serve::run())
        }
        // `mcp` among them: its stdout is the MCP stream, and the
        // framework's console log goes to stderr (sapphire-framework#232).
        Command::Framework(command) => {
            sapphire_ledger_core::init_app_context(AppKind::Cli);
            let code =
                runtime()?.block_on(command.dispatch(serve::build_oneshot(), serve::VERSION))?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
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
            let root = resolve_root(cli.ledger_dir)?;
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
            let root = resolve_root(cli.ledger_dir)?;
            commands::migrate(&root, dry_run)
        }
        Command::Account(c) => commands::account(c, &resolve_root(cli.ledger_dir)?),
        Command::Tx(c) => commands::tx(c, &resolve_root(cli.ledger_dir)?),
        Command::Assertion(c) => commands::assertion(c, &resolve_root(cli.ledger_dir)?),
        Command::Price(c) => commands::price(c, &resolve_root(cli.ledger_dir)?),
        Command::Budget(c) => commands::budget(c, &resolve_root(cli.ledger_dir)?),
    }
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
