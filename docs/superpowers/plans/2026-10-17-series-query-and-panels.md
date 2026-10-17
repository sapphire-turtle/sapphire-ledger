# Series Query and Panels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace #43's chart-specific `budget_report` with one generic series query, panel records that save a chart's queries, and a desktop Charts screen (with an editor) built on them — so a new chart needs no API change.

**Architecture:** `sapphire-ledger-core` gains `series.rs` (the pure query engine) and `panel.rs` (the record, its period, `run_panel`), loaded with the workspace and written through `ops`. `sapphire-ledger-api` mirrors the types for the wire and holds the panel templates. `sapphire-ledger-mcp` serves six methods and tools; the CLI gets `panel list|run`; the desktop draws `run_panel` and edits panels. `budget_report` is removed everywhere.

**Tech Stack:** Rust 2024, serde + toml 1.1, chrono, rust_decimal, schemars 1.0, rmcp, clap, egui 0.36 / egui_plot 0.37.

**Spec:** `docs/superpowers/specs/2026-10-17-series-query-and-panels-design.md`

## Global Constraints

- `sapphire-ledger-api` stays at version **2.0.0**; `API_VERSION` stays 2.
- One currency per query; default `config.base_currency`. Postings in other currencies are ignored.
- Limits: at most **1000 buckets** and **50 result series** (counting groups) per query.
- Amounts in results: `report::round_amount` (2 dp, midpoint away from zero, normalized). On the wire, amounts are strings.
- Group keys: `(untagged)` for a transaction with no tags, `Other` for what `top` folds. Group series ordered by key; `Other` last.
- Label join: the query's `label`, then the group, joined by ` — ` (space, em dash, space). Defaults: `Flow`, `Balance`, `<budget name> pace`, `<budget name> remaining`.
- Template panel names: `<budget name> pace`, `<budget name> balance`.
- Ledger MCP tools: **19** (plus the framework's 7).
- Update ops rewrite the file from its known fields: unknown keys and comments are not kept.
- Desktop categorical colours, in order (light / dark): blue `#2a78d6`/`#3987e5`, orange `#eb6834`/`#d95926`, aqua `#1baf7a`/`#199e70`, yellow `#eda100`/`#c98500`, magenta `#e87ba4`/`#d55181`, green `#008300`/`#008300`, violet `#6250d6`/`#9085e9`, red `#e34948`/`#e66767`; a plot's 9th+ series grey `#8a8a85`. Budget series dashed.
- Commit trailer: `Co-Authored-By: <the model writing the commit> <noreply@anthropic.com>`.
- Run `cargo fmt` before each commit; `cargo clippy --workspace --all-targets -- -D warnings` must stay clean.

## Review Focus

1. A budget template made for a budget with **no balance accounts** — must not create a balance panel that errors on every run (pinned in Task 3's template test and Task 2's `create_budget_with_panels` test).
2. An editor row whose measure was switched (e.g. flow → budget) still holding the old measure's fields — `to_definition` must drop them, or the server refuses the save (pinned in Task 7).
3. `group_by = tag` with a `tags` filter — a transaction tagged `living, eatout` filtered on `living` must not make an `eatout` group (pinned in Task 1).
4. A panel file written by hand with an unknown budget or account name — `run_panel` errors with that name and `validate_workspace` reports it, not a panic or a blank chart (pinned in Task 2).
5. Week buckets starting mid-week, and offsets crossing a year boundary — exact dates (pinned in Tasks 1 and 2).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/sapphire-ledger-core/src/series.rs` (new) | Query types, `check_query`, buckets, `query_series` |
| `crates/sapphire-ledger-core/src/panel.rs` (new) | Panel record, `Period`, `panel_range`, `check_panel`, `run_panel`, `find_panel` |
| `crates/sapphire-ledger-core/src/{workspace,repository,ops,validate,lib}.rs` | `panels/` dir, loading, write ops, issues, exports |
| `crates/sapphire-ledger-core/src/report.rs` | Shrinks to `round_amount` (Task 5) |
| `crates/sapphire-ledger-api/src/series.rs` (new) | Wire mirror of the query and panel types, params and results |
| `crates/sapphire-ledger-api/src/templates.rs` (new) | The five panel templates |
| `crates/sapphire-ledger-mcp/src/{wire,methods,tools}.rs` | Conversions, methods, tools |
| `cli/src/{cli,commands,main}.rs` | `panel list`, `panel run`; `budget` removed |
| `desktop/src/charts.rs` | Charts screen on panels, budgets window, editor window |
| `desktop/src/form.rs` | `BudgetForm.create_panels`, `PanelForm` |

---

### Task 1: Core series query engine

**Files:**
- Create: `crates/sapphire-ledger-core/src/series.rs`
- Modify: `crates/sapphire-ledger-core/src/lib.rs` (add `pub mod series;`)
- Test: `crates/sapphire-ledger-core/tests/series.rs` (new)

**Interfaces:**
- Consumes: `report::round_amount(Decimal) -> Decimal`, `account::{Account, AccountType, ACCOUNT_NAME_SEPARATOR, resolve_account}`, `Posting::balance_contribution() -> (String, Decimal)`, `Workspace { config.base_currency, accounts, transactions, budgets }`.
- Produces (all `pub` in `sapphire_ledger_core::series`): enums `Measure {Flow, Balance, Budget}`, `Interval {Day, Week, Month, Year}`, `GroupBy {None, Account, AccountType, Tag}`, `BudgetLine {Pace, Remaining}` (all `Copy`, serde `snake_case`); `struct SeriesQuery` (fields below) with `SeriesQuery::new(Measure)`, `group_by_or_none()`, `line_or_pace()`; `struct SeriesRequest { from, to: NaiveDate, interval: Interval, currency: Option<String>, series: Vec<SeriesQuery> }`; `struct SeriesData { index: usize, group: Option<String>, label: String, values: Vec<Option<Decimal>> }`; `struct SeriesResult { currency: String, from, to: NaiveDate, interval: Interval, buckets: Vec<NaiveDate>, series: Vec<SeriesData> }`; `fn check_query(&SeriesQuery) -> Result<()>`; `fn buckets(NaiveDate, NaiveDate, Interval) -> Result<Vec<NaiveDate>>`; `fn account_prefix<'a>(&'a str, &'a [Account]) -> Option<&'a str>`; `fn find_budget<'a>(&'a [Budget], &str) -> Result<&'a Budget>`; `fn query_series(&Workspace, &SeriesRequest, today: NaiveDate) -> Result<SeriesResult>`; consts `MAX_BUCKETS = 1000`, `MAX_SERIES = 50`, `UNTAGGED = "(untagged)"`, `OTHER = "Other"`.

- [ ] **Step 1: Write the failing tests** — `crates/sapphire-ledger-core/tests/series.rs`:

```rust
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sapphire_ledger_core::series::{
    BudgetLine, GroupBy, Interval, Measure, SeriesQuery, SeriesRequest, buckets, check_query,
    query_series,
};
use sapphire_ledger_core::{AccountRef, AccountType, Posting, Workspace, load_workspace, ops};

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn vals(raw: &[Option<&str>]) -> Vec<Option<Decimal>> {
    raw.iter().map(|v| v.map(|s| s.parse().unwrap())).collect()
}

fn posting(account: &str, amount: &str, currency: &str) -> Posting {
    Posting {
        account_id: None,
        account_name: Some(account.into()),
        amount: amount.parse().unwrap(),
        currency: currency.into(),
        price: None,
        memo: None,
    }
}

const TODAY: &str = "2026-10-04";

/// Salary in September; food (living), lunch (living, eatout), rent
/// (untagged) on Oct 1–3; food after "today"; a USD expense to ignore.
struct Books(tempfile::TempDir);

impl Books {
    fn new() -> Books {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sapphire_ledger_core::init_workspace(root, "JPY").unwrap();
        for (name, ty) in [
            ("Assets:Bank", AccountType::Asset),
            ("Assets:Wallet", AccountType::Asset),
            ("Liabilities:Card", AccountType::Liability),
            ("Expenses:Food", AccountType::Expense),
            ("Expenses:Food:Lunch", AccountType::Expense),
            ("Expenses:Rent", AccountType::Expense),
            ("Income:Salary", AccountType::Income),
        ] {
            ops::create_account(root, name.into(), ty, vec![], d("2026-01-01"), None, vec![])
                .unwrap();
        }
        ops::create_budget(
            root,
            "生活費".into(),
            "living".into(),
            "JPY".into(),
            "100000".parse().unwrap(),
            vec![AccountRef {
                account_id: None,
                account_name: Some("Assets:Bank".into()),
            }],
        )
        .unwrap();
        ops::create_budget(
            root,
            "Loose".into(),
            "loose".into(),
            "JPY".into(),
            "1000".parse().unwrap(),
            vec![],
        )
        .unwrap();
        let books = Books(dir);
        books.tx("2026-09-30", &[], ("Assets:Bank", "300000"), ("Income:Salary", "-300000"), "JPY");
        books.tx("2026-10-01", &["living"], ("Expenses:Food", "1000"), ("Assets:Bank", "-1000"), "JPY");
        books.tx(
            "2026-10-02",
            &["living", "eatout"],
            ("Expenses:Food:Lunch", "500"),
            ("Assets:Wallet", "-500"),
            "JPY",
        );
        books.tx("2026-10-03", &[], ("Expenses:Rent", "80000"), ("Assets:Bank", "-80000"), "JPY");
        books.tx("2026-10-05", &["living"], ("Expenses:Food", "300"), ("Assets:Bank", "-300"), "JPY");
        books.tx("2026-10-02", &["living"], ("Expenses:Food", "10"), ("Liabilities:Card", "-10"), "USD");
        books
    }

    fn tx(&self, date: &str, tags: &[&str], a: (&str, &str), b: (&str, &str), currency: &str) {
        ops::create_transaction(
            self.0.path(),
            d(date),
            "t".into(),
            None,
            tags.iter().map(|t| t.to_string()).collect(),
            None,
            vec![posting(a.0, a.1, currency), posting(b.0, b.1, currency)],
            None,
            false,
        )
        .unwrap();
    }

    fn ws(&self) -> Workspace {
        load_workspace(self.0.path()).unwrap()
    }
}

fn request(from: &str, to: &str, interval: Interval, series: Vec<SeriesQuery>) -> SeriesRequest {
    SeriesRequest {
        from: d(from),
        to: d(to),
        interval,
        currency: None,
        series,
    }
}

fn expenses() -> SeriesQuery {
    SeriesQuery {
        account_types: vec![AccountType::Expense],
        ..SeriesQuery::new(Measure::Flow)
    }
}

#[test]
fn flow_sums_per_day_in_the_base_currency_and_stops_at_today() {
    let ws = Books::new().ws();
    let r = query_series(&ws, &request("2026-10-01", "2026-10-07", Interval::Day, vec![expenses()]), d(TODAY)).unwrap();
    assert_eq!(r.currency, "JPY");
    assert_eq!(r.buckets.len(), 7);
    assert_eq!(r.series.len(), 1);
    assert_eq!(r.series[0].label, "Flow");
    assert_eq!(r.series[0].group, None);
    assert_eq!(
        r.series[0].values,
        vals(&[Some("1000"), Some("500"), Some("80000"), Some("0"), None, None, None])
    );
}

#[test]
fn filters_match_tags_types_and_accounts_under_a_name() {
    let books = Books::new();
    let ws = books.ws();
    let run = |q: SeriesQuery| {
        query_series(&ws, &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]), d(TODAY))
            .map(|r| r.series[0].values.clone())
    };
    let living = SeriesQuery { tags: vec!["living".into()], ..expenses() };
    assert_eq!(run(living).unwrap(), vals(&[Some("1000"), Some("500"), Some("0"), Some("0")]));
    let food = SeriesQuery { accounts: vec!["Expenses:Food".into()], ..SeriesQuery::new(Measure::Flow) };
    assert_eq!(run(food).unwrap(), vals(&[Some("1000"), Some("500"), Some("0"), Some("0")]));
    let lunch = SeriesQuery { accounts: vec!["Expenses:Food:Lunch".into()], ..SeriesQuery::new(Measure::Flow) };
    assert_eq!(run(lunch).unwrap(), vals(&[Some("0"), Some("500"), Some("0"), Some("0")]));
    // No account is named exactly `Expenses`; the name still matches what is under it.
    let all = SeriesQuery { accounts: vec!["Expenses".into()], ..SeriesQuery::new(Measure::Flow) };
    assert_eq!(run(all).unwrap(), vals(&[Some("1000"), Some("500"), Some("80000"), Some("0")]));
    let food_id = ws.accounts.iter().find(|a| a.name == "Expenses:Food").unwrap().id.clone();
    let by_id = SeriesQuery { accounts: vec![food_id], ..SeriesQuery::new(Measure::Flow) };
    assert_eq!(run(by_id).unwrap(), vals(&[Some("1000"), Some("500"), Some("0"), Some("0")]));
    let nope = SeriesQuery { accounts: vec!["Nope".into()], ..SeriesQuery::new(Measure::Flow) };
    assert!(run(nope).unwrap_err().to_string().contains("no account matches Nope"));
}

#[test]
fn balance_starts_from_everything_before_the_range() {
    let ws = Books::new().ws();
    let bank = SeriesQuery { accounts: vec!["Assets:Bank".into()], ..SeriesQuery::new(Measure::Balance) };
    let assets = SeriesQuery {
        account_types: vec![AccountType::Asset, AccountType::Liability],
        label: Some("Net worth".into()),
        ..SeriesQuery::new(Measure::Balance)
    };
    let r = query_series(&ws, &request("2026-10-01", "2026-10-05", Interval::Day, vec![bank, assets]), d(TODAY)).unwrap();
    assert_eq!(r.series[0].values, vals(&[Some("299000"), Some("299000"), Some("219000"), Some("219000"), None]));
    // The USD card posting is not in JPY, so it is not counted.
    assert_eq!(r.series[1].values, vals(&[Some("299000"), Some("298500"), Some("218500"), Some("218500"), None]));
    assert_eq!(r.series[1].label, "Net worth");
    assert_eq!(r.series[1].index, 1);
}

#[test]
fn group_by_account_at_a_depth_orders_by_name() {
    let ws = Books::new().ws();
    let q = SeriesQuery { group_by: Some(GroupBy::Account), depth: Some(2), label: Some("Spent".into()), ..expenses() };
    let r = query_series(&ws, &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]), d(TODAY)).unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["Expenses:Food", "Expenses:Rent"]);
    assert_eq!(r.series[0].label, "Spent — Expenses:Food");
    assert_eq!(r.series[0].values, vals(&[Some("1000"), Some("500"), Some("0"), Some("0")]));
    assert_eq!(r.series[1].values, vals(&[Some("0"), Some("0"), Some("80000"), Some("0")]));
}

#[test]
fn group_by_tag_counts_each_tag_and_the_untagged() {
    let ws = Books::new().ws();
    let q = SeriesQuery { group_by: Some(GroupBy::Tag), cumulative: true, ..expenses() };
    let r = query_series(&ws, &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]), d(TODAY)).unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["(untagged)", "eatout", "living"]);
    assert_eq!(r.series[0].values, vals(&[Some("0"), Some("0"), Some("80000"), Some("80000")]));
    assert_eq!(r.series[1].values, vals(&[Some("0"), Some("500"), Some("500"), Some("500")]));
    assert_eq!(r.series[2].values, vals(&[Some("1000"), Some("1500"), Some("1500"), Some("1500")]));
    assert_eq!(r.series[2].label, "living");

    // Filtered on `living`, the lunch's other tag makes no group of its own.
    let q = SeriesQuery { group_by: Some(GroupBy::Tag), tags: vec!["living".into()], ..expenses() };
    let r = query_series(&ws, &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]), d(TODAY)).unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["living"]);
}

#[test]
fn top_keeps_the_largest_and_folds_the_rest_into_other() {
    let ws = Books::new().ws();
    let q = SeriesQuery { group_by: Some(GroupBy::Account), top: Some(1), ..expenses() };
    let r = query_series(&ws, &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]), d(TODAY)).unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["Expenses:Rent", "Other"]);
    assert_eq!(r.series[1].values, vals(&[Some("1000"), Some("500"), Some("0"), Some("0")]));
}

#[test]
fn group_by_account_type_and_negate() {
    let ws = Books::new().ws();
    let income = SeriesQuery {
        account_types: vec![AccountType::Income],
        negate: true,
        label: Some("Income".into()),
        ..SeriesQuery::new(Measure::Flow)
    };
    let by_type = SeriesQuery { group_by: Some(GroupBy::AccountType), ..SeriesQuery::new(Measure::Flow) };
    let r = query_series(&ws, &request("2026-09-01", "2026-10-31", Interval::Month, vec![income, by_type]), d(TODAY)).unwrap();
    assert_eq!(r.buckets, [d("2026-09-01"), d("2026-10-01")]);
    assert_eq!(r.series[0].values, vals(&[Some("300000"), Some("0")]));
    let groups: Vec<_> = r.series[1..].iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["Asset", "Expense", "Income"]);
}

#[test]
fn budget_pace_follows_each_months_length() {
    let ws = Books::new().ws();
    let pace = SeriesQuery { budget: Some("生活費".into()), ..SeriesQuery::new(Measure::Budget) };
    let r = query_series(&ws, &request("2026-01-01", "2026-03-31", Interval::Month, vec![pace.clone()]), d(TODAY)).unwrap();
    assert_eq!(r.series[0].values, vals(&[Some("100000"), Some("100000"), Some("100000")]));
    assert_eq!(r.series[0].label, "生活費 pace");
    let cumulative = SeriesQuery { cumulative: true, ..pace.clone() };
    let r = query_series(&ws, &request("2026-02-01", "2026-02-28", Interval::Day, vec![pace, cumulative]), d(TODAY)).unwrap();
    assert_eq!(r.series[0].values[0], Some("3571.43".parse().unwrap()));
    assert_eq!(r.series[1].values[27], Some("100000".parse().unwrap()));
    // Budget lines are drawn past today.
    assert!(r.series[0].values.iter().all(Option::is_some));
}

#[test]
fn budget_remaining_starts_from_the_balance_accounts() {
    let ws = Books::new().ws();
    let remaining = SeriesQuery {
        budget: Some("生活費".into()),
        line: Some(BudgetLine::Remaining),
        ..SeriesQuery::new(Measure::Budget)
    };
    let r = query_series(&ws, &request("2026-10-01", "2026-10-03", Interval::Day, vec![remaining]), d(TODAY)).unwrap();
    assert_eq!(r.series[0].values, vals(&[Some("296774.19"), Some("293548.39"), Some("290322.58")]));
    assert_eq!(r.series[0].label, "生活費 remaining");

    let loose = SeriesQuery { budget: Some("Loose".into()), line: Some(BudgetLine::Remaining), ..SeriesQuery::new(Measure::Budget) };
    let err = query_series(&ws, &request("2026-10-01", "2026-10-03", Interval::Day, vec![loose]), d(TODAY)).unwrap_err();
    assert!(err.to_string().contains("no balance accounts"), "{err}");

    let usd = SeriesRequest {
        currency: Some("USD".into()),
        ..request("2026-10-01", "2026-10-03", Interval::Day, vec![SeriesQuery { budget: Some("生活費".into()), ..SeriesQuery::new(Measure::Budget) }])
    };
    let err = query_series(&ws, &usd, d(TODAY)).unwrap_err();
    assert!(err.to_string().contains("is in JPY"), "{err}");

    let missing = SeriesQuery { budget: Some("nope".into()), ..SeriesQuery::new(Measure::Budget) };
    let err = query_series(&ws, &request("2026-10-01", "2026-10-03", Interval::Day, vec![missing]), d(TODAY)).unwrap_err();
    assert!(err.to_string().contains("no budget nope"), "{err}");
}

#[test]
fn a_currency_other_than_the_base_reads_only_that_currency() {
    let ws = Books::new().ws();
    let usd = SeriesRequest {
        currency: Some("USD".into()),
        ..request("2026-10-01", "2026-10-03", Interval::Day, vec![expenses()])
    };
    let r = query_series(&ws, &usd, d(TODAY)).unwrap();
    assert_eq!(r.currency, "USD");
    assert_eq!(r.series[0].values, vals(&[Some("0"), Some("10"), Some("0")]));
}

#[test]
fn week_buckets_start_on_monday_and_are_clipped_to_the_range() {
    // 2026-10-01 is a Thursday.
    assert_eq!(
        buckets(d("2026-10-01"), d("2026-10-14"), Interval::Week).unwrap(),
        [d("2026-10-01"), d("2026-10-05"), d("2026-10-12")]
    );
    assert_eq!(
        buckets(d("2026-11-15"), d("2027-02-01"), Interval::Year).unwrap(),
        [d("2026-11-15"), d("2027-01-01")]
    );
    assert!(buckets(d("2026-10-02"), d("2026-10-01"), Interval::Day).is_err());
}

#[test]
fn the_limits_are_errors() {
    let ws = Books::new().ws();
    let err = query_series(&ws, &request("2020-01-01", "2026-01-01", Interval::Day, vec![expenses()]), d(TODAY)).unwrap_err();
    assert!(err.to_string().contains("1000"), "{err}");
    let many = vec![expenses(); 51];
    let err = query_series(&ws, &request("2026-10-01", "2026-10-02", Interval::Day, many), d(TODAY)).unwrap_err();
    assert!(err.to_string().contains("50"), "{err}");
    let err = query_series(&ws, &request("2026-10-01", "2026-10-02", Interval::Day, vec![]), d(TODAY)).unwrap_err();
    assert!(err.to_string().contains("at least one series"), "{err}");
}

#[test]
fn a_field_that_does_not_apply_is_an_error() {
    let budget = || SeriesQuery { budget: Some("b".into()), ..SeriesQuery::new(Measure::Budget) };
    let msg = |q: SeriesQuery| check_query(&q).unwrap_err().to_string();
    assert!(msg(SeriesQuery { tags: vec!["x".into()], ..budget() }).contains("`tags` does not apply to a budget series"));
    assert!(msg(SeriesQuery::new(Measure::Budget)).contains("needs `budget`"));
    assert!(msg(SeriesQuery { line: Some(BudgetLine::Remaining), cumulative: true, ..budget() }).contains("always cumulative"));
    assert!(msg(SeriesQuery { budget: Some("b".into()), ..expenses() }).contains("`budget` does not apply to a flow series"));
    assert!(msg(SeriesQuery { cumulative: true, ..SeriesQuery::new(Measure::Balance) }).contains("already cumulative"));
    assert!(msg(SeriesQuery { depth: Some(2), ..expenses() }).contains("`depth` needs group_by = account"));
    assert!(msg(SeriesQuery { top: Some(2), ..expenses() }).contains("`top` needs a group_by"));
    assert!(msg(SeriesQuery { group_by: Some(GroupBy::Account), depth: Some(0), ..expenses() }).contains("start at 1"));
    assert!(check_query(&SeriesQuery { group_by: Some(GroupBy::Account), depth: Some(2), top: Some(7), ..expenses() }).is_ok());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-core --test series`
