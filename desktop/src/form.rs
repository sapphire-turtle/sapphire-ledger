//! The two entry forms — a transaction and an account — as plain data, and
//! what they turn into.
//!
//! Kept free of egui so the rules are testable. The server checks everything
//! again; these checks only let the window say what is wrong before sending.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use sapphire_ledger_api::{
    Account, AccountType, AddAccountParams, AddBudgetParams, AddTransactionParams, Budget,
    PostingParam, UpdateAccountParams, UpdateBudgetParams,
};

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
    /// Markdown; the body of the transaction's file.
    pub comment: String,
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
            comment: String::new(),
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
            // The tags field already shows the accounts' defaults; what it shows is what is stored.
            apply_default_tags: false,
            date: date.to_owned(),
            narration: narration.to_owned(),
            payee: optional(&self.payee),
            tags: split_list(&self.tags),
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
            comment: optional(&self.comment),
        })
    }

    /// Append the `tags` the field does not have yet, keeping what was
    /// typed: what choosing a posting's account does with its default tags.
    pub fn add_tags(&mut self, tags: &[String]) {
        let mut current = split_list(&self.tags);
        let before = current.len();
        for tag in tags {
            if !current.contains(tag) {
                current.push(tag.clone());
            }
        }
        if current.len() != before {
            self.tags = current.join(", ");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountForm {
    /// Colon-separated, e.g. `Assets:Cash:JPY`.
    pub name: String,
    pub account_type: AccountType,
    /// Comma-separated; empty allows any.
    pub currencies: String,
    /// Comma-separated; copied into transactions posting here.
    pub default_tags: String,
    /// `YYYY-MM-DD`.
    pub opened_at: String,
    /// Markdown; the body of the account's file.
    pub description: String,
}

impl AccountForm {
    pub fn new(opened_at: String) -> AccountForm {
        AccountForm {
            name: String::new(),
            account_type: AccountType::Expense,
            currencies: String::new(),
            default_tags: String::new(),
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
            default_tags: split_list(&self.default_tags),
            name: name.to_owned(),
            account_type: type_name(self.account_type).to_owned(),
            currencies: split_list(&self.currencies),
            opened_at: opened_at.to_owned(),
            description: optional(&self.description),
        })
    }
}

/// A comma-separated field as its items: trimmed, empty ones dropped.
pub fn split_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Editing one account's default tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagsForm {
    pub account_id: String,
    pub account_name: String,
    /// Comma-separated.
    pub tags: String,
}

impl TagsForm {
    pub fn new(account: &Account) -> TagsForm {
        TagsForm {
            account_id: account.id.clone(),
            account_name: account.name.clone(),
            tags: account.default_tags.join(", "),
        }
    }

    pub fn to_params(&self) -> UpdateAccountParams {
        UpdateAccountParams {
            account: self.account_id.clone(),
            default_tags: Some(split_list(&self.tags)),
        }
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

/// A budget, new or being edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetForm {
    /// The budget being edited; `None` for a new one.
    pub id: Option<String>,
    pub name: String,
    pub tag: String,
    pub currency: String,
    pub monthly_amount: String,
    /// The ids of the accounts it is paid from.
    pub balance_accounts: Vec<String>,
}

impl BudgetForm {
    pub fn new(currency: &str) -> BudgetForm {
        BudgetForm {
            id: None,
            name: String::new(),
            tag: String::new(),
            currency: currency.to_owned(),
            monthly_amount: String::new(),
            balance_accounts: vec![],
        }
    }

    /// Start from `budget`, its account references turned into ids the way
    /// the server resolves them: by id, else by name.
    pub fn edit(budget: &Budget, accounts: &[Account]) -> BudgetForm {
        let balance_accounts = budget
            .balance_accounts
            .iter()
            .filter_map(|r| match &r.account_id {
                Some(id) => Some(id.clone()),
                None => accounts
                    .iter()
                    .find(|a| Some(a.name.as_str()) == r.account_name.as_deref())
                    .map(|a| a.id.clone()),
            })
            .collect();
        BudgetForm {
            id: Some(budget.id.clone()),
            name: budget.name.clone(),
            tag: budget.tag.clone(),
            currency: budget.currency.clone(),
            monthly_amount: budget.monthly_amount.clone(),
            balance_accounts,
        }
    }

    pub fn toggle(&mut self, id: &str) {
        match self.balance_accounts.iter().position(|a| a == id) {
            Some(i) => {
                self.balance_accounts.remove(i);
            }
            None => self.balance_accounts.push(id.to_owned()),
        }
    }

    fn checked(&self) -> Result<(String, String, String, String), String> {
        let name = require(&self.name, "name")?.to_owned();
        let tag = require(&self.tag, "tag")?.to_owned();
        let currency = require(&self.currency, "currency")?.to_owned();
        let amount = parse_amount(&self.monthly_amount)
            .ok_or_else(|| format!("not a number: {:?}", self.monthly_amount))?;
        if amount <= Decimal::ZERO {
            return Err("the monthly amount must be positive".to_owned());
        }
        Ok((name, tag, currency, amount.to_string()))
    }

