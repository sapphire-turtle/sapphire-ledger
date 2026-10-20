//! `sapphire-ledger`: the ledger's one binary.
//!
//! The ledger's own verbs (`init`, `check`, `account`, `tx`, `assertion`,
//! `price`) work on the ledger's files directly. The framework's verbs
//! (`serve`, `status`, `service`, `workspace`, `mcp`, …) run the app server
//! or talk to a running one. The modules are a library so the integration
//! tests can drive them without a process.

pub mod cli;
pub mod commands;
pub mod http;
pub mod posting;
pub mod serve;
pub mod watch;
