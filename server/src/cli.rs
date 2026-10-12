use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "sapphire-ledger-server",
    about = "Self-hosted MCP server for sapphire-ledger",
    version
)]
pub struct Cli {
    /// Path to the ledger root (the directory containing `.sapphire-ledger/`).
    #[arg(
        long,
        env = "SAPPHIRE_LEDGER_SERVER_DIR",
        global = true,
        value_name = "DIR"
    )]
    pub ledger_dir: Option<PathBuf>,

    /// Address to bind. Loopback by default; widening it requires
    /// `--allowed-host`.
    ///
    /// This server speaks plain HTTP — TLS is out of scope. A bind past
    /// loopback puts bearer tokens on the wire in cleartext, so put it
    /// behind a reverse proxy or on an already-encrypted network.
    #[arg(long, default_value = "127.0.0.1:3173")]
    pub addr: SocketAddr,

    /// A hostname clients use to reach this server. Repeatable. Loopback is
    /// always allowed and never needs listing.
    #[arg(long, value_name = "HOST")]
    pub allowed_host: Vec<String>,

    /// Omit to serve.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a ledger workspace at `--ledger-dir`.
    ///
    /// Creates the directory if it does not exist, and refuses one that is
    /// already a workspace. Clients get their tokens from the workgroup's
    /// external devices (`sapphire-bridge external-device add`).
    Init {
        /// Base currency used for reporting.
        #[arg(long, default_value = "JPY")]
        base_currency: String,
    },
}
