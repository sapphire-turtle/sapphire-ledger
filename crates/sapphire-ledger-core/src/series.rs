//! The series query: how much flowed, or what was held, per period, filtered
//! and grouped — and a budget's pro-rated line. Most household charts are
//! made of these, so a chart is a set of queries, not a method of its own.
//!
//! Pure: `today` is a parameter, so the same books give the same answer.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, Days, Months, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::account::{ACCOUNT_NAME_SEPARATOR, Account, AccountType, resolve_account};
use crate::budget::Budget;
use crate::error::{Error, Result};
use crate::report::round_amount;
use crate::repository::Workspace;
use crate::transaction::Transaction;

pub const MAX_BUCKETS: usize = 1000;
pub const MAX_SERIES: usize = 50;
/// The tag group of a transaction with no tags.
pub const UNTAGGED: &str = "(untagged)";
/// The group `top` folds the smaller groups into.
pub const OTHER: &str = "Other";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// What moved in each bucket.
    Flow,
    /// What was held at the end of each bucket.
    Balance,
    /// A budget's monthly amount spread over the days.
    Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interval {
    Day,
    /// Starts on Monday.
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    None,
    Account,
    AccountType,
    Tag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLine {
    /// The pro-rated allowance.
    Pace,
    /// The balance accounts' opening balance minus the cumulative pace.
    Remaining,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One series to compute. Filters combine with AND across fields and OR
/// within a list; an empty list does not filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesQuery {
    pub measure: Measure,
    /// Account ids or names; a name matches that account and every account
    /// under it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_types: Vec<AccountType>,
    /// The transaction carries any of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// A budget's id or name, for a budget series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<BudgetLine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GroupBy>,
    /// With `group_by = account`: group by the first `depth` name segments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Keep the `top` largest groups; sum the rest into [`OTHER`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<u32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cumulative: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub negate: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl SeriesQuery {
    pub fn new(measure: Measure) -> SeriesQuery {
        SeriesQuery {
            measure,
            accounts: vec![],
            account_types: vec![],
            tags: vec![],
            budget: None,
            line: None,
            group_by: None,
            depth: None,
            top: None,
            cumulative: false,
            negate: false,
            label: None,
        }
    }

    pub fn group_by_or_none(&self) -> GroupBy {
        self.group_by.unwrap_or(GroupBy::None)
    }

    pub fn line_or_pace(&self) -> BudgetLine {
        self.line.unwrap_or(BudgetLine::Pace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesRequest {
    /// Both ends included.
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    /// `None`: the workspace's base currency.
    pub currency: Option<String>,
    pub series: Vec<SeriesQuery>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesData {
    /// The [`SeriesQuery`] it came from.
    pub index: usize,
    pub group: Option<String>,
    pub label: String,
    /// One per bucket; `None` for an actual series in a bucket after today.
    pub values: Vec<Option<Decimal>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesResult {
    pub currency: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    /// Each bucket's first date inside the range.
    pub buckets: Vec<NaiveDate>,
    pub series: Vec<SeriesData>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::Validation(message.into())
}

/// Refuse a query whose fields do not fit its measure, so a mistake is not
/// silently ignored.
pub fn check_query(q: &SeriesQuery) -> Result<()> {
    let kind = match q.measure {
        Measure::Flow => "flow",
        Measure::Balance => "balance",
        Measure::Budget => "budget",
    };
    let not_for = |field: &str| invalid(format!("`{field}` does not apply to a {kind} series"));
    match q.measure {
        Measure::Budget => {
            if q.budget.as_deref().is_none_or(|b| b.trim().is_empty()) {
                return Err(invalid("a budget series needs `budget`"));
            }
            for (field, set) in [
                ("accounts", !q.accounts.is_empty()),
                ("account_types", !q.account_types.is_empty()),
                ("tags", !q.tags.is_empty()),
                ("group_by", q.group_by_or_none() != GroupBy::None),
                ("depth", q.depth.is_some()),
                ("top", q.top.is_some()),
            ] {
                if set {
                    return Err(not_for(field));
                }
            }
            if q.cumulative && q.line_or_pace() == BudgetLine::Remaining {
                return Err(invalid(
                    "`cumulative` does not apply to a remaining line: it is always cumulative",
                ));
            }
        }
        Measure::Flow | Measure::Balance => {
            if q.budget.is_some() {
                return Err(not_for("budget"));
            }
            if q.line.is_some() {
                return Err(not_for("line"));
            }
            if q.measure == Measure::Balance && q.cumulative {
                return Err(invalid(
                    "`cumulative` does not apply to a balance series: it is already cumulative",
                ));
            }
            if q.depth.is_some() && q.group_by_or_none() != GroupBy::Account {
                return Err(invalid("`depth` needs group_by = account"));
            }
            if q.top.is_some() && q.group_by_or_none() == GroupBy::None {
                return Err(invalid("`top` needs a group_by"));
            }
        }
    }
    if q.depth == Some(0) || q.top == Some(0) {
        return Err(invalid("`depth` and `top` start at 1"));
    }
    Ok(())
}

fn next_start(d: NaiveDate, interval: Interval) -> Option<NaiveDate> {
    match interval {
        Interval::Day => d.succ_opt(),
        Interval::Week => {
            d.checked_add_days(Days::new(7 - u64::from(d.weekday().num_days_from_monday())))
        }
        Interval::Month => {
            NaiveDate::from_ymd_opt(d.year(), d.month(), 1)?.checked_add_months(Months::new(1))
        }
        Interval::Year => NaiveDate::from_ymd_opt(d.year() + 1, 1, 1),
    }
}

/// The buckets' first dates: calendar-aligned, clipped to `from..=to`.
pub fn buckets(from: NaiveDate, to: NaiveDate, interval: Interval) -> Result<Vec<NaiveDate>> {
    if from > to {
        return Err(invalid(format!("from {from} is after to {to}")));
    }
    let mut starts = vec![from];
    while let Some(next) = next_start(*starts.last().expect("starts is never empty"), interval) {
        if next > to {
            break;
        }
        starts.push(next);
        if starts.len() > MAX_BUCKETS {
            return Err(invalid(format!(
                "{from}..{to} by {interval:?} is more than {MAX_BUCKETS} buckets; \
                 use a longer interval or a shorter range"
            )));
        }
    }
    Ok(starts)
}

/// Each bucket's last date.
fn bucket_ends(starts: &[NaiveDate], to: NaiveDate) -> Vec<NaiveDate> {
    starts
        .iter()
        .skip(1)
        .map(|next| next.pred_opt().expect("a later start has a day before it"))
        .chain(std::iter::once(to))
        .collect()
}

fn bucket_of(starts: &[NaiveDate], date: NaiveDate) -> usize {
    starts.partition_point(|s| *s <= date) - 1
}

/// `name` is `prefix` or under it.
fn under(name: &str, prefix: &str) -> bool {
    name == prefix
        || name
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(ACCOUNT_NAME_SEPARATOR))
}

/// The name an `accounts` entry matches by: the account's name for an id,
/// else the entry itself. `None` when no account is that name or under it.
pub fn account_prefix<'a>(entry: &'a str, accounts: &'a [Account]) -> Option<&'a str> {
    let prefix = accounts
        .iter()
        .find(|a| a.id == entry)
        .map_or(entry, |a| a.name.as_str());
    accounts
        .iter()
        .any(|a| under(&a.name, prefix))
        .then_some(prefix)
}

/// A budget by id, else by name.
pub fn find_budget<'a>(budgets: &'a [Budget], needle: &str) -> Result<&'a Budget> {
    budgets
        .iter()
        .find(|b| b.id == needle)
        .or_else(|| budgets.iter().find(|b| b.name == needle))
        .ok_or_else(|| invalid(format!("no budget {needle}")))
}

fn label(q: &SeriesQuery, group: Option<&str>, default: impl FnOnce() -> String) -> String {
    let parts: Vec<&str> = [q.label.as_deref(), group]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        default()
    } else {
        parts.join(" — ")
    }
}

fn running(values: &mut [Decimal], start: Decimal) {
    let mut total = start;
    for v in values {
        total += *v;
        *v = total;
    }
}

pub fn query_series(ws: &Workspace, req: &SeriesRequest, today: NaiveDate) -> Result<SeriesResult> {
    if req.series.is_empty() {
        return Err(invalid("a query needs at least one series"));
    }
    for q in &req.series {
        check_query(q)?;
    }
    let currency = req
        .currency
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .unwrap_or(&ws.config.base_currency)
        .to_owned();
    let starts = buckets(req.from, req.to, req.interval)?;
    let ends = bucket_ends(&starts, req.to);

    let mut series = Vec::new();
    for (index, q) in req.series.iter().enumerate() {
        let computed = match q.measure {
            Measure::Budget => vec![budget_line(ws, q, &currency, req.from, &starts, &ends)?],
            Measure::Flow | Measure::Balance => {
                actual(ws, q, &currency, req.from, req.to, &starts, today)?
            }
        };
        for (group, label, mut values) in computed {
            if q.negate {
                for v in values.iter_mut().flatten() {
                    *v = -*v;
                }
            }
            series.push(SeriesData {
                index,
                group,
                label,
                values: values.into_iter().map(|v| v.map(round_amount)).collect(),
            });
            if series.len() > MAX_SERIES {
                return Err(invalid(format!(
                    "the query makes more than {MAX_SERIES} series; use `top` or fewer series"
                )));
            }
        }
    }
    Ok(SeriesResult {
        currency,
        from: req.from,
        to: req.to,
        interval: req.interval,
        buckets: starts,
        series,
    })
}

type Computed = (Option<String>, String, Vec<Option<Decimal>>);

fn group_keys(q: &SeriesQuery, account: &Account, tx: &Transaction) -> Vec<String> {
    match q.group_by_or_none() {
        GroupBy::None => vec![String::new()],
        GroupBy::Account => {
            let segments: Vec<&str> = account.name.split(ACCOUNT_NAME_SEPARATOR).collect();
            let depth = q
                .depth
                .map_or(segments.len(), |d| (d as usize).min(segments.len()));
            vec![segments[..depth].join(&ACCOUNT_NAME_SEPARATOR.to_string())]
        }
        GroupBy::AccountType => vec![format!("{:?}", account.account_type)],
        GroupBy::Tag => {
            let tags: Vec<String> = tx
                .tags
                .iter()
                .filter(|t| q.tags.is_empty() || q.tags.contains(t))
                .cloned()
                .collect();
            if tags.is_empty() {
                vec![UNTAGGED.to_owned()]
            } else {
                tags
            }
        }
    }
}

fn actual(
    ws: &Workspace,
    q: &SeriesQuery,
    currency: &str,
    from: NaiveDate,
    to: NaiveDate,
    starts: &[NaiveDate],
    today: NaiveDate,
) -> Result<Vec<Computed>> {
    let by_id: HashMap<&str, &Account> = ws.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> =
        ws.accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    let prefixes = q
        .accounts
        .iter()
        .map(|entry| {
            account_prefix(entry, &ws.accounts)
                .ok_or_else(|| invalid(format!("no account matches {entry}")))
        })
        .collect::<Result<Vec<&str>>>()?;
    let is_balance = q.measure == Measure::Balance;
    let last = to.min(today);
    let n = starts.len();

    let mut opening: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut per_bucket: BTreeMap<String, Vec<Decimal>> = BTreeMap::new();
    for tx in &ws.transactions {
        if tx.date > last || (!is_balance && tx.date < from) {
            continue;
        }
        if !q.tags.is_empty() && !tx.tags.iter().any(|t| q.tags.contains(t)) {
            continue;
        }
        for p in &tx.postings {
            let Ok(account) = resolve_account(
                p.account_id.as_deref(),
                p.account_name.as_deref(),
                &by_id,
                &by_name,
            ) else {
                continue;
            };
            if !q.account_types.is_empty() && !q.account_types.contains(&account.account_type) {
                continue;
            }
            if !prefixes.is_empty() && !prefixes.iter().any(|pre| under(&account.name, pre)) {
                continue;
            }
            let amount = if is_balance {
                (p.currency == currency).then_some(p.amount)
            } else {
                let (c, a) = p.balance_contribution();
                (c == currency).then_some(a)
            };
            let Some(amount) = amount else { continue };
            for key in group_keys(q, account, tx) {
                if tx.date < from {
                    *opening.entry(key).or_default() += amount;
                } else {
                    per_bucket
                        .entry(key)
                        .or_insert_with(|| vec![Decimal::ZERO; n])[bucket_of(starts, tx.date)] +=
                        amount;
                }
            }
        }
    }
    // A plain series answers even with nothing in it; a group with only an
    // opening balance is still a group.
    if q.group_by_or_none() == GroupBy::None {
        per_bucket
            .entry(String::new())
            .or_insert_with(|| vec![Decimal::ZERO; n]);
    }
    for key in opening.keys() {
        per_bucket
            .entry(key.clone())
            .or_insert_with(|| vec![Decimal::ZERO; n]);
    }

    let runs = is_balance || q.cumulative;
    let mut groups: Vec<(String, Vec<Decimal>)> = per_bucket
        .into_iter()
        .map(|(key, mut values)| {
            if runs {
                let start = if is_balance {
                    opening.get(&key).copied().unwrap_or_default()
                } else {
                    Decimal::ZERO
                };
                running(&mut values, start);
            }
            (key, values)
        })
        .collect();
    if let Some(top) = q.top {
        groups = fold_top(groups, top as usize, runs);
    }

    let grouped = q.group_by_or_none() != GroupBy::None;
    let default = if is_balance { "Balance" } else { "Flow" };
    Ok(groups
        .into_iter()
        .map(|(key, values)| {
            let group = grouped.then_some(key);
            let label = label(q, group.as_deref(), || default.to_owned());
            let values = values
                .into_iter()
                .zip(starts)
                .map(|(v, start)| (*start <= today).then_some(v))
                .collect();
            (group, label, values)
        })
        .collect())
}

/// Keep the `top` groups with the largest absolute total — the last value of
/// a running series, the sum of a per-bucket one — in key order, and sum the
/// rest into [`OTHER`], last.
fn fold_top(
    groups: Vec<(String, Vec<Decimal>)>,
    top: usize,
    runs: bool,
) -> Vec<(String, Vec<Decimal>)> {
    if groups.len() <= top {
        return groups;
    }
    let size = |values: &[Decimal]| {
        if runs {
            values.last().copied().unwrap_or_default().abs()
        } else {
            values.iter().copied().sum::<Decimal>().abs()
        }
    };
    let mut ranked: Vec<usize> = (0..groups.len()).collect();
    ranked.sort_by(|a, b| size(&groups[*b].1).cmp(&size(&groups[*a].1)).then(a.cmp(b)));
    let kept: HashSet<usize> = ranked.into_iter().take(top).collect();
    let n = groups.first().map_or(0, |g| g.1.len());
    let mut other = vec![Decimal::ZERO; n];
    let mut out = Vec::with_capacity(top + 1);
    for (i, (key, values)) in groups.into_iter().enumerate() {
        if kept.contains(&i) {
            out.push((key, values));
        } else {
            for (o, v) in other.iter_mut().zip(values) {
                *o += v;
            }
        }
    }
    out.push((OTHER.to_owned(), other));
    out
}

fn days_in_month(d: NaiveDate) -> u32 {
    let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1).expect("a date's month exists");
    match first.checked_add_months(Months::new(1)) {
        Some(next) => (next - first).num_days() as u32,
        None => 31,
    }
}

/// The budget's allowance for `start..=end`: each day's share of its month.
fn pace_between(budget: &Budget, start: NaiveDate, end: NaiveDate) -> Decimal {
    start
        .iter_days()
        .take_while(|d| *d <= end)
        .map(|d| budget.monthly_amount / Decimal::from(days_in_month(d)))
        .sum()
}

fn budget_line(
    ws: &Workspace,
    q: &SeriesQuery,
    currency: &str,
    from: NaiveDate,
    starts: &[NaiveDate],
    ends: &[NaiveDate],
) -> Result<Computed> {
    let budget = find_budget(&ws.budgets, q.budget.as_deref().unwrap_or_default())?;
    if budget.currency != currency {
        return Err(invalid(format!(
            "budget {} is in {}, the query in {currency}",
            budget.name, budget.currency
        )));
    }
    let mut values: Vec<Decimal> = starts
        .iter()
        .zip(ends)
        .map(|(s, e)| pace_between(budget, *s, *e))
        .collect();
    let line = q.line_or_pace();
    match line {
        BudgetLine::Pace => {
            if q.cumulative {
                running(&mut values, Decimal::ZERO);
            }
        }
        BudgetLine::Remaining => {
            if budget.balance_accounts.is_empty() {
                return Err(invalid(format!(
                    "budget {} has no balance accounts to start the remaining line from",
                    budget.name
                )));
            }
            running(&mut values, Decimal::ZERO);
            let opening = budget_opening(ws, budget, from);
            for v in &mut values {
                *v = opening - *v;
            }
        }
    }
    let label = label(q, None, || match line {
        BudgetLine::Pace => format!("{} pace", budget.name),
        BudgetLine::Remaining => format!("{} remaining", budget.name),
    });
    Ok((None, label, values.into_iter().map(Some).collect()))
}

/// The budget's balance accounts (and everything under them) before `from`, in its currency, raw amounts.
fn budget_opening(ws: &Workspace, budget: &Budget, from: NaiveDate) -> Decimal {
    let by_id: HashMap<&str, &Account> = ws.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> =
        ws.accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    // The same rule as a `balance` series: an account that is one of the
    // balance accounts, or under one.
    let names: Vec<&str> = budget
        .balance_accounts
        .iter()
        .filter_map(|r| {
            resolve_account(
                r.account_id.as_deref(),
                r.account_name.as_deref(),
                &by_id,
                &by_name,
            )
            .ok()
        })
        .map(|a| a.name.as_str())
        .collect();
    let mut total = Decimal::ZERO;
    for tx in ws.transactions.iter().filter(|tx| tx.date < from) {
        for p in &tx.postings {
            let Ok(account) = resolve_account(
                p.account_id.as_deref(),
                p.account_name.as_deref(),
                &by_id,
                &by_name,
            ) else {
                continue;
            };
            if names.iter().any(|n| under(&account.name, n)) && p.currency == budget.currency {
                total += p.amount;
            }
        }
    }
    total
}
