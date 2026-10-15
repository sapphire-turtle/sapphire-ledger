//! The methods' parameters: what a client sends.
//!
//! Dates and amounts are strings, as a caller writes them; the server parses
//! them and says which one it could not read.

use serde::{Deserialize, Serialize};

/// Explicit empty parameter object for zero-argument tools.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EmptyParams {}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetTransactionParams {
    /// The transaction's 7-character id.
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueryPostingsParams {
    /// Account id, or account name. Either is accepted.
    pub account: Option<String>,
    /// Currency code, e.g. `JPY`.
    pub currency: Option<String>,
    /// Inclusive lower bound, `YYYY-MM-DD`.
    pub date_from: Option<String>,
    /// Inclusive upper bound, `YYYY-MM-DD`.
    pub date_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueryPricesParams {
    pub base: Option<String>,
    pub quote: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddAccountParams {
    /// Colon-separated account name, e.g. `Assets:Cash:JPY`.
    pub name: String,
    /// One of: Asset, Liability, Equity, Income, Expense.
    #[serde(rename = "type")]
    pub account_type: String,
    /// Currencies this account may hold. Empty means any.
    #[serde(default)]
    pub currencies: Vec<String>,
    /// `YYYY-MM-DD` the account opened.
    pub opened_at: String,
    /// Free-form notes on the account, in Markdown.
    pub description: Option<String>,
}

/// One side of a transaction. Amounts are strings so decimals survive JSON.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PostingParam {
    /// Account id or account name — either is accepted, and the account must
    /// already exist. The stored record always keeps the id.
    pub account: String,
    /// Signed decimal, e.g. `1200` or `-1200`.
    pub amount: String,
    /// Currency code, e.g. `JPY`.
    pub currency: String,
    /// For a cross-currency posting: the unit price in `price_currency`.
    pub price_value: Option<String>,
    /// Currency the `price_value` is denominated in.
    pub price_currency: Option<String>,
    pub memo: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddTransactionParams {
    /// `YYYY-MM-DD`.
    pub date: String,
    /// What the transaction was.
    pub narration: String,
    pub payee: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// `cleared` or `pending`.
    pub status: Option<String>,
    /// At least two postings, summing to zero per currency.
    pub postings: Vec<PostingParam>,
    /// Free-form notes on the whole transaction, in Markdown.
    pub comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BalanceParam {
    /// Decimal amount as a string.
    pub amount: String,
    pub currency: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddAssertionParams {
    /// Account id or account name.
    pub account: String,
    /// `YYYY-MM-DD`. The balance is asserted at the END of this date.
    pub date: String,
    pub balances: Vec<BalanceParam>,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddPriceParams {
    pub date: String,
    /// Base currency, e.g. `USD`.
    pub base: String,
    /// Quote currency, e.g. `JPY`.
    pub quote: String,
    /// How many `quote` units one `base` unit costs, as a decimal string.
    pub rate: String,
    /// Where the rate came from, e.g. `manual`.
    pub source: Option<String>,
}
