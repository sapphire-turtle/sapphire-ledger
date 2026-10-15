//! The methods' results: what the server answers a read with.
//!
//! The same shapes as the records `sapphire-ledger-core` keeps in the files,
//! with nothing of the files in them. Amounts are decimals written as strings
//! so they survive JSON.

use chrono::{DateTime, FixedOffset, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Which side of the books an account is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccountType {
    Asset,
    Liability,
    Equity,
    Income,
    Expense,
}

/// One account, as `list_accounts` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    /// Colon-separated, e.g. `Assets:Cash:JPY`.
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    /// Currencies this account may hold. Empty means any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub currencies: Vec<String>,
    pub opened_at: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<NaiveDate>,
    /// Free-form notes on the account, in Markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Whether a transaction has cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransactionStatus {
    Cleared,
    Pending,
}

/// The inline price on a posting: "this amount, valued at `value` `currency`".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Price {
    #[serde(with = "rust_decimal::serde::str")]
    pub value: Decimal,
    pub currency: String,
}

/// One side of a [`Transaction`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    /// The authoritative link to the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// The account's name as the record has it, which may be stale after a
    /// rename when `account_id` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

/// One transaction, as `get_transaction` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub date: NaiveDate,
    pub narration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TransactionStatus>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
    pub postings: Vec<Posting>,
    /// Free-form notes on the whole transaction, in Markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// One observed exchange rate, `1 base = rate quote` on `date`, as
/// `query_prices` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceEntry {
    pub id: String,
    pub date: NaiveDate,
    pub base: String,
    pub quote: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub rate: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
}

/// One posting flattened with its transaction's context, which is what a
/// caller asking "what happened to this account" actually wants:
/// `query_postings` answers with a list of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostingHit {
    pub transaction_id: String,
    pub date: NaiveDate,
    pub narration: String,
    pub account_id: Option<String>,
    pub account_name: Option<String>,
    /// Signed decimal, as a string so it survives JSON.
    pub amount: String,
    pub currency: String,
}

/// One problem `validate_workspace` found in the books.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assertion_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}