Expected: FAIL to compile — `unresolved import sapphire_ledger_core::series`.

- [ ] **Step 3: Implement** — `crates/sapphire-ledger-core/src/series.rs`:

```rust
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
        Interval::Month => NaiveDate::from_ymd_opt(d.year(), d.month(), 1)?
            .checked_add_months(Months::new(1)),
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
        per_bucket.entry(String::new()).or_insert_with(|| vec![Decimal::ZERO; n]);
    }
    for key in opening.keys() {
        per_bucket.entry(key.clone()).or_insert_with(|| vec![Decimal::ZERO; n]);
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
fn fold_top(groups: Vec<(String, Vec<Decimal>)>, top: usize, runs: bool) -> Vec<(String, Vec<Decimal>)> {
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

/// The budget's balance accounts before `from`, in its currency, raw amounts.
fn budget_opening(ws: &Workspace, budget: &Budget, from: NaiveDate) -> Decimal {
    let by_id: HashMap<&str, &Account> = ws.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> =
        ws.accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    let ids: HashSet<&str> = budget
        .balance_accounts
        .iter()
        .filter_map(|r| {
            resolve_account(r.account_id.as_deref(), r.account_name.as_deref(), &by_id, &by_name)
                .ok()
        })
        .map(|a| a.id.as_str())
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
            if ids.contains(account.id.as_str()) && p.currency == budget.currency {
                total += p.amount;
            }
        }
    }
    total
}
```

