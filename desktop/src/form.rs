//! The two entry forms — a transaction and an account — as plain data, and
//! what they turn into.
//!
//! Kept free of egui so the rules are testable. The server checks everything
//! again; these checks only let the window say what is wrong before sending.

use std::collections::BTreeMap;

use rust_decimal::Decimal;
use sapphire_ledger_api::{
    Account, AccountType, AddAccountParams, AddBudgetParams, AddTransactionParams, Budget,
    BudgetLine, GroupBy, Interval, Mark, Measure, PanelDefinition, PanelSeries, Period, Plot,
    PostingParam, SeriesQuery, UpdateAccountParams, UpdateBudgetParams, templates,
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
    /// For a new budget: also make its pace and balance panels.
    pub create_panels: bool,
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
            create_panels: true,
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
            create_panels: false,
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
            create_panels: self.create_panels,
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

/// The editor's view of one series: every field of every measure, as typed;
/// [`PanelForm::to_definition`] keeps only those the measure takes.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesRow {
    pub measure: Measure,
    pub accounts: Vec<String>,
    pub account_types: Vec<AccountType>,
    /// Comma-separated.
    pub tags: String,
    pub budget: Option<String>,
    pub line: BudgetLine,
    pub group_by: GroupBy,
    pub depth: String,
    pub top: String,
    pub cumulative: bool,
    pub negate: bool,
    pub mark: Mark,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlotRow {
    pub title: String,
    pub stacked: bool,
    pub series: Vec<SeriesRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PanelForm {
    /// The panel being edited; `None` for a new one.
    pub id: Option<String>,
    pub name: String,
    pub period: Period,
    /// `None`: the period's default.
    pub interval: Option<Interval>,
    /// Empty: the ledger's base currency.
    pub currency: String,
    pub plots: Vec<PlotRow>,
}

impl SeriesRow {
    pub fn new(measure: Measure) -> SeriesRow {
        SeriesRow {
            measure,
            accounts: vec![],
            account_types: vec![],
            tags: String::new(),
            budget: None,
            line: BudgetLine::Pace,
            group_by: GroupBy::None,
            depth: String::new(),
            top: String::new(),
            cumulative: false,
            negate: false,
            mark: Mark::Line,
            label: String::new(),
        }
    }

    fn from_series(s: &PanelSeries) -> SeriesRow {
        let q = &s.query;
        SeriesRow {
            measure: q.measure,
            accounts: q.accounts.clone(),
            account_types: q.account_types.clone(),
            tags: q.tags.join(", "),
            budget: q.budget.clone(),
            line: q.line.unwrap_or(BudgetLine::Pace),
            group_by: q.group_by.unwrap_or(GroupBy::None),
            depth: q.depth.map(|d| d.to_string()).unwrap_or_default(),
            top: q.top.map(|t| t.to_string()).unwrap_or_default(),
            cumulative: q.cumulative,
            negate: q.negate,
            mark: s.mark.unwrap_or(Mark::Line),
            label: q.label.clone().unwrap_or_default(),
        }
    }

    fn to_series(&self) -> Result<PanelSeries, String> {
        let count = |field: &str, raw: &str| -> Result<Option<u32>, String> {
            match raw.trim() {
                "" => Ok(None),
                t => t
                    .parse::<u32>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .map(Some)
                    .ok_or_else(|| {
                        format!("{field} must be a whole number of 1 or more, not {t:?}")
                    }),
            }
        };
        let label = Some(self.label.trim().to_owned()).filter(|l| !l.is_empty());
        let mut query = SeriesQuery {
            label,
            negate: self.negate,
            ..SeriesQuery::new(self.measure)
        };
        match self.measure {
            Measure::Budget => {
                query.budget = Some(
                    self.budget
                        .clone()
                        .ok_or("a budget series needs a budget")?,
                );
                query.line = Some(self.line);
                query.cumulative = self.cumulative && self.line == BudgetLine::Pace;
            }
            Measure::Flow | Measure::Balance => {
                query.accounts = self.accounts.clone();
                query.account_types = self.account_types.clone();
                query.tags = split_list(&self.tags);
                if self.group_by != GroupBy::None {
                    query.group_by = Some(self.group_by);
                    query.top = count("top", &self.top)?;
                }
                if self.group_by == GroupBy::Account {
                    query.depth = count("depth", &self.depth)?;
                }
                query.cumulative = self.cumulative && self.measure == Measure::Flow;
            }
        }
        let mark = (self.mark == Mark::Bar).then_some(Mark::Bar);
        Ok(PanelSeries { query, mark })
    }
}

impl PanelForm {
    /// One plot holding one flow series.
    pub fn new() -> PanelForm {
        PanelForm {
            id: None,
            name: String::new(),
            period: Period::Month,
            interval: None,
            currency: String::new(),
            plots: vec![PlotRow::new()],
        }
    }

    pub fn from_definition(id: Option<String>, def: &PanelDefinition) -> PanelForm {
        PanelForm {
            id,
            name: def.name.clone(),
            period: def.period,
            interval: def.interval,
            currency: def.currency.clone().unwrap_or_default(),
            plots: def
                .plots
                .iter()
                .map(|p| PlotRow {
                    title: p.title.clone().unwrap_or_default(),
                    stacked: p.stacked,
                    series: p.series.iter().map(SeriesRow::from_series).collect(),
                })
                .collect(),
        }
    }

    pub fn to_definition(&self) -> Result<PanelDefinition, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("a panel needs a name".into());
        }
        let plots = self
            .plots
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if p.series.is_empty() {
                    return Err(format!("plot {} has no series", i + 1));
                }
                Ok(Plot {
                    title: Some(p.title.trim().to_owned()).filter(|t| !t.is_empty()),
                    stacked: p.stacked,
                    series: p
                        .series
                        .iter()
                        .map(SeriesRow::to_series)
                        .collect::<Result<_, _>>()
                        .map_err(|e| format!("plot {}: {e}", i + 1))?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        if plots.is_empty() {
            return Err("a panel needs a plot".into());
        }
        Ok(PanelDefinition {
            name: name.to_owned(),
            period: self.period,
            interval: self.interval,
            currency: Some(self.currency.trim().to_owned()).filter(|c| !c.is_empty()),
            plots,
        })
    }
}

impl PlotRow {
    pub fn new() -> PlotRow {
        PlotRow {
            title: String::new(),
            stacked: false,
            series: vec![SeriesRow::new(Measure::Flow)],
        }
    }
}

/// The editor's ready-made panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    BudgetPace,
    BudgetBalance,
    ExpensesByAccount,
    NetWorth,
    IncomeAndExpenses,
}

impl Template {
    pub const ALL: [Template; 5] = [
        Template::BudgetPace,
        Template::BudgetBalance,
        Template::ExpensesByAccount,
        Template::NetWorth,
        Template::IncomeAndExpenses,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Template::BudgetPace => "Budget pace",
            Template::BudgetBalance => "Budget balance",
            Template::ExpensesByAccount => "Expenses by account",
            Template::NetWorth => "Net worth",
            Template::IncomeAndExpenses => "Income and expenses",
        }
    }

    pub fn needs_budget(self) -> bool {
        matches!(self, Template::BudgetPace | Template::BudgetBalance)
    }

    /// `None` when a budget template has no budget, or (balance) the budget
    /// has no accounts it is paid from.
    pub fn build(self, budget: Option<&Budget>) -> Option<PanelDefinition> {
        match self {
            Template::BudgetPace => budget.map(templates::budget_pace),
            Template::BudgetBalance => budget.and_then(templates::budget_balance),
            Template::ExpensesByAccount => Some(templates::expenses_by_account()),
            Template::NetWorth => Some(templates::net_worth()),
            Template::IncomeAndExpenses => Some(templates::income_and_expenses()),
        }
    }
}

