use clap::Parser as _;
use sapphire_ledger_core::AppKind;
use sapphire_ledger_server::cli::{Cli, Command};
use sapphire_ledger_server::{init, serve};

const LEDGER_DIR_REQUIRED: &str = "--ledger-dir is required (or set SAPPHIRE_LEDGER_SERVER_DIR)";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Resolves the app directories and installs the framework's log
    // subscriber (console, plus `app.log` once a server routes it).
    sapphire_ledger_core::init_app_context(AppKind::Server);

    let cli = Cli::parse();
    let ledger_dir = cli
        .ledger_dir
        .clone()
        .ok_or_else(|| anyhow::anyhow!("{LEDGER_DIR_REQUIRED}"))?;

    match cli.command {
        None => serve::run(cli.addr, &ledger_dir, &cli.allowed_host).await,
        Some(Command::Init { base_currency }) => init::run(&ledger_dir, &base_currency),
    }
}
