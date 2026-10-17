//! Core data model and storage for sapphire-ledger.
//!
//! Defines the on-disk record types (Account and Transaction as Markdown with
//! YAML front matter; Assertion, PriceEntry and the workspace Config as TOML),
//! their validation, the [`migrate`] from older layouts, and the [`ops`] write
//! path that mints ids, resolves account references and enforces the
//! refuse-to-overwrite rule. There is no SQLite cache here and none is
//! planned — see "Cache strategy" in `docs/design.md`.

pub mod account;
pub mod assertion;
pub mod budget;
pub mod config;
pub mod error;
pub mod migrate;
pub mod ops;
pub mod panel;
pub mod prices;
pub mod report;
pub mod repository;
pub mod series;
pub mod state;
pub mod transaction;
pub mod validate;
pub mod workspace;

pub use account::{Account, AccountType, account_name_segments, describe_ref, resolve_account};
pub use assertion::{Assertion, Balance};
pub use budget::{AccountRef, Budget};
pub use config::{CURRENT_SCHEMA_VERSION, CacheConfig, Config};
pub use error::{Error, Result};
pub use migrate::{MigrationReport, migrate_workspace};
pub use panel::{Mark, Panel, PanelDefinition, PanelSeries, Period, Plot};
pub use prices::{Price, PriceEntry};
pub use repository::{
    MarkdownRecord, Workspace, from_markdown, load_markdown, load_toml, load_workspace,
    save_markdown, save_toml, to_markdown, walk_files, walk_markdown_files, walk_toml_files,
};
pub use state::LedgerState;
pub use transaction::{Posting, Transaction, TransactionStatus};
pub use validate::ValidationIssue;
pub use workspace::{
    BUDGETS_DIR, PANELS_DIR, PRICES_DIR, account_name_from_relative_path, account_relative_path,
    assertion_relative_path, budget_relative_path, find_workspace_root, init_workspace,
    panel_relative_path, price_relative_path, transaction_relative_path,
};

/// Process-wide application context, naming the cache and data directories
/// the framework uses. Mirrors `JOURNAL_CTX` in sapphire-journal.
pub static LEDGER_CTX: sapphire_workspace::AppContext =
    sapphire_workspace::AppContext::new("sapphire-ledger");

pub use sapphire_workspace::AppKind;

/// Point [`LEDGER_CTX`] at this machine's cache, data and config directories.
///
/// `AppContext::cache_dir()` panics if this has not run, so every binary that
/// touches the context calls this first. The framework resolves the
/// directories (honouring `SAPPHIRE_LEDGER_{CACHE,DATA,CONFIG}_DIR`) and
/// installs its console log subscriber, which prints to **stdout** — a process
/// whose stdout is a protocol (stdio MCP) must install its own subscriber
/// first, or not call this at all. Idempotent: first writer wins.
pub fn init_app_context(kind: AppKind) {
    LEDGER_CTX.init(kind);
}