/// What an account filter can name: every account, and every name above one.
pub fn account_choices(accounts: &[Account]) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for a in accounts {
        let segments: Vec<&str> = a.name.split(':').collect();
        for depth in 1..=segments.len() {
            names.insert(segments[..depth].join(":"));
        }
    }
    names.into_iter().collect()
}

/// Move item `i` one place up or down, staying in bounds.
pub fn move_item<T>(items: &mut [T], i: usize, up: bool) {
    if up && i > 0 {
        items.swap(i, i - 1);
    } else if !up && i + 1 < items.len() {
        items.swap(i, i + 1);
    }
}

impl Default for PanelForm {
    fn default() -> PanelForm {
        PanelForm::new()
    }
}

impl Default for PlotRow {
    fn default() -> PlotRow {
        PlotRow::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_budget_makes_its_panels_unless_told_not_to() {
        let mut form = BudgetForm::new("JPY");
        form.name = "生活費".into();
        form.tag = "living".into();
        form.monthly_amount = "100000".into();
        assert!(form.to_add_params().unwrap().create_panels);
        form.create_panels = false;
        assert!(!form.to_add_params().unwrap().create_panels);
    }

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

    fn account(id: &str, name: &str) -> Account {
        Account {
            id: id.into(),
            name: name.into(),
            account_type: sapphire_ledger_api::AccountType::Asset,
            currencies: vec![],
            opened_at: "2026-01-01".parse().unwrap(),
            closed_at: None,
            default_tags: vec![],
            description: None,
        }
    }

    fn budget_with_account() -> Budget {
        Budget {
            id: "b1".into(),
            name: "生活費".into(),
            tag: "living".into(),
            currency: "JPY".into(),
            monthly_amount: "100000".into(),
            balance_accounts: vec![sapphire_ledger_api::AccountRef {
                account_id: Some("acc".into()),
                account_name: Some("Assets:Bank".into()),
            }],
        }
    }

    use sapphire_ledger_api::{BudgetLine, GroupBy, Interval, Mark, Measure, Period, templates};

    #[test]
    fn a_definition_survives_the_form() {
        for def in [
            templates::expenses_by_account(),
            templates::net_worth(),
            templates::income_and_expenses(),
        ] {
            assert_eq!(
                PanelForm::from_definition(None, &def)
                    .to_definition()
                    .unwrap(),
                def
            );
        }
        let b = budget_with_account();
        let def = templates::budget_balance(&b).unwrap();
        assert_eq!(
            PanelForm::from_definition(Some("p".into()), &def)
                .to_definition()
                .unwrap(),
            def
        );
    }

    #[test]
    fn a_switched_measure_leaves_its_old_fields_behind() {
        let mut form = PanelForm::from_definition(None, &templates::expenses_by_account());
        let row = &mut form.plots[0].series[0];
        row.measure = Measure::Budget;
        row.budget = Some("b1".into());
        let def = form.to_definition().unwrap();
        let q = &def.plots[0].series[0].query;
        assert!(
            q.account_types.is_empty()
                && q.group_by.is_none()
                && q.depth.is_none()
                && q.top.is_none()
        );
        assert_eq!(q.budget.as_deref(), Some("b1"));
        assert_eq!(q.line, Some(BudgetLine::Pace));
    }

    #[test]
    fn the_form_says_what_is_wrong() {
        let mut form = PanelForm::new();
        assert!(form.to_definition().unwrap_err().contains("name"));
        form.name = "A".into();
        form.plots[0].series[0].group_by = GroupBy::Account;
        form.plots[0].series[0].depth = "two".into();
        assert!(form.to_definition().unwrap_err().contains("depth"));
        form.plots[0].series[0].depth = String::new();
        form.plots[0].series[0].measure = Measure::Budget;
        assert!(form.to_definition().unwrap_err().contains("budget"));
    }

    #[test]
    fn a_new_form_is_one_plot_of_one_flow() {
        let form = PanelForm::new();
        assert_eq!(form.period, Period::Month);
        assert_eq!(form.plots.len(), 1);
        assert_eq!(form.plots[0].series[0].measure, Measure::Flow);
        assert_eq!(form.plots[0].series[0].mark, Mark::Line);
        assert_eq!(form.interval, None::<Interval>);
    }

    #[test]
    fn account_choices_include_the_parents() {
        let accounts = vec![
            account("a1", "Expenses:Food:Lunch"),
            account("a2", "Assets:Bank"),
        ];
        assert_eq!(
            account_choices(&accounts),
            [
                "Assets",
                "Assets:Bank",
                "Expenses",
                "Expenses:Food",
                "Expenses:Food:Lunch"
            ]
        );
    }

    #[test]
    fn templates_that_need_a_budget_say_so() {
        let b = budget_with_account();
        assert!(Template::BudgetPace.needs_budget());
        assert!(Template::BudgetPace.build(None).is_none());
        assert_eq!(
            Template::BudgetPace.build(Some(&b)).unwrap().name,
            "生活費 pace"
        );
        assert_eq!(Template::NetWorth.build(None).unwrap().name, "Net worth");
        assert_eq!(Template::ALL.len(), 5);
    }

    #[test]
    fn items_move_within_bounds() {
        let mut v = vec![1, 2, 3];
        move_item(&mut v, 0, true);
        assert_eq!(v, [1, 2, 3]);
        move_item(&mut v, 0, false);
        assert_eq!(v, [2, 1, 3]);
        move_item(&mut v, 2, true);
        assert_eq!(v, [2, 3, 1]);
    }
}