Add `pub mod series;` to `crates/sapphire-ledger-core/src/lib.rs` (alphabetical, after `pub mod repository;`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p sapphire-ledger-core --test series`
Expected: PASS (13 tests). Then `cargo test -p sapphire-ledger-core` — all pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/sapphire-ledger-core/src/series.rs crates/sapphire-ledger-core/src/lib.rs crates/sapphire-ledger-core/tests/series.rs
git commit -m "feat(core): a generic series query — flow, balance and budget lines"
```

---

### Task 2: Core panels — record, period, run, ops, validation

**Files:**
- Create: `crates/sapphire-ledger-core/src/panel.rs`
- Modify: `crates/sapphire-ledger-core/src/workspace.rs` (`PANELS_DIR`, `panel_relative_path`), `repository.rs` (`Workspace.panels`, load), `ops.rs` (`create_panel`, `update_panel`, `delete_panel`, `create_budget_with_panels`), `validate.rs` (panel issues), `lib.rs` (module + exports)
- Modify: `crates/sapphire-ledger-core/tests/account_refs.rs:47` (the `Workspace { … }` literal gains `panels: vec![]`)
- Test: `crates/sapphire-ledger-core/tests/panels.rs` (new)

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces (`sapphire_ledger_core::panel`): `enum Period {Month, Year, Last12Months}` (serde: `month`, `year`, `last_12_months`); `enum Mark {Line, Bar}` (snake_case); `struct PanelSeries { #[serde(flatten)] query: SeriesQuery, mark: Option<Mark> }`; `struct Plot { title: Option<String>, stacked: bool, series: Vec<PanelSeries> }`; `struct PanelDefinition { name: String, period: Period, interval: Option<Interval>, currency: Option<String>, plots: Vec<Plot> }`; `struct Panel { id, name, period, interval, currency, created_at, updated_at, plots }` with `Panel::definition(&self) -> PanelDefinition`; `fn default_interval(Period) -> Interval`; `fn panel_range(Period, anchor: NaiveDate, offset: i32) -> Result<(NaiveDate, NaiveDate)>`; `fn check_panel(&PanelDefinition) -> Result<()>`; `fn find_panel<'a>(&'a [Panel], &str) -> Result<&'a Panel>`; `struct PanelRun { name, period, from, to, interval, currency, buckets: Vec<NaiveDate>, plots: Vec<PlotRun> }`, `struct PlotRun { title: Option<String>, stacked: bool, series: Vec<SeriesRun> }`, `struct SeriesRun { label: String, group: Option<String>, mark: Mark, dashed: bool, values: Vec<Option<Decimal>> }`; `fn run_panel(&Workspace, &PanelDefinition, anchor: NaiveDate, offset: i32, today: NaiveDate) -> Result<PanelRun>`.
- Produces (`ops`): `create_panel(root: &Path, def: PanelDefinition) -> Result<(String, PathBuf)>`; `update_panel(root, panel: &str, def: PanelDefinition) -> Result<(String, PathBuf)>`; `delete_panel(root, panel: &str) -> Result<(String, PathBuf)>`; `create_budget_with_panels(root, name, tag, currency, monthly_amount, balance_accounts, panels: impl FnOnce(&Budget) -> Result<Vec<PanelDefinition>>) -> Result<(String, PathBuf, Vec<(String, PathBuf)>)>`. `create_budget` keeps its signature and calls it with `|_| Ok(vec![])`.
- Produces: `Workspace.panels: Vec<Panel>`; `workspace::{PANELS_DIR = "panels", panel_relative_path(id) -> PathBuf}` (`panels/{id}.toml`); lib re-exports `Panel, PanelDefinition, PanelSeries, Plot, Period, Mark, PANELS_DIR, panel_relative_path`.

- [ ] **Step 1: Write the failing tests** — `crates/sapphire-ledger-core/tests/panels.rs`:

```rust
use chrono::NaiveDate;
use sapphire_ledger_core::panel::{Mark, PanelSeries, Period, Plot, panel_range, run_panel};
use sapphire_ledger_core::series::{BudgetLine, Measure, SeriesQuery};
use sapphire_ledger_core::{AccountRef, AccountType, PanelDefinition, load_workspace, ops};

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn ledger() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    sapphire_ledger_core::init_workspace(dir.path(), "JPY").unwrap();
    for (name, ty) in [("Assets:Bank", AccountType::Asset), ("Expenses:Food", AccountType::Expense)] {
        ops::create_account(dir.path(), name.into(), ty, vec![], d("2026-01-01"), None, vec![])
            .unwrap();
    }
    dir
}

fn spent() -> PanelSeries {
    PanelSeries {
        query: SeriesQuery {
            account_types: vec![AccountType::Expense],
            label: Some("Spent".into()),
            ..SeriesQuery::new(Measure::Flow)
        },
        mark: Some(Mark::Bar),
    }
}

fn definition(name: &str) -> PanelDefinition {
    PanelDefinition {
        name: name.into(),
        period: Period::Month,
        interval: None,
        currency: None,
        plots: vec![Plot { title: Some("Daily".into()), stacked: false, series: vec![spent()] }],
    }
}

#[test]
fn a_period_and_offset_name_a_range() {
    let a = d("2026-10-17");
    assert_eq!(panel_range(Period::Month, a, 0).unwrap(), (d("2026-10-01"), d("2026-10-31")));
    assert_eq!(panel_range(Period::Month, a, -10).unwrap(), (d("2025-12-01"), d("2025-12-31")));
    assert_eq!(panel_range(Period::Month, a, 4).unwrap(), (d("2027-02-01"), d("2027-02-28")));
    assert_eq!(panel_range(Period::Year, a, -1).unwrap(), (d("2025-01-01"), d("2025-12-31")));
    assert_eq!(panel_range(Period::Last12Months, a, 0).unwrap(), (d("2025-11-01"), d("2026-10-31")));
    assert_eq!(panel_range(Period::Last12Months, a, -1).unwrap(), (d("2025-10-01"), d("2026-09-30")));
    assert!(panel_range(Period::Year, a, i32::MAX).is_err());
}

#[test]
fn a_panel_round_trips_through_its_file() {
    let dir = ledger();
    let mut def = definition("Food");
    def.plots[0].series.push(PanelSeries {
        query: SeriesQuery { accounts: vec!["Expenses:Food".into()], cumulative: true, ..SeriesQuery::new(Measure::Flow) },
        mark: None,
    });
    let (id, path) = ops::create_panel(dir.path(), def.clone()).unwrap();
    assert_eq!(path, dir.path().join("panels").join(format!("{id}.toml")));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[[plots.series]]"), "{text}");
    assert!(text.contains("period = \"month\""), "{text}");
    let ws = load_workspace(dir.path()).unwrap();
    assert_eq!(ws.panels.len(), 1);
    assert_eq!(ws.panels[0].id, id);
    assert_eq!(ws.panels[0].definition(), def);
}

#[test]
fn a_bad_definition_is_refused() {
    let dir = ledger();
    ops::create_panel(dir.path(), definition("Food")).unwrap();
    let err = |def| ops::create_panel(dir.path(), def).unwrap_err().to_string();
    assert!(err(definition("Food")).contains("a panel named Food already exists"));
    assert!(err(definition(" ")).contains("needs a name"));
    assert!(err(PanelDefinition { plots: vec![], ..definition("A") }).contains("has no plots"));
    let mut empty = definition("B");
    empty.plots[0].series.clear();
    assert!(err(empty).contains("plot 1 has no series"));
    let mut wrong = definition("C");
    wrong.plots[0].series[0].query = SeriesQuery { budget: Some("b".into()), tags: vec!["x".into()], ..SeriesQuery::new(Measure::Budget) };
    assert!(err(wrong).contains("panel C: plot 1: `tags` does not apply to a budget series"));
}

#[test]
fn update_keeps_the_id_and_delete_removes_the_file() {
    let dir = ledger();
    let (id, path) = ops::create_panel(dir.path(), definition("Food")).unwrap();
    ops::create_panel(dir.path(), definition("Other")).unwrap();
    let created = load_workspace(dir.path()).unwrap().panels.into_iter().find(|p| p.id == id).unwrap();
    let (same, _) = ops::update_panel(dir.path(), "Food", definition("Groceries")).unwrap();
    assert_eq!(same, id);
    let ws = load_workspace(dir.path()).unwrap();
    let updated = ws.panels.iter().find(|p| p.id == id).unwrap();
    assert_eq!(updated.name, "Groceries");
    assert_eq!(updated.created_at, created.created_at);
    let err = ops::update_panel(dir.path(), &id, definition("Other")).unwrap_err();
    assert!(err.to_string().contains("a panel named Other already exists"));
    let (gone, gone_path) = ops::delete_panel(dir.path(), "Groceries").unwrap();
    assert_eq!((gone, gone_path), (id, path.clone()));
    assert!(!path.exists());
    assert!(ops::delete_panel(dir.path(), "Groceries").unwrap_err().to_string().contains("no panel Groceries"));
}

#[test]
fn a_budget_and_its_panels_are_written_together_or_not_at_all() {
    let dir = ledger();
    let bank = vec![AccountRef { account_id: None, account_name: Some("Assets:Bank".into()) }];
    let panels = |b: &sapphire_ledger_core::Budget| {
        Ok(vec![PanelDefinition {
            name: format!("{} pace", b.name),
            plots: vec![Plot {
                title: None,
                stacked: false,
                series: vec![PanelSeries {
                    query: SeriesQuery { budget: Some(b.id.clone()), line: Some(BudgetLine::Pace), ..SeriesQuery::new(Measure::Budget) },
                    mark: None,
                }],
            }],
            ..definition("x")
        }])
    };
    let (id, _, made) = ops::create_budget_with_panels(
        dir.path(), "生活費".into(), "living".into(), "JPY".into(), "100000".parse().unwrap(), bank.clone(), panels,
    )
    .unwrap();
    assert_eq!(made.len(), 1);
    let ws = load_workspace(dir.path()).unwrap();
    assert_eq!(ws.panels[0].name, "生活費 pace");
    assert_eq!(ws.panels[0].plots[0].series[0].query.budget.as_deref(), Some(id.as_str()));

    // A taken panel name: neither the budget nor its panel is written.
    let err = ops::create_budget_with_panels(
        dir.path(), "生活費 2".into(), "x".into(), "JPY".into(), "1".parse().unwrap(), bank,
        |_| Ok(vec![definition("生活費 pace")]),
    )
    .unwrap_err();
    assert!(err.to_string().contains("a panel named 生活費 pace already exists"), "{err}");
    let ws = load_workspace(dir.path()).unwrap();
    assert_eq!(ws.budgets.len(), 1);
    assert_eq!(ws.panels.len(), 1);
}

#[test]
fn validate_reports_a_panel_naming_what_is_not_there() {
    let dir = ledger();
    let mut def = definition("Broken");
    def.plots[0].series.push(PanelSeries {
        query: SeriesQuery { budget: Some("nope".into()), ..SeriesQuery::new(Measure::Budget) },
        mark: None,
    });
    def.plots[0].series.push(PanelSeries {
        query: SeriesQuery { accounts: vec!["Nowhere".into()], ..SeriesQuery::new(Measure::Flow) },
        mark: None,
    });
    ops::create_panel(dir.path(), def).unwrap();
    let ws = load_workspace(dir.path()).unwrap();
    let messages: Vec<String> = ws.validate().into_iter().map(|i| i.message).collect();
    assert!(messages.contains(&"panel Broken: no budget nope".to_string()), "{messages:?}");
    assert!(messages.contains(&"panel Broken: no account matches Nowhere".to_string()), "{messages:?}");
    let err = run_panel(&ws, &ws.panels[0].definition(), d("2026-10-17"), 0, d("2026-10-17")).unwrap_err();
    assert!(err.to_string().contains("no budget nope"), "{err}");
}

#[test]
fn run_panel_places_each_series_in_its_plot() {
    let dir = ledger();
    ops::create_budget(dir.path(), "生活費".into(), "living".into(), "JPY".into(), "31000".parse().unwrap(), vec![]).unwrap();
    let def = PanelDefinition {
        plots: vec![
            Plot { title: Some("Daily".into()), stacked: true, series: vec![spent()] },
            Plot {
                title: None,
                stacked: false,
                series: vec![PanelSeries {
                    query: SeriesQuery { budget: Some("生活費".into()), cumulative: true, ..SeriesQuery::new(Measure::Budget) },
                    mark: None,
                }],
            },
        ],
        ..definition("Two")
    };
    let ws = load_workspace(dir.path()).unwrap();
    let run = run_panel(&ws, &def, d("2026-10-17"), -1, d("2026-10-17")).unwrap();
    assert_eq!((run.from, run.to), (d("2026-09-01"), d("2026-09-30")));
    assert_eq!(run.buckets.len(), 30, "a month panel defaults to days");
    assert_eq!(run.currency, "JPY");
    assert_eq!(run.plots.len(), 2);
    assert!(run.plots[0].stacked);
    assert_eq!(run.plots[0].series[0].mark, Mark::Bar);
    assert!(!run.plots[0].series[0].dashed);
    assert_eq!(run.plots[1].series[0].mark, Mark::Line);
    assert!(run.plots[1].series[0].dashed);
    assert_eq!(run.plots[1].series[0].values[29], Some("31000".parse().unwrap()));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-core --test panels`
Expected: FAIL to compile — `unresolved import sapphire_ledger_core::panel`.

- [ ] **Step 3: Implement `panel.rs`** — `crates/sapphire-ledger-core/src/panel.rs`:

```rust
//! A panel: a saved chart — its period and, per plot, the series queries to
//! draw. A new chart is a new file, not a new method; an agent runs the same
//! files as query presets.

use chrono::{DateTime, Datelike, FixedOffset, Months, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::repository::Workspace;
use crate::series::{Interval, Measure, SeriesQuery, SeriesRequest, check_query, query_series};

/// The range a panel shows, relative to an anchor date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Period {
    #[serde(rename = "month")]
    Month,
    #[serde(rename = "year")]
    Year,
    #[serde(rename = "last_12_months")]
    Last12Months,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Line,
    Bar,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// A series as a panel keeps it: the query, and how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSeries {
    #[serde(flatten)]
    pub query: SeriesQuery,
    /// Default line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<Mark>,
}

/// One plot: its own y-axis, the x-axis shared with the panel's other plots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Stack the plot's bars.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stacked: bool,
    pub series: Vec<PanelSeries>,
}

/// What a panel is, without its identity: what `create_panel` and
/// `update_panel` take, and what `run_panel` runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelDefinition {
    pub name: String,
    pub period: Period,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub plots: Vec<Plot>,
}

/// `panels/{id}.toml`. Plots last, so TOML writes `[[plots]]` after the
/// plain keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub name: String,
    pub period: Period,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
    pub plots: Vec<Plot>,
}

impl Panel {
    pub fn definition(&self) -> PanelDefinition {
        PanelDefinition {
            name: self.name.clone(),
            period: self.period,
            interval: self.interval,
            currency: self.currency.clone(),
            plots: self.plots.clone(),
        }
    }
}

pub fn default_interval(period: Period) -> Interval {
    match period {
        Period::Month => Interval::Day,
        Period::Year | Period::Last12Months => Interval::Month,
    }
}

fn shift_months(first: NaiveDate, by: i32) -> Option<NaiveDate> {
    if by >= 0 {
        first.checked_add_months(Months::new(by as u32))
    } else {
        first.checked_sub_months(Months::new(by.unsigned_abs()))
    }
}

fn month_end(first: NaiveDate) -> Option<NaiveDate> {
    first.checked_add_months(Months::new(1))?.pred_opt()
}

/// The dates `period` covers around `anchor`, moved by `offset` periods
/// (a month for `last_12_months`).
pub fn panel_range(period: Period, anchor: NaiveDate, offset: i32) -> Result<(NaiveDate, NaiveDate)> {
    let out_of_range = || Error::Validation(format!("offset {offset} is out of range"));
    let this_month = NaiveDate::from_ymd_opt(anchor.year(), anchor.month(), 1)
        .expect("a date's month exists");
    match period {
        Period::Month => {
            let first = shift_months(this_month, offset).ok_or_else(out_of_range)?;
            Ok((first, month_end(first).ok_or_else(out_of_range)?))
        }
        Period::Year => {
            let year = anchor.year().checked_add(offset).ok_or_else(out_of_range)?;
            let first = NaiveDate::from_ymd_opt(year, 1, 1).ok_or_else(out_of_range)?;
            let last = NaiveDate::from_ymd_opt(year, 12, 31).ok_or_else(out_of_range)?;
            Ok((first, last))
        }
        Period::Last12Months => {
            let end = shift_months(this_month, offset).ok_or_else(out_of_range)?;
            let first = shift_months(end, -11).ok_or_else(out_of_range)?;
            Ok((first, month_end(end).ok_or_else(out_of_range)?))
        }
    }
}

fn message(e: Error) -> String {
    match e {
        Error::Validation(m) => m,
        other => other.to_string(),
    }
}

/// Refuse a definition that cannot be drawn: no name, no plots, a plot
/// without series, or a series whose fields do not fit its measure.
pub fn check_panel(def: &PanelDefinition) -> Result<()> {
    let name = def.name.trim();
    if name.is_empty() {
        return Err(Error::Validation("a panel needs a name".into()));
    }
    if def.plots.is_empty() {
        return Err(Error::Validation(format!("panel {name} has no plots")));
    }
    for (i, plot) in def.plots.iter().enumerate() {
        if plot.series.is_empty() {
            return Err(Error::Validation(format!("panel {name}: plot {} has no series", i + 1)));
        }
        for s in &plot.series {
            check_query(&s.query).map_err(|e| {
                Error::Validation(format!("panel {name}: plot {}: {}", i + 1, message(e)))
            })?;
        }
    }
    Ok(())
}

/// A panel by id, else by name.
pub fn find_panel<'a>(panels: &'a [Panel], needle: &str) -> Result<&'a Panel> {
    panels
        .iter()
        .find(|p| p.id == needle)
        .or_else(|| panels.iter().find(|p| p.name == needle))
        .ok_or_else(|| Error::Validation(format!("no panel {needle}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesRun {
    pub label: String,
    pub group: Option<String>,
    pub mark: Mark,
    /// A planned line (a budget's), drawn dashed.
    pub dashed: bool,
    pub values: Vec<Option<Decimal>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlotRun {
    pub title: Option<String>,
    pub stacked: bool,
    pub series: Vec<SeriesRun>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRun {
    pub name: String,
    pub period: Period,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    pub currency: String,
    pub buckets: Vec<NaiveDate>,
    pub plots: Vec<PlotRun>,
}

/// Run a panel's queries as one query, each result placed in its plot.
pub fn run_panel(
    ws: &Workspace,
    def: &PanelDefinition,
    anchor: NaiveDate,
    offset: i32,
    today: NaiveDate,
) -> Result<PanelRun> {
    check_panel(def)?;
    let (from, to) = panel_range(def.period, anchor, offset)?;
    let interval = def.interval.unwrap_or_else(|| default_interval(def.period));
    let sources: Vec<(usize, &PanelSeries)> = def
        .plots
        .iter()
        .enumerate()
        .flat_map(|(i, plot)| plot.series.iter().map(move |s| (i, s)))
        .collect();
    let result = query_series(
        ws,
        &SeriesRequest {
            from,
            to,
            interval,
            currency: def.currency.clone(),
            series: sources.iter().map(|(_, s)| s.query.clone()).collect(),
        },
        today,
    )?;
    let mut plots: Vec<PlotRun> = def
        .plots
        .iter()
        .map(|p| PlotRun { title: p.title.clone(), stacked: p.stacked, series: vec![] })
        .collect();
    for data in result.series {
        let (plot, source) = sources[data.index];
        plots[plot].series.push(SeriesRun {
            label: data.label,
            group: data.group,
            mark: source.mark.unwrap_or(Mark::Line),
            dashed: source.query.measure == Measure::Budget,
            values: data.values,
        });
    }
    Ok(PanelRun {
        name: def.name.clone(),
        period: def.period,
        from,
        to,
        interval,
        currency: result.currency,
        buckets: result.buckets,
        plots,
    })
}
```

- [ ] **Step 4: Wire the record into the workspace**

`workspace.rs`, beside `BUDGETS_DIR` and `budget_relative_path`:

```rust
pub const PANELS_DIR: &str = "panels";

/// Relative path for a panel: `panels/{id}.toml`.
pub fn panel_relative_path(id: &str) -> PathBuf {
    PathBuf::from(PANELS_DIR).join(format!("{id}.{TOML_EXTENSION}"))
}
```

`repository.rs`: import `crate::panel::Panel` and `PANELS_DIR`; add `pub panels: Vec<Panel>,` after `budgets` in `Workspace`; in `load_workspace`, after budgets:

```rust
    let panels = walk_toml_files(&root.join(PANELS_DIR))?
        .iter()
        .map(|p| load_toml::<Panel>(p))
        .collect::<Result<Vec<_>>>()?;
```

and `panels,` in the returned `Workspace`. `tests/account_refs.rs:47`: add `panels: vec![],` to the literal.

`lib.rs`: `pub mod panel;` (after `pub mod ops;`), `pub use panel::{Mark, Panel, PanelDefinition, PanelSeries, Period, Plot};`, and add `PANELS_DIR, panel_relative_path` to the `workspace` re-export.

- [ ] **Step 5: Write ops** — in `ops.rs`, import `crate::panel::{Panel, PanelDefinition, check_panel}` and `panel_relative_path`, `PANELS_DIR`; add `use std::fs;` if absent. Replace the body of `create_budget` with a call to the new function, and add after `update_budget`:

```rust
/// Write a new budget. Its name must be unused: it is what people type.
pub fn create_budget(
    root: &Path,
    name: String,
    tag: String,
    currency: String,
    monthly_amount: Decimal,
    balance_accounts: Vec<AccountRef>,
) -> Result<(String, PathBuf)> {
    create_budget_with_panels(root, name, tag, currency, monthly_amount, balance_accounts, |_| {
        Ok(vec![])
    })
    .map(|(id, path, _)| (id, path))
}

/// Write a new budget and the panels `panels` makes for it. Every name is
/// checked before anything is written: a taken panel name writes nothing.
pub fn create_budget_with_panels(
    root: &Path,
    name: String,
    tag: String,
    currency: String,
    monthly_amount: Decimal,
    balance_accounts: Vec<AccountRef>,
    panels: impl FnOnce(&Budget) -> Result<Vec<PanelDefinition>>,
) -> Result<(String, PathBuf, Vec<(String, PathBuf)>)> {
    // … the existing create_budget body up to and including `check_budget(&budget)?;`,
    // unchanged, but WITHOUT `save_toml(&dest, &budget)?;` …
    let definitions = panels(&budget)?;
    let existing_panels = load_panels(root)?;
    let mut names: HashSet<String> = existing_panels.iter().map(|(p, _)| p.name.clone()).collect();
    let mut ids: HashSet<String> = existing_panels.iter().map(|(p, _)| p.id.clone()).collect();
    let mut planned = Vec::with_capacity(definitions.len());
    for def in definitions {
        let def = prepared(def)?;
        if !names.insert(def.name.clone()) {
            return Err(Error::Validation(format!("a panel named {} already exists", def.name)));
        }
        let panel_id = free_random_id(&ids, "panel")?;
        ids.insert(panel_id.clone());
        planned.push(panel_record(panel_id, def, timestamp));
    }
    save_toml(&dest, &budget)?;
    let mut written = Vec::with_capacity(planned.len());
    for panel in planned {
        let path = root.join(panel_relative_path(&panel.id));
        refuse_existing(&path)?;
        save_toml(&path, &panel)?;
        written.push((panel.id, path));
    }
    Ok((id, dest, written))
}

/// Every panel on disk, with its path.
fn load_panels(root: &Path) -> Result<Vec<(Panel, PathBuf)>> {
    walk_files(&root.join(PANELS_DIR), TOML_EXTENSION)?
        .into_iter()
        .map(|p| Ok((load_toml::<Panel>(&p)?, p)))
        .collect()
}

/// A random id not in `taken`.
fn free_random_id(taken: &HashSet<String>, kind: &str) -> Result<String> {
    (0..ID_ATTEMPTS)
        .map(|_| new_random_id())
        .find(|candidate| !taken.contains(candidate))
        .ok_or_else(|| {
            Error::Validation(format!("could not mint a free {kind} id after {ID_ATTEMPTS} attempts"))
        })
}

/// Trim the name and check the definition.
fn prepared(mut def: PanelDefinition) -> Result<PanelDefinition> {
    def.name = def.name.trim().to_owned();
    def.currency = def.currency.map(|c| c.trim().to_owned()).filter(|c| !c.is_empty());
    check_panel(&def)?;
    Ok(def)
}

fn panel_record(id: String, def: PanelDefinition, timestamp: DateTime<FixedOffset>) -> Panel {
    Panel {
        id,
        name: def.name,
        period: def.period,
        interval: def.interval,
        currency: def.currency,
        created_at: timestamp,
        updated_at: timestamp,
        plots: def.plots,
    }
}

fn find_panel_file<'a>(existing: &'a [(Panel, PathBuf)], needle: &str) -> Result<&'a (Panel, PathBuf)> {
    existing
        .iter()
        .find(|(p, _)| p.id == needle)
        .or_else(|| existing.iter().find(|(p, _)| p.name == needle))
        .ok_or_else(|| Error::Validation(format!("no panel {needle}")))
}

/// Write a new panel. Its name must be unused: `run_panel` takes it.
pub fn create_panel(root: &Path, def: PanelDefinition) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let def = prepared(def)?;
    let existing = load_panels(root)?;
    if existing.iter().any(|(p, _)| p.name == def.name) {
        return Err(Error::Validation(format!("a panel named {} already exists", def.name)));
    }
    let taken: HashSet<String> = existing.iter().map(|(p, _)| p.id.clone()).collect();
    let id = free_random_id(&taken, "panel")?;
    let dest = root.join(panel_relative_path(&id));
    refuse_existing(&dest)?;
    save_toml(&dest, &panel_record(id.clone(), def, now()))?;
    Ok((id, dest))
}

/// Replace a panel's definition, found by id or name; its id and
/// `created_at` stay. The file is rewritten from the known fields, so
/// unknown keys and comments in it are not kept.
pub fn update_panel(root: &Path, panel: &str, def: PanelDefinition) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let def = prepared(def)?;
    let existing = load_panels(root)?;
    let (found, path) = find_panel_file(&existing, panel)?.clone();
    if existing.iter().any(|(p, _)| p.name == def.name && p.id != found.id) {
        return Err(Error::Validation(format!("a panel named {} already exists", def.name)));
    }
    let mut record = panel_record(found.id.clone(), def, now());
    record.created_at = found.created_at;
    save_toml(&path, &record)?;
    Ok((found.id, path))
}

/// Remove a panel, found by id or name.
pub fn delete_panel(root: &Path, panel: &str) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let existing = load_panels(root)?;
    let (found, path) = find_panel_file(&existing, panel)?.clone();
    fs::remove_file(&path)?;
    Ok((found.id, path))
}
```

In `create_budget_with_panels`, the existing body's `let timestamp = now();` stays and is reused for the panels. Its id minting may switch to `free_random_id` (collect `taken` as `HashSet<String>`); keep the error text "could not mint a free budget id …" either way.

- [ ] **Step 6: Validation** — in `validate.rs`, after the budgets loop and before `issues`:

```rust
        let mut seen_panel_ids: HashSet<&str> = HashSet::new();
        let mut seen_panel_names: HashSet<&str> = HashSet::new();
        for panel in &self.panels {
            let issue = |message: String| ValidationIssue {
                message,
                transaction_id: None,
                assertion_id: None,
                account: None,
            };
            if !seen_panel_ids.insert(panel.id.as_str()) {
                issues.push(issue(format!("duplicate panel id {}", panel.id)));
            }
            if !seen_panel_names.insert(panel.name.as_str()) {
                issues.push(issue(format!("duplicate panel name {}", panel.name)));
            }
            if let Err(err) = crate::panel::check_panel(&panel.definition()) {
                issues.push(issue(render(&err)));
            }
            for s in panel.plots.iter().flat_map(|p| &p.series) {
                if let Some(budget) = &s.query.budget
                    && crate::series::find_budget(&self.budgets, budget).is_err()
                {
                    issues.push(issue(format!("panel {}: no budget {budget}", panel.name)));
                }
                for entry in &s.query.accounts {
                    if crate::series::account_prefix(entry, &self.accounts).is_none() {
                        issues.push(issue(format!("panel {}: no account matches {entry}", panel.name)));
                    }
                }
            }
        }
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p sapphire-ledger-core`
Expected: PASS, including the 7 new tests in `tests/panels.rs` and the existing budget tests.

If `toml` refuses to (de)serialize the `#[serde(flatten)]` in `PanelSeries`, do not weaken the test and do not copy `SeriesQuery`'s fields into `PanelSeries`: report BLOCKED with the error.

- [ ] **Step 8: Commit**

```bash
cargo fmt
git add crates/sapphire-ledger-core
git commit -m "feat(core): panels — saved charts of series queries, with their ops and checks"
```

---

### Task 3: API — wire types, templates, constants

**Files:**
- Create: `crates/sapphire-ledger-api/src/series.rs`, `crates/sapphire-ledger-api/src/templates.rs`
- Modify: `crates/sapphire-ledger-api/src/lib.rs`, `src/records.rs` (`AccountType` derives `schemars::JsonSchema`), `src/params.rs` (`AddBudgetParams.create_panels`)
- Modify (struct literals gain `create_panels: false`): `crates/sapphire-ledger-mcp/src/methods.rs` (test helper `add_budget`), `cli/tests/mcp_tools.rs`, `desktop/src/form.rs` (`BudgetForm::to_add_params`)

**Interfaces:**
- Consumes: nothing from earlier tasks (the api crate does not depend on core). Its JSON must match core's serde exactly — Task 4 converts by serde round trip.
- Produces (`sapphire_ledger_api`, re-exported at the root): enums `Measure`, `Interval`, `GroupBy`, `BudgetLine`, `Period`, `Mark` (same variants and serde names as core); `SeriesQuery` (same fields as core, plus `SeriesQuery::new(Measure)`); `PanelSeries { #[serde(flatten)] query: SeriesQuery, mark: Option<Mark> }`; `Plot { title, stacked, series }`; `PanelDefinition { name, period, interval, currency, plots }`; params `QuerySeriesParams { from: String, to: String, interval: Interval, currency: Option<String>, series: Vec<SeriesQuery> }`, `RunPanelParams { panel: Option<String>, definition: Option<PanelDefinition>, anchor: Option<String>, offset: Option<i32> }`, `AddPanelParams { definition: PanelDefinition }`, `UpdatePanelParams { panel: String, definition: PanelDefinition }`, `DeletePanelParams { panel: String }`; records `SeriesResult { currency, from: NaiveDate, to: NaiveDate, interval, buckets: Vec<NaiveDate>, series: Vec<SeriesData> }`, `SeriesData { index: usize, group: Option<String>, label: String, values: Vec<Option<String>> }`, `Panel { id, name, period, interval, currency, created_at, updated_at, plots }` with `Panel::definition()`, `PanelRun { panel: Option<String>, name, period, from, to, interval, currency, buckets, plots: Vec<PlotRun> }`, `PlotRun { title: Option<String>, stacked: bool, series: Vec<SeriesRun> }`, `SeriesRun { label, group: Option<String>, mark: Mark, dashed: bool, values: Vec<Option<String>> }`; `templates::{budget_pace(&Budget) -> PanelDefinition, budget_balance(&Budget) -> Option<PanelDefinition>, expenses_by_account(), net_worth(), income_and_expenses() -> PanelDefinition}`; constants `QUERY_SERIES = "sapphire-ledger.query_series"`, `LIST_PANELS = "sapphire-ledger.list_panels"`, `RUN_PANEL = "sapphire-ledger.run_panel"`, `ADD_PANEL = "sapphire-ledger.add_panel"`, `UPDATE_PANEL = "sapphire-ledger.update_panel"`, `DELETE_PANEL = "sapphire-ledger.delete_panel"`; `AddBudgetParams.create_panels: bool` (`#[serde(default)]`).
- `BUDGET_REPORT`, `BudgetReport`, `BudgetDay`, `BudgetReportParams` stay until Task 6.

- [ ] **Step 1: Write the failing tests** — at the bottom of the new `series.rs` and `templates.rs` (written in Step 3), these `#[cfg(test)]` modules:

`series.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_reads_as_a_hand_written_one() {
        let json = r#"{
            "name": "Food", "period": "last_12_months",
            "plots": [{ "series": [
                { "measure": "flow", "account_types": ["Expense"], "group_by": "account_type", "mark": "bar" },
                { "measure": "budget", "budget": "b", "line": "remaining" }
            ]}]
        }"#;
        let def: PanelDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(def.period, Period::Last12Months);
        let s = &def.plots[0].series[0];
        assert_eq!(s.query.group_by, Some(GroupBy::AccountType));
        assert_eq!(s.mark, Some(Mark::Bar));
        assert!(!def.plots[0].stacked);
        let back = serde_json::to_value(&def).unwrap();
        assert_eq!(back["plots"][0]["series"][1]["line"], "remaining");
        // Defaults are left out, as a person would write it.
        assert!(back["plots"][0]["series"][0].get("cumulative").is_none());
        assert!(back.get("interval").is_none());
    }

    #[test]
    fn run_panel_takes_a_name_or_a_definition() {
        let p: RunPanelParams = serde_json::from_str(r#"{"panel":"生活費 pace","offset":-1}"#).unwrap();
        assert_eq!(p.offset, Some(-1));
        assert!(p.definition.is_none());
    }

    #[test]
    fn the_params_have_object_schemas() {
        for schema in [
            serde_json::to_value(schemars::schema_for!(QuerySeriesParams)).unwrap(),
            serde_json::to_value(schemars::schema_for!(RunPanelParams)).unwrap(),
            serde_json::to_value(schemars::schema_for!(AddPanelParams)).unwrap(),
        ] {
            assert_eq!(schema["type"], "object", "{schema}");
        }
    }
}
```

`templates.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountRef, Measure};

    fn budget(accounts: Vec<AccountRef>) -> Budget {
        Budget {
            id: "b1".into(),
            name: "生活費".into(),
            tag: "living".into(),
            currency: "JPY".into(),
            monthly_amount: "100000".into(),
            balance_accounts: accounts,
        }
    }

    #[test]
    fn the_budget_templates_follow_the_budget() {
        let b = budget(vec![AccountRef { account_id: Some("acc".into()), account_name: Some("Assets:Bank".into()) }]);
        let pace = budget_pace(&b);
        assert_eq!(pace.name, "生活費 pace");
        assert_eq!(pace.currency.as_deref(), Some("JPY"));
        assert_eq!(pace.plots.len(), 2);
        assert_eq!(pace.plots[0].series[0].query.tags, ["living"]);
        assert!(pace.plots[0].series[1].query.cumulative);
        assert_eq!(pace.plots[0].series[1].query.budget.as_deref(), Some("b1"));
        assert_eq!(pace.plots[1].series[0].mark, Some(Mark::Bar));
        let balance = budget_balance(&b).expect("it has an account to start from");
        assert_eq!(balance.name, "生活費 balance");
        assert_eq!(balance.plots[0].series[0].query.accounts, ["acc"]);
        assert_eq!(balance.plots[0].series[1].query.line, Some(BudgetLine::Remaining));
        // Nothing to start a remaining line from: no balance panel.
        assert!(budget_balance(&budget(vec![])).is_none());
    }

    #[test]
    fn the_general_templates_are_what_the_spec_lists() {
        let e = expenses_by_account();
        let s = &e.plots[0].series[0];
        assert_eq!((s.query.group_by, s.query.depth, s.query.top), (Some(GroupBy::Account), Some(2), Some(7)));
        assert!(e.plots[0].stacked);
        assert_eq!(net_worth().plots[0].series[0].query.measure, Measure::Balance);
        let ie = income_and_expenses();
        assert!(ie.plots[0].series[0].query.negate);
        assert_eq!(ie.period, Period::Last12Months);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-api`
Expected: FAIL to compile (the modules do not exist yet — write the test modules together with Step 3's code; a compile failure here is the red step).

- [ ] **Step 3: Implement**

`records.rs`: `AccountType` gains `schemars::JsonSchema` in its derive list.

`params.rs`, `AddBudgetParams` gains, after `balance_accounts`:

```rust
    /// Also create the panels `<name> pace` and, when it has balance
    /// accounts, `<name> balance`. If either name is taken, nothing is written.
    #[serde(default)]
    pub create_panels: bool,
```

`crates/sapphire-ledger-api/src/series.rs` (above its test module):

```rust
//! The series query and panels on the wire: what `query_series` and the
//! panel methods take and answer. The same shapes as core's, so a panel file
//! reads as a definition here.

use chrono::{DateTime, FixedOffset, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::AccountType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// What moved in each bucket: expenses positive, income negative.
    Flow,
    /// What was held at the end of each bucket.
    Balance,
    /// A budget's monthly amount spread over the days.
    Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Interval {
    Day,
    /// Starts on Monday.
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    None,
    Account,
    AccountType,
    Tag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLine {
    /// The pro-rated allowance.
    Pace,
    /// The budget's balance accounts at the start, minus the cumulative pace.
    Remaining,
}

/// The range a panel shows around its anchor date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum Period {
    /// The calendar month containing the anchor.
    #[serde(rename = "month")]
    Month,
    /// The calendar year.
    #[serde(rename = "year")]
    Year,
    /// Twelve calendar months ending with the anchor's.
    #[serde(rename = "last_12_months")]
    Last12Months,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Line,
    Bar,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One series. Filters combine with AND across fields and OR within a list;
/// a field that does not apply to the measure is an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SeriesQuery {
    pub measure: Measure,
    /// flow/balance: account ids or names. A name matches that account and
    /// every account under it (`Expenses:Food` matches `Expenses:Food:Lunch`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<String>,
    /// flow/balance.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_types: Vec<AccountType>,
    /// flow/balance: the transaction carries any of these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// budget: the budget's id or name. Required for a budget series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<String>,
    /// budget: `pace` (default) or `remaining`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<BudgetLine>,
    /// flow/balance: split into one series per group. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GroupBy>,
    /// With group_by = account: group by the first `depth` name segments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Grouped: keep the largest `top` groups, sum the rest into `Other`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<u32>,
    /// flow and budget pace: a running total from the range's start.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cumulative: bool,
    /// Flip the sign, e.g. to show income as positive.
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
}

/// A series as a panel keeps it: the query, and how it is drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PanelSeries {
    #[serde(flatten)]
    pub query: SeriesQuery,
    /// `line` (default) or `bar`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<Mark>,
}

/// One plot: its own y-axis, the x-axis shared with the panel's others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Plot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Stack the plot's bars.
    #[serde(default, skip_serializing_if = "is_false")]
    pub stacked: bool,
    pub series: Vec<PanelSeries>,
}

/// A panel without its identity: what is saved, and what is run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PanelDefinition {
    /// Unique among panels.
    pub name: String,
    pub period: Period,
    /// Default: day for a month, month otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Interval>,
    /// Default: the ledger's base currency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub plots: Vec<Plot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QuerySeriesParams {
    /// `YYYY-MM-DD`, included.
    pub from: String,
    /// `YYYY-MM-DD`, included.
    pub to: String,
    pub interval: Interval,
    /// Default: the ledger's base currency. Postings in others are ignored.
    pub currency: Option<String>,
    /// At least one. All share the same buckets.
    pub series: Vec<SeriesQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RunPanelParams {
    /// A saved panel's id or name. Give this or `definition`.
    pub panel: Option<String>,
    /// An unsaved panel to run as it is. Give this or `panel`.
    pub definition: Option<PanelDefinition>,
    /// `YYYY-MM-DD` the period is around. Default today.
    pub anchor: Option<String>,
    /// Periods to move by: -1 is the previous month (or year). Default 0.
    pub offset: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddPanelParams {
    pub definition: PanelDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdatePanelParams {
    /// The panel's id or name.
    pub panel: String,
    /// Replaces the old definition whole.
    pub definition: PanelDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DeletePanelParams {
    /// The panel's id or name.
    pub panel: String,
}

/// One result series. Amounts are decimals as strings; `null` is a bucket
/// after today in an actual (flow or balance) series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesData {
    /// The query it came from.
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub label: String,
    pub values: Vec<Option<String>>,
}

/// What `query_series` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesResult {
    pub currency: String,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    /// Each bucket's first date inside the range.
    pub buckets: Vec<NaiveDate>,
    pub series: Vec<SeriesData>,
}

/// One saved panel, as `list_panels` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Panel {
    pub id: String,
    pub name: String,
    pub period: Period,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<Interval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub updated_at: DateTime<FixedOffset>,
    pub plots: Vec<Plot>,
}

impl Panel {
    pub fn definition(&self) -> PanelDefinition {
        PanelDefinition {
            name: self.name.clone(),
            period: self.period,
            interval: self.interval,
            currency: self.currency.clone(),
            plots: self.plots.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesRun {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub mark: Mark,
    /// A planned line (a budget's).
    pub dashed: bool,
    pub values: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlotRun {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub stacked: bool,
    pub series: Vec<SeriesRun>,
}

/// What `run_panel` answers: the panel's queries run, each series in its plot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelRun {
    /// The saved panel's id; absent for a definition run as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel: Option<String>,
    pub name: String,
    pub period: Period,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub interval: Interval,
    pub currency: String,
    pub buckets: Vec<NaiveDate>,
    pub plots: Vec<PlotRun>,
}
```

`crates/sapphire-ledger-api/src/templates.rs` (above its test module):

```rust
//! Ready-made panels, shared by the server (a budget's panels) and the
//! desktop (the editor's templates).

use crate::{
    AccountType, Budget, BudgetLine, GroupBy, Interval, Mark, Measure, PanelDefinition,
    PanelSeries, Period, Plot, SeriesQuery,
};

fn line(query: SeriesQuery) -> PanelSeries {
    PanelSeries { query, mark: None }
}

fn bar(query: SeriesQuery) -> PanelSeries {
    PanelSeries { query, mark: Some(Mark::Bar) }
}

fn labelled(label: &str, query: SeriesQuery) -> SeriesQuery {
    SeriesQuery { label: Some(label.to_owned()), ..query }
}

fn budget_spent(b: &Budget) -> SeriesQuery {
    SeriesQuery {
        tags: vec![b.tag.clone()],
        account_types: vec![AccountType::Expense],
        ..SeriesQuery::new(Measure::Flow)
    }
}

fn budget_series(b: &Budget, line: BudgetLine) -> SeriesQuery {
    SeriesQuery { budget: Some(b.id.clone()), line: Some(line), ..SeriesQuery::new(Measure::Budget) }
}

/// The month's tagged spending against the pro-rated budget: cumulated, and
/// day by day.
pub fn budget_pace(b: &Budget) -> PanelDefinition {
    PanelDefinition {
        name: format!("{} pace", b.name),
        period: Period::Month,
        interval: Some(Interval::Day),
        currency: Some(b.currency.clone()),
        plots: vec![
            Plot {
                title: Some("Cumulative".into()),
                stacked: false,
                series: vec![
                    line(labelled("Spent", SeriesQuery { cumulative: true, ..budget_spent(b) })),
                    line(labelled(
                        "Budget",
                        SeriesQuery { cumulative: true, ..budget_series(b, BudgetLine::Pace) },
                    )),
                ],
            },
            Plot {
                title: Some("Daily".into()),
                stacked: false,
                series: vec![
                    bar(labelled("Spent", budget_spent(b))),
                    line(labelled("Budget", budget_series(b, BudgetLine::Pace))),
                ],
            },
        ],
    }
}

/// The balance accounts against the planned path. `None` for a budget with
/// no balance accounts: there is nothing to start the plan from.
pub fn budget_balance(b: &Budget) -> Option<PanelDefinition> {
    let accounts: Vec<String> = b
        .balance_accounts
        .iter()
        .filter_map(|r| r.account_id.clone().or_else(|| r.account_name.clone()))
        .collect();
    if accounts.is_empty() {
        return None;
    }
    Some(PanelDefinition {
        name: format!("{} balance", b.name),
        period: Period::Month,
        interval: Some(Interval::Day),
        currency: Some(b.currency.clone()),
        plots: vec![Plot {
            title: None,
            stacked: false,
            series: vec![
                line(labelled("Balance", SeriesQuery { accounts, ..SeriesQuery::new(Measure::Balance) })),
                line(labelled("Plan", budget_series(b, BudgetLine::Remaining))),
            ],
        }],
    })
}

/// Monthly spending by second-level account, the seven largest and the rest.
pub fn expenses_by_account() -> PanelDefinition {
    PanelDefinition {
        name: "Expenses by account".into(),
        period: Period::Last12Months,
        interval: Some(Interval::Month),
        currency: None,
        plots: vec![Plot {
            title: None,
            stacked: true,
            series: vec![bar(SeriesQuery {
                account_types: vec![AccountType::Expense],
                group_by: Some(GroupBy::Account),
                depth: Some(2),
                top: Some(7),
                ..SeriesQuery::new(Measure::Flow)
            })],
        }],
    }
}

/// Assets and liabilities together, month by month.
pub fn net_worth() -> PanelDefinition {
    PanelDefinition {
        name: "Net worth".into(),
        period: Period::Last12Months,
        interval: Some(Interval::Month),
        currency: None,
        plots: vec![Plot {
            title: None,
            stacked: false,
            series: vec![line(labelled(
                "Net worth",
                SeriesQuery {
                    account_types: vec![AccountType::Asset, AccountType::Liability],
                    ..SeriesQuery::new(Measure::Balance)
                },
            ))],
        }],
    }
}

/// Income (as positive) beside expenses, month by month.
pub fn income_and_expenses() -> PanelDefinition {
    let flow = |ty| SeriesQuery { account_types: vec![ty], ..SeriesQuery::new(Measure::Flow) };
    PanelDefinition {
        name: "Income and expenses".into(),
        period: Period::Last12Months,
        interval: Some(Interval::Month),
        currency: None,
        plots: vec![Plot {
            title: None,
            stacked: false,
            series: vec![
                bar(labelled("Income", SeriesQuery { negate: true, ..flow(AccountType::Income) })),
                bar(labelled("Expenses", flow(AccountType::Expense))),
            ],
        }],
    }
}
```

`lib.rs`: add `mod series; pub use series::{AddPanelParams, BudgetLine, DeletePanelParams, GroupBy, Interval, Mark, Measure, Panel, PanelDefinition, PanelRun, PanelSeries, Period, Plot, PlotRun, QuerySeriesParams, RunPanelParams, SeriesData, SeriesQuery, SeriesResult, SeriesRun, UpdatePanelParams};` and `pub mod templates;`. Add the six constants after `UPDATE_BUDGET`, each with a doc line in the existing style:

```rust
/// A series query, as JSON: a [`SeriesResult`].
pub const QUERY_SERIES: &str = "sapphire-ledger.query_series";
/// Every panel, as JSON: a list of [`Panel`].
pub const LIST_PANELS: &str = "sapphire-ledger.list_panels";
/// A panel's queries run, as JSON: a [`PanelRun`].
pub const RUN_PANEL: &str = "sapphire-ledger.run_panel";
/// Save a panel; answers with a confirmation to show as it is.
pub const ADD_PANEL: &str = "sapphire-ledger.add_panel";
/// Replace a panel's definition; answers with a confirmation to show as it is.
pub const UPDATE_PANEL: &str = "sapphire-ledger.update_panel";
/// Remove a panel; answers with a confirmation to show as it is.
pub const DELETE_PANEL: &str = "sapphire-ledger.delete_panel";
```

Update the existing `AddBudgetParams { … }` literals: `crates/sapphire-ledger-mcp/src/methods.rs` (test helper `add_budget`), `cli/tests/mcp_tools.rs`, and `desktop/src/form.rs` `to_add_params` — add `create_panels: false`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p sapphire-ledger-api && cargo build --workspace --all-targets`
Expected: api tests PASS (the 5 new ones and the existing); the workspace builds.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add crates/sapphire-ledger-api crates/sapphire-ledger-mcp/src/methods.rs cli/tests/mcp_tools.rs desktop/src/form.rs
git commit -m "feat(api): series query and panel types, and the panel templates"
```

---

### Task 4: Server methods and MCP tools

**Files:**
- Modify: `crates/sapphire-ledger-mcp/src/wire.rs`, `src/methods.rs`, `src/tools.rs`
- Modify: `cli/tests/mcp_tools.rs` (budget_report part → run_panel), `desktop/src/client.rs` (its server test's `BUDGET_REPORT` part → `RUN_PANEL`)

**Interfaces:**
- Consumes: core `series::{SeriesRequest, query_series, SeriesResult}`, `panel::{run_panel, find_panel, PanelRun}`, `ops::{create_panel, update_panel, delete_panel, create_budget_with_panels}`, `Workspace.panels`; api types and templates from Task 3.
- Produces: methods `query_series`, `list_panels`, `run_panel`, `add_panel`, `update_panel`, `delete_panel` on `LedgerMethods`, routed under the Task 3 constants; `add_budget` honours `create_panels`; tools of the same six names; `budget_report` method, route, tool and `wire::budget_report` removed. `wire::{convert, panel, panel_definition, series_query, series_result, panel_run}`.

- [ ] **Step 1: Write the failing tests** — in `methods.rs` tests, replace `a_budget_is_listed_and_reported_by_day` (keep `update_budget_changes_the_amount_and_a_bad_month_is_refused` but drop its `budget_report` half; keep its update assertions) with:

```rust
    #[test]
    fn a_budget_with_panels_runs_by_name_and_a_query_runs_as_given() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");
        let created = server
            .add_budget(api::AddBudgetParams {
                name: "生活費".into(),
                tag: "living".into(),
                currency: "JPY".into(),
                monthly_amount: "100000".into(),
                balance_accounts: vec!["Assets:Cash:JPY".into()],
                create_panels: true,
            })
            .expect("add_budget");
        assert!(created.contains("created budget"), "{created}");
        assert_eq!(created.matches("created panel").count(), 2, "{created}");
        spend(&server, "2026-05-21", vec!["living".into()], "1200", false);

        let panels: Vec<api::Panel> =
            serde_json::from_str(&server.list_panels(EmptyParams {}).unwrap()).unwrap();
        let names: Vec<&str> = panels.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"生活費 pace") && names.contains(&"生活費 balance"), "{names:?}");

        let run: api::PanelRun = serde_json::from_str(
            &server
                .run_panel(api::RunPanelParams {
                    panel: Some("生活費 pace".into()),
                    definition: None,
                    anchor: Some("2026-05-10".into()),
                    offset: None,
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(run.buckets.len(), 31);
        assert_eq!(run.plots[0].series[1].values[30].as_deref(), Some("100000"));
        assert!(run.panel.is_some());

        let result: api::SeriesResult = serde_json::from_str(
            &server
                .query_series(api::QuerySeriesParams {
                    from: "2026-05-01".into(),
                    to: "2026-05-31".into(),
                    interval: api::Interval::Month,
                    currency: None,
                    series: vec![api::SeriesQuery {
                        tags: vec!["living".into()],
                        ..api::SeriesQuery::new(api::Measure::Flow)
                    }],
                })
                .unwrap(),
        )
        .unwrap();
        // Both sides of the posting carry the tag: +1200 and -1200.
        assert_eq!(result.series[0].values, [Some("0".to_string())]);

        let err = server
            .run_panel(api::RunPanelParams { panel: None, definition: None, anchor: None, offset: None })
            .unwrap_err();
        assert!(err.contains("exactly one of `panel` and `definition`"), "{err}");
        let deleted = server
            .delete_panel(api::DeletePanelParams { panel: "生活費 balance".into() })
            .unwrap();
        assert!(deleted.contains("deleted panel"), "{deleted}");
    }

    #[test]
    fn a_panel_is_added_updated_and_refused_when_wrong() {
        let (_dir, server) = test_server();
        let def = api::templates::net_worth();
        assert!(server.add_panel(api::AddPanelParams { definition: def.clone() }).unwrap().contains("created panel"));
        let renamed = api::PanelDefinition { name: "Worth".into(), ..def.clone() };
        assert!(server
            .update_panel(api::UpdatePanelParams { panel: "Net worth".into(), definition: renamed })
            .unwrap()
            .contains("updated panel"));
        let mut wrong = def;
        wrong.name = "Wrong".into();
        wrong.plots[0].series[0].query.budget = Some("x".into());
        let err = server.add_panel(api::AddPanelParams { definition: wrong }).unwrap_err();
        assert!(err.contains("`budget` does not apply to a balance series"), "{err}");
    }
```

In `tools.rs` tests: the expected-name list gains `"query_series", "list_panels", "run_panel", "add_panel", "update_panel", "delete_panel"` and loses `"budget_report"`; add `assert!(!names.iter().any(|n| n == "budget_report"), "{names:?}");`; the count becomes `19 + 7` and the doc comment "The ledger's nineteen, …".

In `cli/tests/mcp_tools.rs`: the `add_budget` call sets `create_panels: true`; replace the `budget_report` call and its assertion with:

```rust
    let run = tools
        .run_panel(Parameters(RunPanelParams {
            panel: Some("生活費 pace".into()),
            definition: None,
            anchor: Some("2026-05-10".into()),
            offset: None,
        }))
        .await
        .expect("run_panel");
    assert!(run.contains("\"100000\""), "got: {run}");
```

(import `RunPanelParams` in place of `BudgetReportParams`).

In `desktop/src/client.rs`'s test:
- set `form.tags = "living".into();` on the `TxForm` before it is sent, so the spending carries the budget's tag;
- replace the `BUDGET_REPORT` read (and the imports `BudgetReport, BudgetReportParams`) with: read `LIST_BUDGETS` as `Vec<api::Budget>`, then read `RUN_PANEL` as `api::PanelRun` with `RunPanelParams { panel: None, definition: Some(api::templates::budget_pace(&budgets[0])), anchor: Some("2026-05-21".into()), offset: None }`;
- assert `run.buckets.len() == 31` and `run.plots[0].series[0].values[20].as_deref() == Some("1200")` (the tagged spending, cumulated, on the 21st).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-mcp`
Expected: FAIL to compile — `no method named run_panel`.

- [ ] **Step 3: Implement `wire.rs`** — remove `budget_report`; add:

```rust
use serde::{Serialize, de::DeserializeOwned};

/// Between core's and the API's copies of one shape, through their JSON.
/// The tests pin that the two serialize alike.
pub fn convert<T: Serialize, U: DeserializeOwned>(from: &T) -> anyhow::Result<U> {
    Ok(serde_json::from_value(serde_json::to_value(from)?)?)
}

pub fn panel(p: &core::Panel) -> anyhow::Result<api::Panel> {
    convert(p)
}

pub fn panel_definition(d: &api::PanelDefinition) -> anyhow::Result<core::PanelDefinition> {
    convert(d)
}

pub fn series_query(q: &api::SeriesQuery) -> anyhow::Result<core::series::SeriesQuery> {
    convert(q)
}

fn amounts(values: Vec<Option<rust_decimal::Decimal>>) -> Vec<Option<String>> {
    values.into_iter().map(|v| v.map(|d| d.to_string())).collect()
}

pub fn series_result(r: core::series::SeriesResult) -> anyhow::Result<api::SeriesResult> {
    Ok(api::SeriesResult {
        currency: r.currency,
        from: r.from,
        to: r.to,
        interval: convert(&r.interval)?,
        buckets: r.buckets,
        series: r
            .series
            .into_iter()
            .map(|s| api::SeriesData { index: s.index, group: s.group, label: s.label, values: amounts(s.values) })
            .collect(),
    })
}

pub fn panel_run(id: Option<String>, r: core::panel::PanelRun) -> anyhow::Result<api::PanelRun> {
    Ok(api::PanelRun {
        panel: id,
        name: r.name,
        period: convert(&r.period)?,
        from: r.from,
        to: r.to,
        interval: convert(&r.interval)?,
        currency: r.currency,
        buckets: r.buckets,
        plots: r
            .plots
            .into_iter()
            .map(|p| {
                Ok(api::PlotRun {
                    title: p.title,
                    stacked: p.stacked,
                    series: p
                        .series
                        .into_iter()
                        .map(|s| {
                            Ok(api::SeriesRun {
                                label: s.label,
                                group: s.group,
                                mark: convert(&s.mark)?,
                                dashed: s.dashed,
                                values: amounts(s.values),
                            })
                        })
                        .collect::<anyhow::Result<_>>()?,
                })
            })
            .collect::<anyhow::Result<_>>()?,
    })
}
```

Add a wire test that a core `PanelDefinition` built with every field set (flow with accounts/types/tags/group_by/depth/top/cumulative/negate/label/mark bar; budget with line remaining; period last_12_months; interval week; currency) converts to the API's and back unchanged.

- [ ] **Step 4: Implement methods** — in `methods.rs`: remove `budget_report`, its route, `BudgetReportParams` import and `find_budget` (now unused; `core::series::find_budget` exists). Add routes for the six constants (same `route(...)` pattern). Add to `impl LedgerMethods`:

```rust
    fn query_series(&self, p: QuerySeriesParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let request = series::SeriesRequest {
                from: parse_date(&p.from)?,
                to: parse_date(&p.to)?,
                interval: wire::convert(&p.interval)?,
                currency: p.currency,
                series: p.series.iter().map(wire::series_query).collect::<anyhow::Result<_>>()?,
            };
            let guard = self.lock_state();
            let today = chrono::Local::now().date_naive();
            let result = series::query_series(guard.workspace(), &request, today)?;
            Ok(serde_json::to_string_pretty(&wire::series_result(result)?)?)
        })()
        .map_err(render_error)
    }

    fn list_panels(&self, _: EmptyParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let guard = self.lock_state();
            let panels = guard
                .workspace()
                .panels
                .iter()
                .map(wire::panel)
                .collect::<anyhow::Result<Vec<_>>>()?;
            Ok(serde_json::to_string_pretty(&panels)?)
        })()
        .map_err(render_error)
    }

    fn run_panel(&self, p: RunPanelParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let guard = self.lock_state();
            let ws = guard.workspace();
            let (id, definition) = match (p.panel, p.definition) {
                (Some(needle), None) => {
                    let found = panel::find_panel(&ws.panels, &needle)?;
                    (Some(found.id.clone()), found.definition())
                }
                (None, Some(definition)) => (None, wire::panel_definition(&definition)?),
                _ => anyhow::bail!("give exactly one of `panel` and `definition`"),
            };
            let today = chrono::Local::now().date_naive();
            let anchor = p.anchor.as_deref().map(parse_date).transpose()?.unwrap_or(today);
            let run = panel::run_panel(ws, &definition, anchor, p.offset.unwrap_or(0), today)?;
            Ok(serde_json::to_string_pretty(&wire::panel_run(id, run)?)?)
        })()
        .map_err(render_error)
    }

    fn add_panel(&self, p: AddPanelParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let definition = wire::panel_definition(&p.definition)?;
            let mut guard = self.lock_state();
            let (id, dest) = ops::create_panel(guard.root(), definition)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("created", "panel", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn update_panel(&self, p: UpdatePanelParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let definition = wire::panel_definition(&p.definition)?;
            let mut guard = self.lock_state();
            let (id, dest) = ops::update_panel(guard.root(), &p.panel, definition)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("updated", "panel", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn delete_panel(&self, p: DeletePanelParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let mut guard = self.lock_state();
            let (id, dest) = ops::delete_panel(guard.root(), &p.panel)?;
            let reload_err = guard.reload().err();
            drop(guard);
            // A removed path is still a path the write touched: sync sends the delete.
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("deleted", "panel", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }
```

Before relying on the delete notification, read where `WriteObserver` is constructed (`cli/src/serve.rs`) and confirm a removed path is handled (the framework's sync treats a missing file as a delete). If it is not, report DONE_WITH_CONCERNS naming what the observer does with it.

`add_budget` becomes:

```rust
    fn add_budget(&self, p: AddBudgetParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let amount = parse_decimal(&p.monthly_amount)?;
            let mut guard = self.lock_state();
            // See add_transaction: classify references against fresh accounts.
            guard.reload()?;
            let refs = account_refs(p.balance_accounts, &guard.workspace().accounts);
            let with_panels = p.create_panels;
            let (id, dest, panels) = ops::create_budget_with_panels(
                guard.root(),
                p.name,
                p.tag,
                p.currency,
                amount,
                refs,
                |budget| {
                    if !with_panels {
                        return Ok(vec![]);
                    }
                    let budget = wire::budget(budget);
                    [Some(api::templates::budget_pace(&budget)), api::templates::budget_balance(&budget)]
                        .into_iter()
                        .flatten()
                        .map(|d| {
                            wire::panel_definition(&d)
                                .map_err(|e| sapphire_ledger_core::Error::Validation(e.to_string()))
                        })
                        .collect()
                },
            )?;
            let reload_err = guard.reload().err();
            drop(guard);
            let mut touched = vec![dest.clone()];
            touched.extend(panels.iter().map(|(_, path)| path.clone()));
            self.notify_write(&touched);
            let mut message = write_result("created", "budget", &id, &dest, reload_err);
            for (panel_id, path) in &panels {
                message.push_str(&format!("\ncreated panel {panel_id}: {}", path.display()));
            }
            Ok(message)
        })()
        .map_err(render_error)
    }
```

Imports: `sapphire_ledger_core::{panel, series}` and the new params.

- [ ] **Step 5: Implement tools** — in `tools.rs`, remove `budget_report`, add (imports updated):

```rust
    #[tool(
        description = "Run a series query: per bucket (day, week, month or year) between \
        `from` and `to`, how much flowed (`flow`: expenses positive, income negative), what \
        was held (`balance`), or a budget's pro-rated line (`budget`: `pace`, or `remaining` \
        from its balance accounts). Filter by accounts (a name matches everything under \
        it), account_types and tags; split with group_by; `top` folds small groups into \
        Other. One currency, the ledger's base by default. Check list_panels first: a saved \
        panel may already be the query you want.",
        annotations(read_only_hint = true)
    )]
    pub async fn query_series(&self, Parameters(p): Parameters<QuerySeriesParams>) -> Result<String, String> {
        self.call(api::QUERY_SERIES, p).await
    }

    #[tool(
        description = "List the saved panels: each a chart's period and, per plot, the \
        series queries it draws. They are query presets: run one with run_panel.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_panels(&self, Parameters(p): Parameters<EmptyParams>) -> Result<String, String> {
        self.call(api::LIST_PANELS, p).await
    }

    #[tool(
        description = "Run a saved panel (by id or name) as a query preset, or an unsaved \
        `definition` as given. The period is around `anchor` (default today); `offset: -1` \
        is the previous month (or year). Look in list_panels for a preset first; use \
        query_series when none fits.",
        annotations(read_only_hint = true)
    )]
    pub async fn run_panel(&self, Parameters(p): Parameters<RunPanelParams>) -> Result<String, String> {
        self.call(api::RUN_PANEL, p).await
    }

    #[tool(
        description = "Save a panel: a query you will run again, kept as a preset (and \
        drawn on the desktop's Charts screen). Its name must be unused."
    )]
    pub async fn add_panel(&self, Parameters(p): Parameters<AddPanelParams>) -> Result<String, String> {
        self.call(api::ADD_PANEL, p).await
    }

    #[tool(
        description = "Replace a panel's definition, by id or name. The file is rewritten \
        from its known fields: unknown keys and comments in it are not kept."
    )]
    pub async fn update_panel(&self, Parameters(p): Parameters<UpdatePanelParams>) -> Result<String, String> {
        self.call(api::UPDATE_PANEL, p).await
    }

    #[tool(description = "Delete a panel, by id or name.")]
    pub async fn delete_panel(&self, Parameters(p): Parameters<DeletePanelParams>) -> Result<String, String> {
        self.call(api::DELETE_PANEL, p).await
    }
