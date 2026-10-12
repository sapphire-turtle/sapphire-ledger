//! MCP server logic for sapphire-ledger.
//!
//! Consumed by `sapphire-ledger`: stdio transport for `sapphire-ledger mcp`,
//! HTTP transport for the `/mcp` its server mounts.

pub mod server;

#[cfg(feature = "http-server")]
pub mod http;

pub use server::{SapphireLedgerServer, run};

#[cfg(feature = "http-server")]
pub use http::{StateSource, mcp_router, mcp_router_with};
