//! MCP for sapphire-ledger, on the framework's MCP (`sapphire_framework::mcp`).
//!
//! Two halves, joined by the app server's IPC:
//!
//! - [`methods`]: the ledger's `sapphire-ledger.*` methods, registered on the
//!   app server, working on the workspace it has selected.
//! - [`tools`]: the MCP tools an agent sees, which call those methods. The
//!   framework serves them on stdio (`sapphire-ledger mcp`) and at `/mcp` on
//!   the server's HTTP listener.

pub mod methods;
pub mod tools;

pub use methods::{APP_API, LedgerMethods, StateSource, WriteObserver, register};
pub use tools::{LedgerTools, service};
