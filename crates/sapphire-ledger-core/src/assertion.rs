use jiff::Timestamp;
use jiff::civil::Date;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,
    pub currency: String,
}

/// A balance assertion: at the end of `date`, `account` should hold each of
/// the listed `balances`. Evaluated *after* all transactions on that date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assertion {
    pub id: String,
    /// The authoritative link. Survives a rename of the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// A denormalized copy of the account's name, for whoever reads the raw
    /// file. Never used for matching when `account_id` is set, and allowed to
    /// go stale after a rename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
    pub date: Date,
    pub balances: Vec<Balance>,
    #[serde(with = "crate::time::rfc3339")]
    pub created_at: Timestamp,
    #[serde(with = "crate::time::rfc3339")]
    pub updated_at: Timestamp,
}