```

`add_budget`'s description gains: "With `create_panels`, also saves its `<name> pace` and `<name> balance` panels." The server instructions' last sentence becomes: "list_panels and run_panel show saved charts (a budget's pace, net worth, …) as query presets; query_series answers anything else."

- [ ] **Step 6: Run tests**

Run: `cargo test -p sapphire-ledger-mcp && cargo test -p sapphire-ledger --test mcp_tools && cargo test -p sapphire-ledger-desktop`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
cargo fmt
git add crates/sapphire-ledger-mcp cli/tests/mcp_tools.rs desktop/src/client.rs
git commit -m "feat(mcp): query_series and panels as query presets, in place of budget_report"
```

---

### Task 5: CLI `panel list|run`; remove `budget report` and core's report

**Files:**
- Modify: `cli/src/cli.rs` (remove `Budget`/`BudgetCommand`; add `Panel(PanelCommand)`), `cli/src/commands.rs` (remove `budget`; add `panel`), `cli/src/main.rs`
- Modify: `crates/sapphire-ledger-core/src/report.rs` (keep only `round_amount`), `crates/sapphire-ledger-core/tests/report.rs` (keep only the `round_amount` test)
- Test: `cli/tests/write_commands.rs` (replace `budget_report_prints_a_hand_written_budgets_month`)

