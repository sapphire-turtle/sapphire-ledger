//! The two entry forms — a transaction and an account — as plain data, and
//! what they turn into.
//!
//! Kept free of egui so the rules are testable. The server checks everything
//! again; these checks only let the window say what is wrong before sending.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use sapphire_ledger_core::AccountType;
use sapphire_ledger_mcp::methods::{AddAccountParams, AddTransactionParams, PostingParam};

/// One posting row as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostingRow {
    /// An account name (or id).
    pub account: String,
    /// A signed decimal.
    pub amount: String,
    pub currency: String,
    pub memo: String,
}

/// A transaction's status, as the form offers it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unset,
    Cleared,
    Pending,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Unset, Status::Cleared, Status::Pending];

    pub fn label(self) -> &'static str {
        match self {
            Status::Unset => "—",
            Status::Cleared => "cleared",
            Status::Pending => "pending",
        }
    }

    fn param(self) -> Option<String> {
        match self {
            Status::Unset => None,
            other => Some(other.label().to_owned()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxForm {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub narration: String,
    pub payee: String,
    /// Comma-separated.
    pub tags: String,
    pub status: Status,
    pub postings: Vec<PostingRow>,
}

impl TxForm {
    /// An empty form dated `date`, with the two postings every transaction
    /// needs, in `currency`.
    pub fn new(date: String, currency: &str) -> TxForm {
        let row = PostingRow {
            currency: currency.to_owned(),
            ..PostingRow::default()
        };
        TxForm {
            date,
            narration: String::new(),
            payee: String::new(),
            tags: String::new(),
            status: Status::Unset,
            postings: vec![row.clone(), row],
        }
    }

    /// What the postings sum to per currency, leaving out the currencies that
    /// balance. Empty when the transaction balances. An amount that does not
    /// parse is an error naming its row.
    pub fn imbalance(&self) -> Result<BTreeMap<String, Decimal>, String> {
        let mut sums: BTreeMap<String, Decimal> = BTreeMap::new();
        for (i, row) in self.postings.iter().enumerate() {
            let amount = parse_amount(&row.amount)
                .ok_or_else(|| format!("posting {}: not a number: {:?}", i + 1, row.amount))?;
            *sums.entry(row.currency.trim().to_owned()).or_default() += amount;
        }
        sums.retain(|_, sum| !sum.is_zero());
        Ok(sums)
    }

    /// The amount that would balance the last posting in its currency: what
    /// the "balance" button fills in.
    pub fn balancing_amount(&self) -> Option<Decimal> {
        let (last, rest) = self.postings.split_last()?;
        let currency = last.currency.trim();
        let mut sum = Decimal::ZERO;
        for row in rest.iter().filter(|r| r.currency.trim() == currency) {
            sum += parse_amount(&row.amount)?;
        }
        Some(-sum)
    }

    /// The parameters `add_transaction` takes, or why the form is not ready.
    pub fn to_params(&self) -> Result<AddTransactionParams, String> {
        let date = require(&self.date, "date")?;
        date.parse::<chrono::NaiveDate>()
            .map_err(|_| format!("not a YYYY-MM-DD date: {date}"))?;
        let narration = require(&self.narration, "narration")?;
        if self.postings.len() < 2 {
            return Err("a transaction needs at least two postings".to_owned());
        }
        for (i, row) in self.postings.iter().enumerate() {
            if row.account.trim().is_empty() {
                return Err(format!("posting {}: choose an account", i + 1));
            }
            if row.currency.trim().is_empty() {
                return Err(format!("posting {}: enter a currency", i + 1));
            }
        }
        let imbalance = self.imbalance()?;
        if !imbalance.is_empty() {
            let off: Vec<String> = imbalance.iter().map(|(c, s)| format!("{s} {c}")).collect();
            return Err(format!("does not balance: off by {}", off.join(", ")));
        }
        Ok(AddTransactionParams {
            date: date.to_owned(),
            narration: narration.to_owned(),
            payee: optional(&self.payee),
            tags: self
                .tags
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect(),
            status: self.status.param(),
            postings: self
                .postings
                .iter()
                .map(|row| PostingParam {
                    account: row.account.trim().to_owned(),
                    // Normalised: the server takes a plain decimal, not
                    // the `+` or thousands separators the form allows.
                    // `imbalance` above has parsed every row already.
                    amount: parse_amount(&row.amount)
                        .map(|a| a.to_string())
                        .unwrap_or_default(),
                    currency: row.currency.trim().to_owned(),
                    price_value: None,
                    price_currency: None,
                    memo: optional(&row.memo),
                })
                .collect(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountForm {
    /// Colon-separated, e.g. `Assets:Cash:JPY`.
    pub name: String,
    pub account_type: AccountType,
    /// Comma-separated; empty allows any.
    pub currencies: String,
    /// `YYYY-MM-DD`.
    pub opened_at: String,
    pub description: String,
}

impl AccountForm {
    pub fn new(opened_at: String) -> AccountForm {
        AccountForm {
            name: String::new(),
            account_type: AccountType::Expense,
            currencies: String::new(),
            opened_at,
            description: String::new(),
        }
    }

    /// The parameters `add_account` takes, or why the form is not ready.
    pub fn to_params(&self) -> Result<AddAccountParams, String> {
        let name = require(&self.name, "name")?;
        let opened_at = require(&self.opened_at, "opening date")?;
        opened_at
            .parse::<chrono::NaiveDate>()
            .map_err(|_| format!("not a YYYY-MM-DD date: {opened_at}"))?;
        Ok(AddAccountParams {
            name: name.to_owned(),
            account_type: type_name(self.account_type).to_owned(),
            currencies: self
                .currencies
                .split(',')
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_owned)
                .collect(),
            opened_at: opened_at.to_owned(),
            description: optional(&self.description),
        })
    }
}

/// Every account type, in the order the books are read.
pub const ACCOUNT_TYPES: [AccountType; 5] = [
    AccountType::Asset,
    AccountType::Liability,
    AccountType::Equity,
    AccountType::Income,
    AccountType::Expense,
];

/// The name `add_account` takes for `ty`.
pub fn type_name(ty: AccountType) -> &'static str {
    match ty {
        AccountType::Asset => "Asset",
        AccountType::Liability => "Liability",
        AccountType::Equity => "Equity",
        AccountType::Income => "Income",
        AccountType::Expense => "Expense",
    }
}

/// A typed amount, allowing a leading `+` and thousands separators.
pub fn parse_amount(raw: &str) -> Option<Decimal> {
    let cleaned: String = raw.trim().trim_start_matches('+').replace(',', "");
    cleaned.parse().ok()
}

fn require<'a>(value: &'a str, what: &str) -> Result<&'a str, String> {
    match value.trim() {
        "" => Err(format!("enter a {what}")),
        v => Ok(v),
    }
}

fn optional(value: &str) -> Option<String> {
    Some(value.trim())
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx(rows: &[(&str, &str, &str)]) -> TxForm {
        TxForm {
            date: "2026-05-21".into(),
            narration: "groceries".into(),
            payee: " ".into(),
            tags: "food, , weekly".into(),
            status: Status::Unset,
            postings: rows
                .iter()
                .map(|(account, amount, currency)| PostingRow {
                    account: (*account).into(),
                    amount: (*amount).into(),
                    currency: (*currency).into(),
                    memo: String::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn a_balanced_form_becomes_the_methods_parameters() {
        let p = tx(&[
            ("Expenses:Food", "1,200", "JPY"),
            ("Assets:Cash", "-1200", "JPY"),
        ])
        .to_params()
        .expect("balanced");
        assert_eq!(p.postings.len(), 2);
        assert_eq!(p.postings[0].amount, "1200", "sent as a plain decimal");
        assert_eq!(p.payee, None, "a blank payee is no payee");
        assert_eq!(p.tags, ["food", "weekly"]);
        assert_eq!(p.status, None);
    }

    #[test]
    fn an_unbalanced_form_says_by_how_much_per_currency() {
        let form = tx(&[
            ("Expenses:Food", "1200", "JPY"),
            ("Assets:Cash", "-999", "JPY"),
            ("Expenses:Travel", "5", "USD"),
            ("Assets:Card", "-5", "USD"),
        ]);
        let off = form.imbalance().unwrap();
        assert_eq!(off.len(), 1, "USD balances: {off:?}");
        assert_eq!(off["JPY"], Decimal::from(201));
        let err = form.to_params().unwrap_err();
        assert!(err.contains("201 JPY"), "got: {err}");
    }

    #[test]
    fn a_bad_amount_names_its_row() {
        let err = tx(&[("A", "12x", "JPY"), ("B", "-12", "JPY")])
            .imbalance()
            .unwrap_err();
        assert!(err.contains("posting 1"), "got: {err}");
    }

    #[test]
    fn the_balancing_amount_fills_the_last_row_in_its_currency() {
        let form = tx(&[
            ("Expenses:Food", "1200", "JPY"),
            ("Expenses:Travel", "5", "USD"),
            ("Assets:Cash", "", "JPY"),
        ]);
        assert_eq!(form.balancing_amount(), Some(Decimal::from(-1200)));
    }

    #[test]
    fn a_missing_account_or_date_is_refused_before_sending() {
        let mut form = tx(&[("Expenses:Food", "1", "JPY"), ("", "-1", "JPY")]);
        assert!(form.to_params().unwrap_err().contains("posting 2"));
        form.postings[1].account = "Assets:Cash".into();
        form.date = "21/05/2026".into();
        assert!(form.to_params().unwrap_err().contains("YYYY-MM-DD"));
    }

    #[test]
    fn an_account_form_becomes_the_methods_parameters() {
        let mut form = AccountForm::new("2026-01-01".into());
        assert!(form.to_params().unwrap_err().contains("name"));
        form.name = " Assets:Cash ".into();
        form.account_type = AccountType::Asset;
        form.currencies = "JPY, USD".into();
        let p = form.to_params().unwrap();
        assert_eq!(p.name, "Assets:Cash");
        assert_eq!(p.account_type, "Asset");
        assert_eq!(p.currencies, ["JPY", "USD"]);
        assert_eq!(p.description, None);
    }
}
