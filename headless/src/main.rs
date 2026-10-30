//! `sapphire-ledger`, the headless edition: the command line of
//! [`sapphire_ledger_cli`] and nothing else. The desktop app's binary runs the
//! same commands.

use anyhow::Result;
use clap::Parser as _;
use sapphire_ledger_cli::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    sapphire_ledger_cli::run(cli.command, cli.global)
}