**Interfaces:**
- Consumes: `panel::{find_panel, run_panel}`, `Workspace.panels`, `report::round_amount`.
- Produces: `sapphire-ledger panel list`, `sapphire-ledger panel run <panel> [--anchor YYYY-MM-DD] [--offset N]`. Core loses `report::{budget_report, parse_month, BudgetDay, BudgetReport}`.

- [ ] **Step 1: Write the failing test** — in `cli/tests/write_commands.rs`, replace the budget test with:

```rust
#[test]
fn panel_run_prints_a_hand_written_panels_month() {
    let dir = seeded();
    let root = dir.path();
    // A budget and a panel written by hand, naming accounts by name only.
    std::fs::create_dir_all(root.join("budgets")).unwrap();
    std::fs::write(
        root.join("budgets/bud0001.toml"),
        "id = \"bud0001\"\nname = \"living\"\ntag = \"grocery\"\ncurrency = \"JPY\"\n\
         monthly_amount = \"31000\"\ncreated_at = \"2026-05-01T00:00:00+09:00\"\n\
         updated_at = \"2026-05-01T00:00:00+09:00\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("panels")).unwrap();
    std::fs::write(
        root.join("panels/pan0001.toml"),
        "id = \"pan0001\"\nname = \"Living pace\"\nperiod = \"month\"\n\
         created_at = \"2026-05-01T00:00:00+09:00\"\nupdated_at = \"2026-05-01T00:00:00+09:00\"\n\n\
         [[plots]]\ntitle = \"Cumulative\"\n\n\
         [[plots.series]]\nmeasure = \"flow\"\ntags = [\"grocery\"]\naccount_types = [\"Expense\"]\ncumulative = true\nlabel = \"Spent\"\n\n\
         [[plots.series]]\nmeasure = \"budget\"\nbudget = \"living\"\ncumulative = true\nlabel = \"Budget\"\n",
    )
    .unwrap();
    ok(
        root,
        &[
            "tx", "add", "--date", "2026-05-22", "--narration", "veg", "--tag", "grocery",
            "--posting", "Expenses:Food 500 JPY", "--posting", "Assets:Cash -500 JPY",
        ],
    );

    let list = ok(root, &["panel", "list"]);
    assert!(list.contains("pan0001  Living pace  (month)"), "got: {list}");

    let out = ok(root, &["panel", "run", "Living pace", "--anchor", "2026-05-10"]);
    assert!(out.starts_with("Living pace · 2026-05-01 – 2026-05-31 · JPY"), "got: {out}");
    assert!(out.contains("== Cumulative =="), "got: {out}");
    let line = out.lines().find(|l| l.starts_with("2026-05-22")).expect("a row per day");
    let cols: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(cols, ["2026-05-22", "500", "22000"], "got: {line}");
    assert_eq!(out.lines().filter(|l| l.starts_with("2026-05-")).count(), 31);

    let prev = ok(root, &["panel", "run", "pan0001", "--anchor", "2026-06-10", "--offset", "-1"]);
    assert!(prev.contains("2026-05-01 – 2026-05-31"), "got: {prev}");

    let err = run(root, &["panel", "run", "nope"]);
    assert!(!err.status.success());
    assert!(String::from_utf8_lossy(&err.stderr).contains("no panel nope"));
    let gone = run(root, &["budget", "report", "living"]);
    assert!(!gone.status.success(), "`budget report` is gone");
}
```

