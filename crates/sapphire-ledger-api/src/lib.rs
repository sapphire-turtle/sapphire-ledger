//! What a client says to the sapphire-ledger app server, and how the server
//! answers: the ledger's `sapphire-ledger.*` methods, their parameters and
//! their results.
//!
//! Kept apart from the rest of the ledger so that its version moves only when
//! this API does, and a client that only talks to the server links neither
//! the ledger's files nor its store through it. Its types are plain serde
//! data: the transport is the framework's IPC, the server side
//! `sapphire-ledger-mcp`.
//!
//! The records here are the wire's, not the files': `sapphire-ledger-core`
//! keeps its own, and the server converts. A change to the files' format is
//! not a change to this API unless the server's answers change with it.
//!
//! The framework's own methods every app server serves — `workspace.*`,
//! `sync.*`, `server.info` — are [`server`]'s.

pub use sapphire_server_api as server;

mod params;
pub use params::{
    AddAccountParams, AddAssertionParams, AddBudgetParams, AddPriceParams, AddTransactionParams,
    BalanceParam, EmptyParams, GetTransactionParams, PostingParam, QueryPostingsParams,
    QueryPricesParams, UpdateAccountParams, UpdateBudgetParams,
};

mod series;
pub use series::{
    AddPanelParams, BudgetLine, DeletePanelParams, GroupBy, Interval, Mark, Measure, Panel,
    PanelDefinition, PanelRun, PanelSeries, Period, Plot, PlotRun, QuerySeriesParams,
    RunPanelParams, SeriesData, SeriesQuery, SeriesResult, SeriesRun, UpdatePanelParams,
};

pub mod templates;

mod records;
pub use records::{
    Account, AccountRef, AccountType, Budget, Posting, PostingHit, Price, PriceEntry, Transaction,
    TransactionStatus, ValidationIssue,
};

/// The version of the ledger's API: the methods below and their types,
/// reported in the IPC handshake as the app's API (`app_api`).
///
/// It is this crate's major version, parsed at compile time, so the two cannot
/// drift: a breaking change to a method, a parameter or a result here is a
/// major release of this crate, and that release is the new API version. A
/// client built against another version is refused at connect time rather
/// than mid-call.
pub const API_VERSION: u32 = parse_major(env!("CARGO_PKG_VERSION_MAJOR"));

/// `CARGO_PKG_VERSION_MAJOR` as a number. Cargo guarantees it is decimal digits.
const fn parse_major(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut n = 0u32;
    while i < bytes.len() {
        n = n * 10 + (bytes[i] - b'0') as u32;
        i += 1;
    }
    n
}

/// Every account, as JSON: a list of [`Account`].
pub const LIST_ACCOUNTS: &str = "sapphire-ledger.list_accounts";
/// One transaction by id, as JSON: a [`Transaction`].
pub const GET_TRANSACTION: &str = "sapphire-ledger.get_transaction";
/// The postings matching a filter, as JSON: a list of [`PostingHit`].
pub const QUERY_POSTINGS: &str = "sapphire-ledger.query_postings";
/// The exchange rates matching a filter, as JSON: a list of [`PriceEntry`].
pub const QUERY_PRICES: &str = "sapphire-ledger.query_prices";
/// Check the books, as JSON: a list of [`ValidationIssue`], empty when clean.
pub const VALIDATE_WORKSPACE: &str = "sapphire-ledger.validate_workspace";
/// Open an account; answers with a confirmation to show as it is.
pub const ADD_ACCOUNT: &str = "sapphire-ledger.add_account";
/// Record a transaction; answers with a confirmation to show as it is.
pub const ADD_TRANSACTION: &str = "sapphire-ledger.add_transaction";
/// Assert an account's balance; answers with a confirmation to show as it is.
pub const ADD_ASSERTION: &str = "sapphire-ledger.add_assertion";
/// Record an exchange rate; answers with a confirmation to show as it is.
pub const ADD_PRICE: &str = "sapphire-ledger.add_price";
/// Change an account's default tags; answers with a confirmation to show as it is.
pub const UPDATE_ACCOUNT: &str = "sapphire-ledger.update_account";
/// Every budget, as JSON: a list of [`Budget`].
pub const LIST_BUDGETS: &str = "sapphire-ledger.list_budgets";
/// Create a budget; answers with a confirmation to show as it is.
pub const ADD_BUDGET: &str = "sapphire-ledger.add_budget";
/// Change a budget; answers with a confirmation to show as it is.
pub const UPDATE_BUDGET: &str = "sapphire-ledger.update_budget";
/// A series query, as JSON: a [`SeriesResult`].
pub const QUERY_SERIES: &str = "sapphire-ledger.query_series";
/// Every panel, as JSON: a list of [`Panel`].
pub const LIST_PANELS: &str = "sapphire-ledger.list_panels";
/// A panel's queries run, as JSON: a [`PanelRun`].
pub const RUN_PANEL: &str = "sapphire-ledger.run_panel";
/// Save a panel; answers with a confirmation to show as it is.
pub const ADD_PANEL: &str = "sapphire-ledger.add_panel";
/// Replace a panel's definition; answers with a confirmation to show as it is.
pub const UPDATE_PANEL: &str = "sapphire-ledger.update_panel";
/// Remove a panel; answers with a confirmation to show as it is.
pub const DELETE_PANEL: &str = "sapphire-ledger.delete_panel";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_version_is_the_major() {
        assert_eq!(API_VERSION, 2, "the crate's major is the API version");
    }
}
