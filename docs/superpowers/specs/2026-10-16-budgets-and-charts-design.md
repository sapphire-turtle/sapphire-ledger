# Budgets and charts: a monthly budget, compared day by day

- **Scope**: account default tags, a budget record, a `budget_report` method and MCP tool, `budget report` on the CLI, and a Charts screen on the desktop's rail
- **Builds on**: [#42](https://github.com/sapphire-turtle/sapphire-ledger/pull/42), which moved the desktop onto the framework's `Shell` (left rail)
- **Reverses**: the restart design's "budgets are explicitly not planned" ([2026-09-01](2026-09-01-restart-roadmap-design.md#deferred))

## What this is

A household sets a monthly budget — "living expenses, 100,000 JPY a month" —
and wants to see, at a glance, whether this month is on pace. Two views answer
that:

1. **Daily pace**: the budget spread evenly over the month's days, cumulated,
   against what was actually spent, cumulated. Each day's spending is shown
   as a bar beneath.
2. **Balance path**: the balance of the account(s) the budget is paid from,
   as planned (month-start balance minus the daily budget per elapsed day)
   against as it actually went.

Both live on a new **Charts** destination on the desktop's rail, beside
Ledger.

What counts as "living expenses" is decided by **tag**: a budget names one
tag, and the transactions carrying it are the budget's. So that the tag need
not be typed every time, an **account can carry default tags**, copied into a
transaction when it is recorded.

## What this is not

- **No budget history.** A budget has one `monthly_amount`; changing it
  redraws past months with the new amount. If that bites, an
  `effective_from` list is the extension.
- **No income planning, no multi-month cash-flow forecast.** One budget, one
  month at a time.
- **No inherited tags at report time.** Default tags are copied on write; the
  file's tags are the whole truth. Changing an account's defaults leaves past
  transactions alone.
- **No CLI `budget add` / `update`, no CLI `account update`.** Those go
  through MCP and the desktop for now; the CLI gets `budget report` only.

## Data model

### Account default tags

`Account` gains an optional `default_tags` front-matter field:

```yaml
---
id: 0f2e9a1
name: Expenses:Food
type: Expense
opened_at: 2026-05-01
default_tags:
- living
---
```

Omitted or empty means none. Values are written quoted like other tags.

### Copying default tags on write

`core::ops::create_transaction` takes an `apply_default_tags: bool`:

- `true` — the stored tags are the given tags, then every default tag of
  every posting's account not already present, in posting order. Order is
  stable and duplicates are dropped.
- `false` — the stored tags are exactly the given tags.

The CLI and MCP pass `true` (the API's serde default). The desktop's form
merges the defaults into its tags field as accounts are picked — so they are
visible and removable — and sends `false`, so what the field shows is what is
stored.

### Budget record

`budgets/{grain-id}.toml`. TOML, because it has no free text; no
`{year}/{MM}/` directory, because a budget is not dated. The id is minted
with `GrainId::random()` like an account's, checked against the budgets on
disk.

```toml
id = "b1c2d3e"
name = "生活費"
tag = "living"
currency = "JPY"
monthly_amount = "100000"
created_at = "2026-10-16T10:00:00+09:00"
updated_at = "2026-10-16T10:00:00+09:00"

[[balance_accounts]]
account_id = "4h6j2k9"            # authoritative; account_name is denormalized
account_name = "Assets:Bank:Living"
```

- `monthly_amount` is a string-encoded `Decimal`, positive.
- `balance_accounts` may be empty; the balance view then says so instead of
  drawing. References follow the account-reference rules (id wins; either
  alone accepted on read).
- `name` must be unique among budgets; it is what people type.

### Validation additions

`Workspace::validate()` additionally reports:

- Two budgets share an `id` or a `name`.
- A budget's `balance_accounts` entry references no existing account, or no
  account at all.
- A budget's `monthly_amount` is not positive.

## The report

`core::report::budget_report(ws, budget, month, today) -> BudgetReport`, a
pure function: `today` is a parameter so tests fix it.

For a month of `n` days:

- `daily_budget = monthly_amount / n`.
- Every amount the report answers with is rounded to 2 decimal places
  (midpoint away from zero) and normalized (`100000`, not `100000.00`);
  the computation itself runs at full `Decimal` precision.
- A **budget posting** is a posting of a transaction whose tags contain the
  budget's `tag`, to an account of type `Expense`, whose
  `balance_contribution()` is in the budget's `currency`. Its amount is that
  contribution, signed: a refund (negative) reduces spending.
- `spent(d)` = sum of budget postings dated `d`;
  `spent_cumulative(d)` = sum over the month's days up to and including `d`.
- `budget_cumulative(d) = monthly_amount × day_of_month(d) / n` — multiplied
  first, so the last day is exactly `monthly_amount`.
- `opening_balance` = sum over `balance_accounts` of the `amount` of
  postings whose own `currency` is the budget's, dated before the 1st of the
  month. A balance is what the account holds, so an inline price does not
  convert it.
- `planned_balance(d) = opening_balance − budget_cumulative(d)`.
- `elapsed(d) = d <= today`. Clients draw the actual lines (spent, actual
  balance) only over elapsed days, so they stop at today.
- `actual_balance(d)` = the same sum as `opening_balance`, up to and
  including `d`; `None` when `d` is not elapsed or `balance_accounts` is
  empty. For a month wholly in the past every day is elapsed; for a month
  wholly in the future none is.

`days` always holds every day of the month, so the x-axis is the whole month.

## API: `sapphire-ledger-api` 2.0.0

Adding public fields to `Account` and `AddTransactionParams` breaks struct
literals, which cargo-semver-checks (run by release-plz) flags, so the crate
goes to **2.0.0** and `API_VERSION` to 2. The handshake then refuses a 1.x
desktop against a 2.x server; both ship from this repository together. On the
wire every new field has a serde default, so the JSON stays readable either
way.

Changed:

- `Account.default_tags: Vec<String>` (default empty, skipped when empty)
- `AddAccountParams.default_tags: Vec<String>` (default empty)
- `AddTransactionParams.apply_default_tags: bool` (default `true`)

New methods:

| Constant | Method | Params | Answers |
|---|---|---|---|
| `UPDATE_ACCOUNT` | `sapphire-ledger.update_account` | `UpdateAccountParams { account, default_tags: Option<Vec<String>> }` | confirmation text |
| `LIST_BUDGETS` | `sapphire-ledger.list_budgets` | `EmptyParams` | `Vec<Budget>` |
| `ADD_BUDGET` | `sapphire-ledger.add_budget` | `AddBudgetParams { name, tag, currency, monthly_amount, balance_accounts: Vec<String> }` | confirmation text |
| `UPDATE_BUDGET` | `sapphire-ledger.update_budget` | `UpdateBudgetParams { budget, name?, tag?, currency?, monthly_amount?, balance_accounts? }` | confirmation text |
| `BUDGET_REPORT` | `sapphire-ledger.budget_report` | `BudgetReportParams { budget, month: "YYYY-MM" }` | `BudgetReport` |

`account`, `budget` and each `balance_accounts` entry accept an id or a name.
`update_*` leave a `None` field unchanged; `update_account` with no field set
is refused rather than rewriting the file for nothing. `update_account`
covers `default_tags` only for now; a rename or other field is a later,
additive `Option`.

New records:

```rust
pub struct Budget {
    pub id: String,
    pub name: String,
    pub tag: String,
    pub currency: String,
    pub monthly_amount: String,          // Decimal as string
    pub balance_accounts: Vec<AccountRef>,
}
pub struct AccountRef { pub account_id: Option<String>, pub account_name: Option<String> }

pub struct BudgetReport {
    pub budget: Budget,
    pub month: String,                   // YYYY-MM
    pub daily_budget: String,
    pub opening_balance: String,
    pub days: Vec<BudgetDay>,
}
pub struct BudgetDay {
    pub date: NaiveDate,
    pub spent: String,
    pub spent_cumulative: String,
    pub budget_cumulative: String,
    pub planned_balance: String,
    pub elapsed: bool,                   // date <= today
    pub actual_balance: Option<String>,  // None if not elapsed or no balance accounts
}
```

`today` for `budget_report` is the server's local date.

## Server and MCP

`sapphire-ledger-mcp`'s `methods.rs` registers the five methods against the
current workspace's `LedgerState`, converting through `wire.rs` as today.
`tools.rs` exposes them as tools: `list_budgets` and `budget_report` (read),
`add_budget`, `update_budget` and `update_account` (write). The
`add_transaction` tool's description gains one sentence: the posting
accounts' default tags are added unless `apply_default_tags` is false.

Core gains `budget.rs` (the record, its path `budgets/{id}.toml`, load into
`Workspace`), `report.rs`, and in `ops.rs` `create_budget`, `update_budget`
and `update_account`, each with the existing refuse-to-overwrite and
validate-before-write discipline. `update_*` bump `updated_at`.

## CLI

`sapphire-ledger budget report <budget> [--month YYYY-MM]` (default: this
month) prints the report as a table: date, spent, cumulative spent,
cumulative budget, planned balance, actual balance. It reads the workspace
directly like the CLI's other read commands.

## Desktop

### Rail

`Screen::Charts`, with Lucide's `chart-line.svg` under `desktop/assets/icons/`
(covered by the `LICENSE-Lucide` #42 added), labelled "Charts", after Ledger.

### Charts screen — `desktop/src/charts.rs`

A `ChartsView` alongside `LedgerView`, sharing its `LedgerClient` (an `Arc`)
and its reply-channel pattern; it never blocks.

```
┌ Budget: [生活費 ▾]  ◀ 2026-10 ▶          [New budget] [Edit] ┐
│ 62,300 / 100,000 JPY · 3,226 / day · 10,600 under pace today │
│ ( Daily pace | Balance path )                                │
│                                                              │
│   plot                                                       │
└──────────────────────────────────────────────────────────────┘
```

- Calls `list_budgets`, then `budget_report` for the chosen budget and month;
  refreshes every 10 s like Ledger, and immediately on a change of budget or
  month or after a budget is saved.
- **Daily pace**: cumulative budget (dashed line) and cumulative spent
  (solid line); beneath it, a shorter plot with its own y-axis and the same
  linked x-axis shows each day's spending as bars against the daily budget
  (dashed horizontal line). Two plots, never two y-scales on one. **Balance path**: planned balance (dashed),
  actual balance (solid, to today). Hover shows the day's values; a legend
  names the series. Actual = blue (`#2a78d6` light / `#3987e5` dark), budget
  and plan = orange (`#eb6834` / `#d95926`), dashed; the pair passes the
  dataviz validator in both modes, and dashed-vs-solid is the secondary
  encoding.
- Summary line: spent so far / monthly amount; daily budget; under or over
  pace by `budget_cumulative(today) − spent_cumulative(today)` (for the
  current month only).
- No budgets → a line pointing at New budget. No `balance_accounts` → the
  Balance path tab says so.
- Plotting with `egui_plot` 0.37 (requires egui ^0.36, matching the
  framework's egui 0.36).

### Budget form

A window with name, tag, currency, monthly amount, and a multi-select of
Asset accounts. New sends `add_budget`; Edit pre-fills from the selected
budget and sends `update_budget`. Errors keep the window open with what was
typed, like the existing forms.

### Ledger screen changes

- New account form: a "Default tags" field (comma-separated).
- The selected account's heading gets an "Edit tags" button: a small window
  that sends `update_account`.
- Transaction form: choosing a posting's account appends that account's
  default tags to the tags field (no duplicates). The request carries
  `apply_default_tags: false`.

## Testing

TDD throughout.

- **core**: `budget_report` on small fixed workspaces — even split over 31-,
  30- and 28-day months; refunds; untagged and non-Expense postings
  excluded; postings in another currency excluded unless their contribution
  is in the budget's; opening balance; `actual_balance` is `None` after
  `today`; past and future months. `create_transaction` with and without
  default tags (union, order, duplicates). Budget record round trip.
  `create_budget` / `update_budget` / `update_account`. New validation
  issues.
- **mcp / cli**: the new methods and tools in `cli/tests/mcp_tools.rs` and
  `write_commands.rs`'s style; `budget report` output in the CLI tests.
- **desktop**: `form.rs` pure functions (default-tag merge, budget form
  `to_params`); the client test gains a `budget_report` round trip.
- **Finish**: `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, and the app run to look at both charts.

## Documentation

`docs/design.md` gains the budget record, account default tags and the
report's rules; the restart design's deferred list notes budgets are now
built.

## Branch

`feat/budgets-charts`, from `feat/desktop-shell` (#42, open). Once #42 merges
the PR targets `main`.