Note the expected `500` for the 22nd: the `Spent` column is cumulative tagged spending to Expense accounts (only the Food posting is an Expense), and today is after May 2026 so the 22nd is not null — if this test runs before 2026-05-22 in real time the value would be `-`; that cannot happen (today is 2026-10-17 or later).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger --test write_commands panel_run`
Expected: FAIL — `unrecognized subcommand 'panel'`.

- [ ] **Step 3: Implement** — `cli.rs`: delete `Budget(BudgetCommand)` and `BudgetCommand`; add in their place:

```rust
    /// Run saved panels: a chart's queries, as a table.
    #[command(subcommand)]
    Panel(PanelCommand),
```

```rust
#[derive(Subcommand)]
pub enum PanelCommand {
    /// Every panel: id, name and period.
    List,
    /// One panel's queries, a table per plot: a row per bucket, a column per series.
    Run {
        /// Panel name or id.
        panel: String,
        /// `YYYY-MM-DD` the period is around. Defaults to today.
        #[arg(long, value_name = "DATE")]
        anchor: Option<String>,
        /// Periods to move by: -1 is the previous month (or year).
        #[arg(long, allow_hyphen_values = true, value_name = "N")]
        offset: Option<i32>,
    },
}
```

`main.rs`: `Command::Panel(c) => commands::panel(c, &resolve_root(cli.ledger_dir)?),` in place of the budget arm.

`commands.rs`: remove `budget` (and the `report` import if unused; keep `today()`); add:

```rust
pub fn panel(command: PanelCommand, root: &Path) -> anyhow::Result<()> {
    let workspace = sapphire_ledger_core::load_workspace(root)?;
    match command {
        PanelCommand::List => {
            if workspace.panels.is_empty() {
                println!("no panels");
            }
            for p in &workspace.panels {
                let period = serde_json::to_value(p.period)?;
                println!("{}  {}  ({})", p.id, p.name, period.as_str().unwrap_or_default());
            }
        }
        PanelCommand::Run { panel, anchor, offset } => {
            let found = sapphire_ledger_core::panel::find_panel(&workspace.panels, &panel)?;
            let anchor = match anchor.as_deref() {
                Some(raw) => parse_date(raw)?,
                None => today(),
            };
            let run = sapphire_ledger_core::panel::run_panel(
                &workspace,
                &found.definition(),
                anchor,
                offset.unwrap_or(0),
                today(),
            )?;
            println!("{} · {} – {} · {}", run.name, run.from, run.to, run.currency);
            for (i, plot) in run.plots.iter().enumerate() {
                println!();
                println!("== {} ==", plot.title.clone().unwrap_or_else(|| format!("plot {}", i + 1)));
                let widths: Vec<usize> = plot.series.iter().map(|s| s.label.chars().count().max(12)).collect();
                let mut header = format!("{:<10}", "date");
                for (s, w) in plot.series.iter().zip(&widths) {
                    header.push_str(&format!("  {:>w$}", s.label, w = w));
                }
                println!("{header}");
                for (b, date) in run.buckets.iter().enumerate() {
                    let mut row = format!("{date}");
                    for (s, w) in plot.series.iter().zip(&widths) {
                        let v = s.values[b].map_or_else(|| "-".to_owned(), |d| d.to_string());
                        row.push_str(&format!("  {v:>w$}", w = w));
                    }
                    println!("{row}");
                }
            }
        }
    }
    Ok(())
}
```

(`serde_json` is already a dependency of `cli`.)

`report.rs`: delete everything but `round_amount` (and its imports); update the module doc to "How a report's amounts are rounded." `tests/report.rs`: keep only `round_amount_keeps_two_places_and_drops_trailing_zeros` (drop the fixture and other tests; the series tests cover their ground).

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: PASS. `grep -rn "budget_report\|parse_month" crates cli` finds nothing outside the api crate's still-present `BUDGET_REPORT` types and the desktop's charts.rs.

- [ ] **Step 5: Commit**

```bash
cargo fmt
git add cli crates/sapphire-ledger-core
git commit -m "feat(cli)!: panel list and panel run in place of budget report"
```

---

### Task 6: Desktop Charts screen on panels

**Files:**
- Modify: `desktop/src/charts.rs` (rewritten around panels), `desktop/src/form.rs` (`BudgetForm.create_panels`), `desktop/src/main.rs` (module doc line)
- Modify: `crates/sapphire-ledger-api/src/{lib,params,records}.rs` — remove `BUDGET_REPORT`, `BudgetReportParams`, `BudgetReport`, `BudgetDay`

**Interfaces:**
- Consumes: `api::{LIST_PANELS, LIST_BUDGETS, LIST_ACCOUNTS, RUN_PANEL, ADD_BUDGET, UPDATE_BUDGET, Panel, PanelRun, PlotRun, SeriesRun, RunPanelParams, Interval, Period, Mark}`; `BudgetForm` (existing).
- Produces (for Task 7, in `charts.rs`): `ChartsView` fields `panels: Vec<Panel>`, `selected: Option<String>` (panel id), `select_name: Option<String>`, `offset: i32`, `run: Option<Result<PanelRun, String>>`, `fn reload_now(&mut self)`, `fn spawn(...)`, `enum Reply` (extended by Task 7), `fn draw_run(ui: &mut egui::Ui, run: &PanelRun)`; pure helpers `latest(&[Option<String>]) -> Option<&str>`, `bucket_label(Interval, Period, NaiveDate) -> String`, `period_label(&PanelRun) -> String`, `series_color(usize, dark: bool) -> Color32`, `grouped(&str) -> String`. `BudgetForm.create_panels: bool`.

- [ ] **Step 1: Write the failing tests** — replace the `#[cfg(test)] mod tests` in `charts.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> chrono::NaiveDate {
        s.parse().unwrap()
    }

    fn run(period: Period, interval: Interval, from: &str, to: &str) -> PanelRun {
        PanelRun {
            panel: Some("p".into()),
            name: "n".into(),
            period,
            from: d(from),
            to: d(to),
            interval,
            currency: "JPY".into(),
            buckets: vec![d(from)],
            plots: vec![],
        }
    }

    #[test]
    fn the_latest_value_is_the_last_one_there() {
        let v = vec![Some("1".to_string()), Some("2".to_string()), None];
        assert_eq!(latest(&v), Some("2"));
        assert_eq!(latest(&[None]), None);
    }

    #[test]
    fn buckets_and_periods_are_labelled_for_their_scale() {
        assert_eq!(bucket_label(Interval::Day, Period::Month, d("2026-10-07")), "7");
        assert_eq!(bucket_label(Interval::Day, Period::Year, d("2026-10-07")), "10-07");
        assert_eq!(bucket_label(Interval::Week, Period::Month, d("2026-10-05")), "10-05");
        assert_eq!(bucket_label(Interval::Month, Period::Last12Months, d("2026-10-01")), "2026-10");
        assert_eq!(bucket_label(Interval::Year, Period::Year, d("2026-01-01")), "2026");
        assert_eq!(period_label(&run(Period::Month, Interval::Day, "2026-10-01", "2026-10-31")), "2026-10");
        assert_eq!(period_label(&run(Period::Year, Interval::Month, "2026-01-01", "2026-12-31")), "2026");
        assert_eq!(
            period_label(&run(Period::Last12Months, Interval::Month, "2025-11-01", "2026-10-31")),
            "2025-11 – 2026-10"
        );
    }

    #[test]
    fn colours_follow_the_order_and_go_grey_past_eight() {
        assert_eq!(series_color(0, false), Color32::from_rgb(0x2a, 0x78, 0xd6));
        assert_eq!(series_color(1, true), Color32::from_rgb(0xd9, 0x59, 0x26));
        assert_eq!(series_color(7, false), Color32::from_rgb(0xe3, 0x49, 0x48));
        assert_eq!(series_color(8, true), Color32::from_rgb(0x8a, 0x8a, 0x85));
    }

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(grouped("-1234567.5"), "-1,234,567.5");
        assert_eq!(grouped("999"), "999");
    }

    fn view() -> (tokio::runtime::Runtime, ChartsView) {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let endpoint = sapphire_framework::ipc::Endpoint::in_dir("test", std::env::temp_dir().join("charts-test"));
        let client = Arc::new(LedgerClient::new("test", "0.0.0", endpoint));
        let view = ChartsView::new(runtime.handle().clone(), client, Arc::new(|| {}));
        (runtime, view)
    }

    fn panel(id: &str, name: &str) -> Panel {
        let at = chrono::DateTime::parse_from_rfc3339("2026-10-17T00:00:00+09:00").unwrap();
        Panel {
            id: id.into(),
            name: name.into(),
            period: Period::Month,
            interval: None,
            currency: None,
            created_at: at,
            updated_at: at,
            plots: vec![],
        }
    }

    #[test]
    fn a_load_overtaken_by_a_panel_change_is_dropped() {
        let (_rt, mut view) = view();
        let in_flight = view.generation.next();
        view.selected = Some("other".into());
        view.reload_now();
        let loaded = Loaded { panels: vec![panel("p1", "A")], budgets: vec![], accounts: vec![], shown: Some("p1".into()), run: None };
        view.replies.0.send(Reply::Loaded(in_flight, Ok(Box::new(loaded)))).unwrap();
        view.drain_replies();
        assert_eq!(view.selected.as_deref(), Some("other"));
    }

    #[test]
    fn a_panel_named_after_a_write_is_picked() {
        let panels = vec![panel("p1", "A"), panel("p2", "生活費 pace")];
        assert_eq!(pick(&panels, None, Some("生活費 pace")), Some("p2".to_string()));
        assert_eq!(pick(&panels, Some("p1"), None), Some("p1".to_string()));
        assert_eq!(pick(&panels, Some("gone"), None), Some("p1".to_string()));
        assert_eq!(pick(&[], None, None), None);
    }
}
```

In `form.rs` tests, add:

```rust
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-desktop`
Expected: FAIL to compile.

- [ ] **Step 3: Implement `BudgetForm.create_panels`** — field `pub create_panels: bool` (doc: "For a new budget: also make its pace and balance panels."), `true` in `new`, `false` in `edit`; `to_add_params` sets `create_panels: self.create_panels` (replacing Task 3's `false`).

- [ ] **Step 4: Rewrite `charts.rs`** — keep `REFRESH_EVERY`, `Generation`, `spawn`, `grouped`, and the plot conventions of #43 (`y_axis_min_width(72.0)`, linked x-axis and cursor, no drag/zoom). Remove the budget-report helpers (`pace_series`, `balance_series`, `summary`, `month_plot`, `Colors`, `shift_month`, `month_label`, `Tab`). New structure:

```rust
//! The Charts screen: the workspace's panels, each drawn from the server's
//! `run_panel`, and the budgets a panel can draw against.
//!
//! Like the Ledger screen it is a client of the running server: calls run on
//! the tokio runtime and come back through a channel, and the view never
//! blocks. Everything it draws is computed by the server; the helpers here
//! only turn the answer into plot points and text.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate};
use egui_plot::{Bar, BarChart, HoverPosition, Legend, Line, LineStyle, Plot, PlotPoints};
use rust_decimal::prelude::ToPrimitive;
use sapphire_framework::gui::egui::{self, Color32};
use sapphire_ledger_api::{
    self as api, Account, Budget, EmptyParams, Interval, Mark, Panel, PanelRun, Period, PlotRun,
    RunPanelParams,
};

