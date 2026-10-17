//! A budget's month, day by day: what was planned against what happened.
//!
//! Pure: `today` is a parameter, so the same books give the same report.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Months, NaiveDate};
use rust_decimal::{Decimal, RoundingStrategy};

use crate::account::{Account, AccountType, resolve_account};
use crate::budget::Budget;
use crate::error::{Error, Result};
use crate::repository::Workspace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetDay {
    pub date: NaiveDate,
    /// The budget's spending dated this day; a refund is negative.
    pub spent: Decimal,
    pub spent_cumulative: Decimal,
    pub budget_cumulative: Decimal,
    pub planned_balance: Decimal,
    /// `date <= today`: the actual lines stop at the last elapsed day.
    pub elapsed: bool,
    /// The balance accounts at the end of this day; `None` when the day is
    /// not elapsed or the budget has no balance accounts.
    pub actual_balance: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetReport {
    pub daily_budget: Decimal,
    pub opening_balance: Decimal,
    /// Every day of the month, in order.
    pub days: Vec<BudgetDay>,
}

/// `YYYY-MM` as (year, month).
pub fn parse_month(raw: &str) -> Result<(i32, u32)> {
    let bad = || Error::Validation(format!("not a YYYY-MM month: {raw}"));
    let (y, m) = raw.split_once('-').ok_or_else(bad)?;
    let (year, month) = (
        y.parse::<i32>().map_err(|_| bad())?,
        m.parse::<u32>().map_err(|_| bad())?,
    );
    NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(bad)?;
    Ok((year, month))
}

/// Two decimal places, midpoint away from zero, trailing zeros dropped: how
/// a report's amounts are answered.
pub fn round_amount(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
        .normalize()
}

pub fn budget_report(
    ws: &Workspace,
    budget: &Budget,
    year: i32,
    month: u32,
    today: NaiveDate,
) -> Result<BudgetReport> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| Error::Validation(format!("no such month {year}-{month:02}")))?;
    let next = first
        .checked_add_months(Months::new(1))
        .ok_or_else(|| Error::Validation(format!("no such month {year}-{month:02}")))?;
    let n = (next - first).num_days() as usize;

    let by_id: HashMap<&str, &Account> = ws.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> =
        ws.accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    let balance_ids: HashSet<&str> = budget
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
        .map(|a| a.id.as_str())
        .collect();

    let mut spent = vec![Decimal::ZERO; n];
    let mut balance_change = vec![Decimal::ZERO; n];
    let mut opening = Decimal::ZERO;
    for tx in &ws.transactions {
        if tx.date >= next {
            continue;
        }
        let tagged = tx.tags.contains(&budget.tag);
        for p in &tx.postings {
            let Ok(account) = resolve_account(
                p.account_id.as_deref(),
                p.account_name.as_deref(),
                &by_id,
                &by_name,
            ) else {
                continue;
            };
            let holds = balance_ids.contains(account.id.as_str()) && p.currency == budget.currency;
            if tx.date < first {
                if holds {
                    opening += p.amount;
                }
                continue;
            }
            let i = tx.date.day0() as usize;
            if holds {
                balance_change[i] += p.amount;
            }
            if tagged && account.account_type == AccountType::Expense {
                let (currency, amount) = p.balance_contribution();
                if currency == budget.currency {
                    spent[i] += amount;
                }
            }
        }
    }

    let total = Decimal::from(n as u64);
    let mut days = Vec::with_capacity(n);
    let (mut spent_so_far, mut balance) = (Decimal::ZERO, opening);
    for i in 0..n {
        let date = first + chrono::Days::new(i as u64);
        spent_so_far += spent[i];
        balance += balance_change[i];
        let budget_cumulative = budget.monthly_amount * Decimal::from(i as u64 + 1) / total;
        let elapsed = date <= today;
        days.push(BudgetDay {
            date,
            spent: spent[i],
            spent_cumulative: spent_so_far,
            budget_cumulative,
            planned_balance: opening - budget_cumulative,
            elapsed,
            actual_balance: (elapsed && !balance_ids.is_empty()).then_some(balance),
        });
    }
    Ok(BudgetReport {
        daily_budget: budget.monthly_amount / total,
        opening_balance: opening,
        days,
    })
}
