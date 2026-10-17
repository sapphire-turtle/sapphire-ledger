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
    /// Tags that `add_transaction` adds to a transaction posting to this
    /// account. Empty means none.
    #[serde(default)]
    pub default_tags: Vec<String>,
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
    /// Add the posting accounts' default tags to `tags`. On unless set to
    /// false: a client that shows the tags before sending (the desktop) sends
    /// false and the tags as shown.
    #[serde(default = "applies")]
    pub apply_default_tags: bool,
}

/// `add_transaction` applies default tags unless told otherwise.
fn applies() -> bool {
    true
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

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateAccountParams {
    /// Account id or account name.
    pub account: String,
    /// The account's new default tags, replacing the old. Leave out to keep them.
    pub default_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddBudgetParams {
    /// What the budget is called.
    pub name: String,
    /// The tag whose transactions count as spending against this budget.
    pub tag: String,
    /// Currency code, e.g. `JPY`.
    pub currency: String,
    /// The month's allowance, as a decimal string.
    pub monthly_amount: String,
    /// Accounts (id or name) whose balance the report shows against the plan.
    #[serde(default)]
    pub balance_accounts: Vec<String>,
    /// Also create the panels `<name> pace` and, when it has balance
    /// accounts, `<name> balance`. If either name is taken, nothing is written.
    #[serde(default)]
    pub create_panels: bool,
}

/// The budget's id or name, and the fields to change; a field left out stays as it is.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateBudgetParams {
    /// The budget's id or name.
    pub budget: String,
    pub name: Option<String>,
    pub tag: Option<String>,
    pub currency: Option<String>,
    pub monthly_amount: Option<String>,
    /// Account ids or names, replacing the old.
    pub balance_accounts: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An agent that has never heard of default tags gets them applied; an
    /// older caller's JSON still parses.
    #[test]
    fn default_tags_apply_unless_a_caller_says_otherwise() {
        let p: AddTransactionParams =
            serde_json::from_str(r#"{"date":"2026-05-21","narration":"n","postings":[]}"#).unwrap();
        assert!(p.apply_default_tags);
        assert!(p.tags.is_empty());
        let p: AddTransactionParams = serde_json::from_str(
            r#"{"date":"2026-05-21","narration":"n","postings":[],"apply_default_tags":false}"#,
        )
        .unwrap();
        assert!(!p.apply_default_tags);
        let a: AddAccountParams =
            serde_json::from_str(r#"{"name":"A","type":"Asset","opened_at":"2026-01-01"}"#)
                .unwrap();
        assert!(a.default_tags.is_empty());
    }
}
