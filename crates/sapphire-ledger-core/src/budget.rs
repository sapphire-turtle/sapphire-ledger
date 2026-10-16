//! A monthly budget: the transactions carrying `tag`, against
//! `monthly_amount` in `currency`, with the accounts it is paid from.

use chrono::{DateTime, FixedOffset};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// An account reference, as a budget stores it: the id links, the name is
/// for whoever reads the file, and either alone is accepted on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub id: String,
    pub name: String,
    /// The transactions carrying this tag are the budget's.
    pub tag: String,
    pub currency: String,
    #[serde(with = "rust_decimal::serde::str")]
    pub monthly_amount: Decimal,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
    /// What the budget is paid from, summed for the balance view. Last, so
    /// TOML writes it as `[[balance_accounts]]` after the plain keys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub balance_accounts: Vec<AccountRef>,
}