    pub fn to_add_params(&self) -> Result<AddBudgetParams, String> {
        let (name, tag, currency, monthly_amount) = self.checked()?;
        Ok(AddBudgetParams {
            name,
            tag,
            currency,
            monthly_amount,
            balance_accounts: self.balance_accounts.clone(),
            create_panels: false,
        })
    }

    pub fn to_update_params(&self) -> Result<UpdateBudgetParams, String> {
        let (name, tag, currency, monthly_amount) = self.checked()?;
        Ok(UpdateBudgetParams {
            budget: self
                .id
                .clone()
                .ok_or("this budget has not been created yet")?,
            name: Some(name),
            tag: Some(tag),
            currency: Some(currency),
            monthly_amount: Some(monthly_amount),
            balance_accounts: Some(self.balance_accounts.clone()),
        })
    }
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
            comment: String::new(),
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

    #[test]
    fn choosing_an_account_adds_its_default_tags_once() {
        let mut form = tx(&[("Expenses:Food", "1", "JPY"), ("Assets:Cash", "-1", "JPY")]);
        form.tags = "weekly".into();
        form.add_tags(&["living".into(), "weekly".into()]);
        assert_eq!(form.tags, "weekly, living");
        form.add_tags(&["living".into()]);
        assert_eq!(
            form.tags, "weekly, living",
            "nothing new, nothing rewritten"
        );
    }

    #[test]
    fn the_form_sends_its_tags_as_shown() {
        let mut form = tx(&[("Expenses:Food", "1", "JPY"), ("Assets:Cash", "-1", "JPY")]);
        form.add_tags(&["living".into()]);
        form.tags = form.tags.replace("living", "");
        let p = form.to_params().unwrap();
        assert_eq!(p.tags, ["food", "weekly"]);
        assert!(
            !p.apply_default_tags,
            "the server must not add back what was removed"
        );
    }

    #[test]
    fn an_account_form_carries_default_tags() {
        let mut form = AccountForm::new("2026-01-01".into());
        form.name = "Expenses:Food".into();
        form.default_tags = "living, , food".into();
        assert_eq!(form.to_params().unwrap().default_tags, ["living", "food"]);
    }

    #[test]
    fn a_tags_form_replaces_the_accounts_default_tags() {
        let account = sapphire_ledger_api::Account {
            id: "acc0001".into(),
            name: "Expenses:Food".into(),
            account_type: AccountType::Expense,
            currencies: vec![],
            opened_at: "2026-01-01".parse().unwrap(),
            closed_at: None,
            default_tags: vec!["living".into()],
            description: None,
        };
        let mut form = TagsForm::new(&account);
        assert_eq!(form.tags, "living");
        form.tags = "".into();
        let p = form.to_params();
        assert_eq!(p.account, "acc0001");
        assert_eq!(p.default_tags, Some(vec![]));
    }

    #[test]
    fn a_budget_form_becomes_add_or_update_parameters() {
        let mut form = BudgetForm::new("JPY");
        assert!(form.to_add_params().unwrap_err().contains("name"));
        form.name = " 生活費 ".into();
        form.tag = "living".into();
        form.monthly_amount = "100,000".into();
        form.toggle("acc0001");
        form.toggle("acc0002");
        form.toggle("acc0002");
        let p = form.to_add_params().unwrap();
        assert_eq!(
            (p.name.as_str(), p.monthly_amount.as_str()),
            ("生活費", "100000")
        );
        assert_eq!(p.balance_accounts, ["acc0001"]);
        form.monthly_amount = "0".into();
        assert!(form.to_add_params().unwrap_err().contains("positive"));
    }

    #[test]
    fn editing_a_budget_starts_from_it_and_updates_every_field() {
        let accounts = [sapphire_ledger_api::Account {
            id: "acc0001".into(),
            name: "Assets:Bank".into(),
            account_type: AccountType::Asset,
            currencies: vec![],
            opened_at: "2026-01-01".parse().unwrap(),
            closed_at: None,
            default_tags: vec![],
            description: None,
        }];
        let budget = sapphire_ledger_api::Budget {
            id: "bud0001".into(),
            name: "生活費".into(),
            tag: "living".into(),
            currency: "JPY".into(),
            monthly_amount: "100000".into(),
            // A hand-written budget naming its account only by name.
            balance_accounts: vec![sapphire_ledger_api::AccountRef {
                account_id: None,
                account_name: Some("Assets:Bank".into()),
            }],
        };
        let form = BudgetForm::edit(&budget, &accounts);
        assert_eq!(form.balance_accounts, ["acc0001"]);
        let p = form.to_update_params().unwrap();
        assert_eq!(p.budget, "bud0001");
        assert_eq!(p.monthly_amount.as_deref(), Some("100000"));
        assert_eq!(p.balance_accounts, Some(vec!["acc0001".to_string()]));
    }
}
