//! MCP for sapphire-ledger, on the framework's MCP (`sapphire_framework::mcp`).
//!
//! Two halves, joined by the app server's IPC:
//!
//! - [`methods`]: the ledger's `sapphire-ledger.*` methods, registered on the
//!   app server, working on the workspace it has selected.
//! - [`tools`]: the MCP tools an agent sees, which call those methods. The
//!   framework serves them on stdio (`sapphire-ledger mcp`) and at `/mcp` on
//!   the server's HTTP listener.
//!
//! The methods' names, parameters and results are `sapphire-ledger-api`'s,
//! which a client links in place of this crate.

pub mod methods;
pub mod tools;
mod wire;

pub use methods::{LedgerMethods, StateSource, WriteObserver, register};
pub use tools::{LedgerTools, service};