use crate::client::LedgerClient;
use crate::form::{self, BudgetForm};

const REFRESH_EVERY: Duration = Duration::from_secs(10);

// `Generation` as before.

struct Loaded {
    panels: Vec<Panel>,
    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    /// The panel the run is of.
    shown: Option<String>,
    /// The panel's run; its error is shown in the body, not as a load error.
    run: Option<Result<PanelRun, String>>,
}

enum Reply {
    Loaded(u64, Result<Box<Loaded>, String>),
    /// A write finished; on success, select the panel of this name next.
    Written(Option<String>, Result<String, String>),
}

pub struct ChartsView {
    runtime: tokio::runtime::Handle,
    client: Arc<LedgerClient>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    replies: (mpsc::Sender<Reply>, mpsc::Receiver<Reply>),
    generation: Generation,

    panels: Vec<Panel>,
    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    /// The panel shown, by id.
    selected: Option<String>,
    /// After a write: the panel to pick on the next load, by name.
    select_name: Option<String>,
    /// Periods from the current one.
    offset: i32,
    run: Option<Result<PanelRun, String>>,
    load_error: Option<String>,
    loading: bool,
    last_refresh: Option<Instant>,

    budgets_open: bool,
    budget_form: Option<BudgetForm>,
    sending: bool,
    form_error: Option<String>,
}

/// The panel to show: the selected one if it is still there, else the one
/// just written by name, else the first.
fn pick(panels: &[Panel], selected: Option<&str>, by_name: Option<&str>) -> Option<String> {
    by_name
        .and_then(|name| panels.iter().find(|p| p.name == name))
        .or_else(|| selected.and_then(|id| panels.iter().find(|p| p.id == id)))
        .or_else(|| panels.first())
        .map(|p| p.id.clone())
}
```

`ui`:

```rust
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.drain_replies();
        if !self.loading && self.last_refresh.is_none_or(|t| t.elapsed() >= REFRESH_EVERY) {
            self.refresh();
        }
        ui.ctx().request_repaint_after(REFRESH_EVERY);

        egui::Panel::left("charts_panels")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| self.panel_list(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            self.toolbar(ui);
            ui.separator();
            self.body(ui);
        });
        self.budgets_window(ui.ctx());
        self.budget_form_window(ui.ctx());
    }
```

`refresh` reads `LIST_PANELS`, `LIST_BUDGETS`, `LIST_ACCOUNTS`, computes `shown = pick(&panels, selected, select_name)`, and if `Some(id)` calls `RUN_PANEL` with `RunPanelParams { panel: Some(id), definition: None, anchor: None, offset: Some(offset) }`, keeping its `Result` as `run` (`client.read::<PanelRun>(…).await` — an `Err` here goes into `run`, not the load error). `drain_replies` on a current `Loaded(Ok)`: set `panels`, `budgets`, `accounts`, `selected = loaded.shown`, `run = loaded.run`, clear `select_name` and `load_error`, `loading = false`; on `Loaded(Err)` keep the last chart, set `load_error`. On `Written(select, Ok)`: `sending = false`, `budget_form = None`, `select_name = select`, `reload_now()`. On `Written(_, Err(e))`: `sending = false`, `form_error = Some(e)`.

`reload_now` as #43's, but clears `run` instead of `report`.

`panel_list`: heading "Panels"; a `selectable_label` per panel (sorted by name) that sets `selected`, `offset = 0` and calls `reload_now()` when clicked; `ui.weak("No panels yet.")` when empty; then `ui.separator()` and a "Budgets…" button setting `budgets_open = true`.

`toolbar`: the shown panel's name as `ui.strong`, then `◀` / period label / `▶` (hover texts "Previous period"/"Next period") changing `offset` by ∓1 and calling `reload_now()`; the period label is `period_label(run)` when `run` is `Some(Ok)`, else empty; spinner while loading; the load error below in `error_fg_color`.

`body`: no panels and not loading → `ui.label("No panels yet. Make one from a budget under Budgets…, or with an agent's add_panel.")`; `run` `None` → spinner; `Some(Err(e))` → `ui.colored_label(error_fg_color, e)`; `Some(Ok(run))` → `draw_run(ui, run)`.

`budgets_window` (`egui::Window::new("Budgets")`, `open(&mut self.budgets_open)`): each budget as `"{name} · {grouped(amount)} {currency} / month"` with an "Edit" button (`BudgetForm::edit(b, &self.accounts)`); a "New budget" button (`BudgetForm::new(currency of the first budget, else "JPY")`).

`budget_form_window`: #43's `form_window`, plus — for a new budget (`form.id.is_none()`) — `ui.checkbox(&mut form.create_panels, "Create its panels (pace, and balance when paid from accounts)")` above Save. `send_form` sends `Reply::Written(select, …)` with `select = (form.id.is_none() && form.create_panels).then(|| format!("{} pace", form.name.trim()))`.

Drawing:

```rust
const PALETTE_LIGHT: [Color32; 8] = [
    Color32::from_rgb(0x2a, 0x78, 0xd6),
    Color32::from_rgb(0xeb, 0x68, 0x34),
    Color32::from_rgb(0x1b, 0xaf, 0x7a),
    Color32::from_rgb(0xed, 0xa1, 0x00),
    Color32::from_rgb(0xe8, 0x7b, 0xa4),
    Color32::from_rgb(0x00, 0x83, 0x00),
    Color32::from_rgb(0x62, 0x50, 0xd6),
    Color32::from_rgb(0xe3, 0x49, 0x48),
];
const PALETTE_DARK: [Color32; 8] = [
    Color32::from_rgb(0x39, 0x87, 0xe5),
    Color32::from_rgb(0xd9, 0x59, 0x26),
    Color32::from_rgb(0x19, 0x9e, 0x70),
    Color32::from_rgb(0xc9, 0x85, 0x00),
    Color32::from_rgb(0xd5, 0x51, 0x81),
    Color32::from_rgb(0x00, 0x83, 0x00),
    Color32::from_rgb(0x90, 0x85, 0xe9),
    Color32::from_rgb(0xe6, 0x67, 0x67),
];
/// A plot's ninth series and later: past the categorical palette.
const OVERFLOW: Color32 = Color32::from_rgb(0x8a, 0x8a, 0x85);

/// The dataviz reference palette, in its fixed order; validated in both modes.
fn series_color(i: usize, dark: bool) -> Color32 {
    let palette = if dark { &PALETTE_DARK } else { &PALETTE_LIGHT };
    palette.get(i).copied().unwrap_or(OVERFLOW)
}

