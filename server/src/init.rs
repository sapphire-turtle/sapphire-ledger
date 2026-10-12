//! Creating a ledger workspace for this server to serve.
//!
//! The same job `sapphire-ledger init` does, offered here so that setting up a
//! server does not require a second binary. It creates the workspace and stops:
//! a client's token comes from the workgroup's external devices, which the
//! bridge owns.

use std::path::Path;

use anyhow::Context as _;

/// Create a ledger workspace at `root`.
///
/// Refuses a directory that is already a workspace — that check lives in
/// `sapphire_ledger_core::init_workspace`, and the error it returns says so.
///
/// Everything printed here goes to **stderr**, beside the log.
pub fn run(root: &Path, base_currency: &str) -> anyhow::Result<()> {
    sapphire_ledger_core::init_workspace(root, base_currency)
        .with_context(|| format!("failed to initialize a ledger at {}", root.display()))?;

    eprintln!(
        "Initialized a sapphire-ledger workspace at {} (base currency: {base_currency})",
        root.display()
    );
    eprintln!();
    eprintln!("Next, give a client a token (the bridge must be running):");
    eprintln!("  sapphire-bridge external-device add <name> --app sapphire-ledger");
    eprintln!("The token is printed once. It is not recoverable afterwards.");

    Ok(())
}