fn number(raw: &str) -> f64 {
    form::parse_amount(raw).and_then(|d| d.to_f64()).unwrap_or_default()
}

/// The last value a series has: what its legend shows.
fn latest(values: &[Option<String>]) -> Option<&str> {
    values.iter().rev().find_map(|v| v.as_deref())
}

fn bucket_label(interval: Interval, period: Period, date: NaiveDate) -> String {
    match interval {
        Interval::Day if period == Period::Month => date.day().to_string(),
        Interval::Day | Interval::Week => date.format("%m-%d").to_string(),
        Interval::Month => date.format("%Y-%m").to_string(),
        Interval::Year => date.format("%Y").to_string(),
    }
}

fn period_label(run: &PanelRun) -> String {
    match run.period {
        Period::Month => run.from.format("%Y-%m").to_string(),
        Period::Year => run.from.format("%Y").to_string(),
        Period::Last12Months => format!("{} – {}", run.from.format("%Y-%m"), run.to.format("%Y-%m")),
    }
}

/// The panel's plots stacked, sharing the x-axis: bucket `i` at x = i.
fn draw_run(ui: &mut egui::Ui, run: &PanelRun) {
    let labels: Vec<String> =
        run.buckets.iter().map(|b| bucket_label(run.interval, run.period, *b)).collect();
    let n = run.plots.len().max(1);
    let dark = ui.visuals().dark_mode;
    for (i, plot) in run.plots.iter().enumerate() {
        if let Some(title) = &plot.title {
            ui.strong(title);
        }
        let left = n - i;
        let height = ((ui.available_height() - 4.0) / left as f32).max(140.0);
        draw_plot(ui, i, plot, &labels, height, dark);
    }
}

fn draw_plot(ui: &mut egui::Ui, i: usize, plot: &PlotRun, labels: &[String], height: f32, dark: bool) {
    let count = labels.len();
    let axis_labels = labels.to_vec();
    let hover_labels = labels.to_vec();
    let has_bars = plot.series.iter().any(|s| s.mark == Mark::Bar);
    let mut p = Plot::new(("panel_plot", i))
        .legend(Legend::default())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_x(-0.5)
        .include_x(count as f64 - 0.5)
        .height(height)
        .y_axis_min_width(72.0)
        .link_axis("panel_x", [true, false])
        .link_cursor("panel_x", [true, false])
        .x_axis_formatter(move |mark, _| {
            let x = mark.value;
            if (x - x.round()).abs() > 1e-6 || x < 0.0 {
                return String::new();
            }
            axis_labels.get(x.round() as usize).cloned().unwrap_or_default()
        })
        .y_axis_formatter(|mark, _| grouped(&format!("{}", mark.value.round() as i64)))
        .label_formatter(move |pos: &HoverPosition<'_>| match pos {
            HoverPosition::NearDataPoint { plot_name, position, .. } => Some(format!(
                "{plot_name}\n{}: {}",
                hover_labels.get(position.x.round().max(0.0) as usize).cloned().unwrap_or_default(),
                grouped(&format!("{:.2}", position.y)),
            )),
            HoverPosition::Elsewhere { .. } => None,
        });
    // Bars read from zero; lines (a balance) are scaled to their data.
    if has_bars {
        p = p.include_y(0.0);
    }
    p.show(ui, |pu| {
        let mut bars: Vec<BarChart> = vec![];
        for (j, s) in plot.series.iter().enumerate() {
            let color = series_color(j, dark);
            let name = format!("{}  {}", s.label, latest(&s.values).map_or("–".to_owned(), grouped));
            match s.mark {
                Mark::Line => {
                    let points: Vec<[f64; 2]> = s
                        .values
                        .iter()
                        .enumerate()
                        .filter_map(|(x, v)| v.as_deref().map(|v| [x as f64, number(v)]))
                        .collect();
                    let mut line = Line::new(name, PlotPoints::from(points)).color(color).width(2.0);
                    if s.dashed {
                        line = line.style(LineStyle::dashed_loose());
                    }
                    pu.line(line);
                }
                Mark::Bar => {
                    // Every bucket gets a bar, a null one at zero, so stacking lines up.
                    let items = s
                        .values
                        .iter()
                        .enumerate()
                        .map(|(x, v)| Bar::new(x as f64, v.as_deref().map_or(0.0, number)).width(0.7))
                        .collect();
                    let mut chart = BarChart::new(name, items).color(color);
                    if plot.stacked {
                        let below: Vec<&BarChart> = bars.iter().collect();
                        chart = chart.stack_on(&below);
                    }
                    bars.push(chart);
                }
            }
        }
        for chart in bars {
            pu.bar_chart(chart);
        }
    });
}
```

Remove the now-unused `BudgetReport`/`BudgetDay`/`BudgetReportParams` imports. Then remove from the api crate: `BUDGET_REPORT` (lib.rs), `BudgetReportParams` (params.rs) and `BudgetReport`, `BudgetDay` (records.rs), and their re-exports. `grep -rn "BudgetReport\|BUDGET_REPORT\|BudgetDay" crates cli desktop` must find nothing.

`main.rs` module doc: "the Charts screen draws the workspace's panels from `run_panel`".

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS and clean.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add desktop crates/sapphire-ledger-api
git commit -m "feat(desktop)!: the Charts screen draws panels; budgets get their panels on creation"
```

---

### Task 7: Desktop panel editor, preview and delete

**Files:**
- Modify: `desktop/src/form.rs` (`PanelForm`, `PlotRow`, `SeriesRow`, `Template`, `account_choices`, `move_item`)
- Modify: `desktop/src/charts.rs` (New panel / Edit / Delete, editor window, preview, delete confirmation)

**Interfaces:**
- Consumes: Task 6's `ChartsView`, `Reply`, `reload_now`, `spawn`, `draw_run`; `api::{templates, PanelDefinition, PanelSeries, Plot, SeriesQuery, Measure, Interval, Period, GroupBy, BudgetLine, Mark, AccountType, ADD_PANEL, UPDATE_PANEL, DELETE_PANEL, RUN_PANEL, AddPanelParams, UpdatePanelParams, DeletePanelParams, RunPanelParams}`; `form::split_list`.
- Produces: `form::{PanelForm, PlotRow, SeriesRow, Template, account_choices, move_item}` as below.

- [ ] **Step 1: Write the failing tests** — in `form.rs` tests:

```rust
    use sapphire_ledger_api::{BudgetLine, GroupBy, Interval, Mark, Measure, Period, templates};

    #[test]
    fn a_definition_survives_the_form() {
        for def in [templates::expenses_by_account(), templates::net_worth(), templates::income_and_expenses()] {
            assert_eq!(PanelForm::from_definition(None, &def).to_definition().unwrap(), def);
        }
        let b = budget_with_account();
        let def = templates::budget_balance(&b).unwrap();
        assert_eq!(PanelForm::from_definition(Some("p".into()), &def).to_definition().unwrap(), def);
    }

    #[test]
    fn a_switched_measure_leaves_its_old_fields_behind() {
        let mut form = PanelForm::from_definition(None, &templates::expenses_by_account());
        let row = &mut form.plots[0].series[0];
        row.measure = Measure::Budget;
        row.budget = Some("b1".into());
        let def = form.to_definition().unwrap();
        let q = &def.plots[0].series[0].query;
        assert!(q.account_types.is_empty() && q.group_by.is_none() && q.depth.is_none() && q.top.is_none());
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
        let accounts = vec![account("a1", "Expenses:Food:Lunch"), account("a2", "Assets:Bank")];
        assert_eq!(
            account_choices(&accounts),
            ["Assets", "Assets:Bank", "Expenses", "Expenses:Food", "Expenses:Food:Lunch"]
        );
    }

    #[test]
    fn templates_that_need_a_budget_say_so() {
        let b = budget_with_account();
        assert!(Template::BudgetPace.needs_budget());
        assert!(Template::BudgetPace.build(None).is_none());
        assert_eq!(Template::BudgetPace.build(Some(&b)).unwrap().name, "生活費 pace");
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
```

with helpers in the test module (reuse existing ones if `form.rs` tests already define `account(...)`; otherwise):

```rust
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
```

In `charts.rs` tests:

```rust
    #[test]
    fn a_saved_or_deleted_panel_is_followed() {
        let (_rt, mut view) = view();
        view.selected = Some("p1".into());
        view.replies.0.send(Reply::Written(Some("New".into()), Ok("created panel".into()))).unwrap();
        view.drain_replies();
        assert_eq!(view.select_name.as_deref(), Some("New"));
        view.replies.0.send(Reply::Deleted(Ok("deleted panel p1".into()))).unwrap();
        view.drain_replies();
        assert_eq!(view.selected, None);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-desktop`
Expected: FAIL to compile — `PanelForm` not found.

- [ ] **Step 3: Implement the form** — in `form.rs`:

```rust
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
                t => t.parse::<u32>().ok().filter(|n| *n >= 1).map(Some).ok_or_else(|| {
                    format!("{field} must be a whole number of 1 or more, not {t:?}")
                }),
            }
        };
        let label = Some(self.label.trim().to_owned()).filter(|l| !l.is_empty());
        let mut query = SeriesQuery { label, negate: self.negate, ..SeriesQuery::new(self.measure) };
        match self.measure {
            Measure::Budget => {
                query.budget = Some(self.budget.clone().ok_or("a budget series needs a budget")?);
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
        PlotRow { title: String::new(), stacked: false, series: vec![SeriesRow::new(Measure::Flow)] }
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
```

(`clippy::new_without_default`: add `impl Default` for `PanelForm` and `PlotRow` delegating to `new`, or `#[allow]` with a reason — prefer `Default`.)

- [ ] **Step 4: Implement the editor in `charts.rs`**

State on `ChartsView`: `editor: Option<PanelForm>`, `template: Template`, `template_budget: Option<String>` (budget id), `previewing: bool`, `confirm_delete: Option<Panel>`. `Reply` gains `Preview(u64, Result<PanelRun, String>)` and `Deleted(Result<String, String>)`; `drain_replies` handles them: a current `Preview(n, r)` sets `run = Some(r)`; `Deleted(Ok)` sets `selected = None`, `confirm_delete = None`, `reload_now()`; `Deleted(Err(e))` sets `form_error = Some(e)`. `Written(select, Ok)` also clears `editor` and `previewing`.

- `panel_list`: a "New panel" button above "Budgets…" → `editor = Some(PanelForm::new())`, `form_error = None`.
- `toolbar`: "Edit" (enabled with a shown panel) → `editor = Some(PanelForm::from_definition(Some(p.id.clone()), &p.definition()))`; "Delete" → `confirm_delete = Some(p.clone())`. While `previewing`, show `ui.weak("Preview — not saved")`.
- `refresh` does nothing while `previewing` (the preview stays until Save or Close).
- Delete confirmation window: "Delete panel {name}? Its file is removed." with "Delete" (spawns `DELETE_PANEL` → `Reply::Deleted`) and "Cancel".
- Editor window (`egui::Window::new(if id.is_some() {"Edit panel"} else {"New panel"})`, `.open(&mut open)`, `.resizable(true)`, `.default_width(560.0)`), its content in `egui::ScrollArea::vertical().max_height(520.0)`:
  1. Row: Template `ComboBox` over `Template::ALL` (by `label()`); when `template.needs_budget()`, a budget `ComboBox` over `self.budgets` setting `template_budget`; "Use" button → `if let Some(def) = template.build(budget) { *form = PanelForm::from_definition(form.id.clone(), &def) }`, else `form_error = Some("choose a budget (the balance template also needs one paid from accounts)")`.
  2. Grid: Name (`TextEdit`); Period (`ComboBox`: Month / Year / Last 12 months); Interval (`ComboBox`: Default / Day / Week / Month / Year → `Option<Interval>`); Currency (`TextEdit`, hint "base").
  3. For each plot `pi` (`ui.group`): Title, "Stacked" checkbox, and `↑ ↓ ✕` small buttons (record the action, apply after the loop with `move_item` / `remove`; ✕ disabled when only one plot). For each series `si` (`ui.group` inside): measure `ComboBox` (Flow / Balance / Budget), then:
     - Flow/Balance: an "Accounts (n)" `menu_button` listing `account_choices(&self.accounts)` with a checkbox each (toggling membership in `row.accounts`); five type checkboxes (`Asset … Expense`, toggling `row.account_types`); Tags `TextEdit` (hint "living, food"); Group by `ComboBox` (None / Account / Account type / Tag); when Account, Depth `TextEdit` (width 40); when not None, Top `TextEdit` (width 40).
     - Budget: budget `ComboBox` over `self.budgets` (by name, storing id); Line `ComboBox` (Pace / Remaining).
     - All: "Cumulative" checkbox (shown for Flow and for Budget+Pace); "Negate"; Mark `ComboBox` (Line / Bar); Label `TextEdit`; `↑ ↓ ✕` (✕ disabled when only one series).
     - "Add series" button → `plot.series.push(SeriesRow::new(Measure::Flow))`.
  4. "Add plot" → `form.plots.push(PlotRow::new())`.
  5. Error label (`form_error`); buttons "Preview" and "Save" (disabled while `sending`), spinner.
- Preview: `form.to_definition()` → `RunPanelParams { panel: None, definition: Some(def), anchor: None, offset: Some(self.offset) }` spawned as `Reply::Preview(self.generation.next(), …)`; set `previewing = true`. An invalid form sets `form_error`.
- Save: `to_definition()` → `ADD_PANEL` (`AddPanelParams { definition }`) for a new form, `UPDATE_PANEL` (`UpdatePanelParams { panel: id, definition }`) otherwise, spawned as `Reply::Written(Some(definition.name.clone()), …)`.
- Closing the editor (`!open`): `editor = None`, `form_error = None`, and if `previewing` { `previewing = false`; `reload_now()` }.

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS and clean.

- [ ] **Step 6: Commit**

```bash
cargo fmt
git add desktop
git commit -m "feat(desktop): edit, preview and delete panels from the Charts screen"
```

---

### Task 8: Documentation

**Files:**
- Modify: `docs/design.md` (Budget section's report text, a new Panel section after Budget, the MCP tools list, the series query's semantics), `README.md` and `README.ja.md` (the Charts screen and panels)

- [ ] **Step 1: Update `docs/design.md`**
  - In the Budget section, replace the `budget_report` description with: a budget is drawn by panels (`budget` series: `pace`, `remaining`); `add_budget` with `create_panels` makes `<name> pace` and `<name> balance` (the latter only with balance accounts).
  - Add `### Panel ([panel.rs](../crates/sapphire-ledger-core/src/panel.rs))` after Budget: the `panels/{id}.toml` layout (the spec's TOML example), periods and offsets, plots sharing the x-axis (never two y-axes), the five templates, the update-rewrites-known-fields note.
  - Add a `### Series query ([series.rs](../crates/sapphire-ledger-core/src/series.rs))` subsection: measures, filters (AND across, OR within; names match their sub-accounts), buckets (calendar-aligned, Monday weeks, clipped), actual vs planned (`today`, nulls), groups (`(untagged)`, `top` → `Other`, key order), limits (1000 / 50), one currency.
  - In `### Tools`: remove `budget_report`; add the six tools; the count 19 (plus the framework's 7).
- [ ] **Step 2: Update the READMEs** — a short "Charts" paragraph (both languages): the Charts screen on the left rail lists panels; a new budget makes its pace/balance panels; New panel offers templates; agents use `list_panels` / `run_panel` as query presets; `sapphire-ledger panel run <name>` prints one as a table. Mention `budget report` is replaced by `panel run`.
- [ ] **Step 3: Check**

Run: `grep -rn "budget_report\|budget report" docs/design.md README.md README.ja.md`
Expected: no hits except an explicit "replaced by" note if you wrote one.

- [ ] **Step 4: Commit**

```bash
git add docs/design.md README.md README.ja.md
git commit -m "docs: the series query, panels and the Charts screen"
```
