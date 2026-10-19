# Series query and panels: charts without an API change each

- **Scope**: a generic `query_series` method, panel records (`panels/{id}.toml`) that save a chart's queries, `run_panel` to run one as a preset, MCP tools and CLI for both, and a desktop Charts screen rebuilt on panels with an editor
- **Builds on**: [#43](https://github.com/sapphire-turtle/sapphire-ledger/pull/43) (budgets, default tags, the Charts screen) — this branch, `feat/panels`, is stacked on `feat/budgets-charts`
- **Replaces**: #43's `budget_report` (method, MCP tool, CLI `budget report`) and the Charts screen's two fixed tabs. Unreleased, so `sapphire-ledger-api` stays **2.0.0**

## What this is

#43 drew one budget's month through a method made for that chart. Every new
chart would need a method of its own — parameters, an MCP tool, a CLI
command. This replaces that with one query that most household charts are
made of: **how much flowed, or what was held, per period, filtered and
grouped**, plus a budget's pro-rated line. A **panel** saves a chart as data —
its period and the queries of each plot — so a new chart is a new file, not a
new API.

The same panels are an AI agent's **query presets**: `list_panels` shows what
has been saved, `run_panel` runs one, and an agent can save its own with
`add_panel`.

## What this is not

- **One currency.** A query reads one currency (default: `config.toml`'s
  `base_currency`) and ignores postings in others. Converting everything to the
  base currency through the price log is a later change; the display and the
  query stay single-currency even then.
- **No series arithmetic.** "Income minus expenses" is two series, not one. A
  `formula` series is the extension.
- **No dashboards, no grid layout.** One panel is shown at a time; a panel
  stacks its plots vertically.
- **No rename tracking for panel account filters.** A panel names accounts as
  strings; renaming an account leaves a panel naming the old one, and
  `validate_workspace` reports it.
- **No budget history** (as in #43).

## The query

### Parameters — `QuerySeriesParams`

| Field | Type | Meaning |
|---|---|---|
| `from`, `to` | date | The range, both ends included. `from <= to`. |
| `interval` | `day` \| `week` \| `month` \| `year` | Bucket size. |
| `currency` | string? | Default `base_currency`. |
| `series` | `[SeriesQuery]` | At least one. |

`SeriesQuery`:

| Field | Type | For | Meaning |
|---|---|---|---|
| `measure` | `flow` \| `balance` \| `budget` | all | What is measured. |
| `accounts` | `[string]`? | flow, balance | Each an account id or a name. A name matches that account **and every account under it** (`Expenses:Food` matches `Expenses:Food:Lunch`), whether or not an account of exactly that name exists. An id matches that account and every account under its name. |
| `account_types` | `[AccountType]`? | flow, balance | |
| `tags` | `[string]`? | flow, balance | The transaction carries any of them. |
| `budget` | string? | budget | A budget id or name. Required for `budget`. |
| `line` | `pace` \| `remaining`? | budget | Default `pace`. |
| `group_by` | `none` \| `account` \| `account_type` \| `tag`? | flow, balance | Default `none`. |
| `depth` | integer ≥ 1? | `group_by = account` | Group by the first `depth` segments of the name. Default: the full name. |
| `top` | integer ≥ 1? | grouped | Keep the `top` groups with the largest absolute total over the range; sum the rest into one group `Other`. |
| `cumulative` | bool? | flow, budget pace | Running total from `from`. Default false. |
| `negate` | bool? | all | Flip the sign (income as positive). Default false. |
| `label` | string? | all | |

Filters combine with AND across fields and OR within a list; an absent or
empty list does not filter. A field that does not apply to the measure is an
error (`tags` on a `budget` series), so a mistake is not silently ignored.

### Buckets

Calendar-aligned, clipped to the range: `day`; `week` starts Monday; `month`;
`year`. A bucket is named by its first date inside the range. At most **1000
buckets** per query.

### Measures

- **flow** — per bucket, the sum over matching postings of their
  `balance_contribution()` in the query currency (as #43 counted spending):
  expenses positive, income negative. The account filters apply to the
  posting's account, `tags` to its transaction.
- **balance** — at the end of each bucket, the sum over matching postings of
  their **raw amount** in the query currency (what the accounts hold), from
  the start of the books.
- **budget pace** — per day, `monthly_amount / days in that day's month`;
  per bucket, the sum of its days. A range across months of different lengths
  is exact. The budget's currency must be the query currency, or the query
  is an error.
- **budget remaining** — the budget's `balance_accounts` balance at the end of
  the day before `from` (raw amounts, budget currency), minus the cumulative
  pace up to the end of each bucket. Always cumulative. A budget with no
  balance accounts is an error for this line.

**Actual vs planned.** `flow` and `balance` count only postings dated on or
before **today**, and a bucket that starts after today is `null`. Budget lines
cover the whole range. `today` is a parameter of the core function, so the
same books give the same answer.

### Groups and limits

`group_by` splits a series into one result series per group, ordered by group
key (so a colour that follows order follows the group, not its rank). For
`tag`, a transaction with several tags counts in each; one with none goes to
the group `(untagged)`. `top` folds the rest into `Other`, placed last. At
most **50 result series** per query, counting groups.

### Result — `SeriesResult`

```
{ currency, from, to, interval,
  buckets: [date],
  series: [{ index, group?, label, values: [decimal | null] }] }
```

`index` is the position of the `SeriesQuery` it came from. `label` is the
query's `label`, then the group, joined by ` — `; without either, a default
(`Flow`, `Balance`, `<budget name> pace`, `<budget name> remaining`). Values
are rounded as #43's (`round_amount`: 2 dp, midpoint away from zero,
normalized).

## Panels

### Record — `panels/{id}.toml`

```toml
id = "…"                      # random, as budgets
name = "Living pace"          # unique among panels
period = "month"              # month | year | last_12_months
interval = "day"              # optional; month → day, others → month
# currency = "JPY"            # optional; base_currency
created_at = 2026-10-17T…
updated_at = 2026-10-17T…

[[plots]]
title = "Cumulative"          # optional
stacked = false               # optional; stacks the plot's bars

[[plots.series]]              # a SeriesQuery, plus:
measure = "flow"
tags = ["living"]
account_types = ["Expense"]
cumulative = true
label = "Spent"
mark = "line"                 # line | bar; default line
```

A panel has at least one plot, each with at least one series. The plots
share the x-axis and each has its own y-axis — a different scale is another
plot, never a second axis. Budget series are drawn dashed.

### Period

A panel's range is relative to an **anchor** date (default today), moved by
an integer **offset** of periods:

| `period` | Range | One offset step |
|---|---|---|
| `month` | the calendar month containing the anchor | 1 month |
| `year` | the calendar year | 1 year |
| `last_12_months` | 12 calendar months ending with the anchor's month | 1 month |

The range is computed in core (`panel_range(period, anchor, offset)`) so the
desktop, MCP and CLI agree.

### Validation

`add_panel` / `update_panel` reject a structurally bad definition: no plots,
a plot with no series, a field that does not apply to the measure, `budget`
missing on a budget series, an empty name, a duplicate name. On the books,
`validate_workspace` reports a panel whose budget does not resolve or whose
account filter matches no account.

### Templates

Plain functions in `sapphire-ledger-api` (`templates` module) that build a
`PanelDefinition`, so the server and the desktop share them:

| Template | Period / interval | Plots |
|---|---|---|
| `budget_pace(budget)` | month / day | **Cumulative**: flow (budget's tag, Expense, cumulative) "Spent", budget pace cumulative "Budget". **Daily**: flow bar "Spent", budget pace "Budget". |
| `budget_balance(budget)` | month / day | balance of the budget's balance accounts "Balance", budget remaining "Plan". |
| `expenses_by_account()` | last_12_months / month | flow of Expense, group by account depth 2, top 7, bar, stacked. |
| `net_worth()` | last_12_months / month | balance of Asset and Liability, "Net worth". |
| `income_and_expenses()` | last_12_months / month | flow of Income negated "Income" (bar), flow of Expense "Expenses" (bar). |

## API — `sapphire-ledger-api` 2.0.0 (unchanged version)

Removed: `BUDGET_REPORT`, `BudgetReportParams`, `BudgetReport`, `BudgetDay`.

Added:

| Constant | Params | Answer |
|---|---|---|
| `QUERY_SERIES` | `QuerySeriesParams` | `SeriesResult` |
| `LIST_PANELS` | `EmptyParams` | `[Panel]` |
| `RUN_PANEL` | `RunPanelParams { panel?, definition?, anchor?, offset? }` — exactly one of `panel` (id or name) and `definition` | `PanelRun` |
| `ADD_PANEL` | `AddPanelParams { definition }` | confirmation |
| `UPDATE_PANEL` | `UpdatePanelParams { panel, definition }` — replaces the definition whole | confirmation |
| `DELETE_PANEL` | `DeletePanelParams { panel }` | confirmation |

`PanelDefinition { name, period, interval?, currency?, plots: [Plot] }`,
`Plot { title?, stacked?, series: [PanelSeries] }`, `PanelSeries` = the
`SeriesQuery` fields flattened plus `mark?`. `Panel` = `id`, the definition
flattened, `created_at`, `updated_at`.

`PanelRun { panel?: id, name, period, from, to, interval, currency, buckets,
plots: [{ title?, stacked, series: [{ label, group?, mark, dashed, values }] }] }`
— the panel's queries run as one `query_series`, each result series placed in
its plot. `definition` lets the desktop preview an unsaved panel.

`AddBudgetParams` gains `create_panels: bool` (default **false**): also create
`<budget name> pace` and `<budget name> balance` from the templates. If either
name is taken, nothing is written. The confirmation names the panels' ids.

Update and delete rewrite or remove the file; as #43's updates, the file is
rewritten from its known fields and unknown keys and comments are not kept.

## Server and MCP

Methods for each constant above, in `methods.rs`, as #43's. MCP tools
(ledger tools: 14 − `budget_report` + 6 = **19**, plus the framework's 7):

- `query_series`
- `list_panels`
- `run_panel` — its description says: look in `list_panels` for a saved
  preset first and run it; use `query_series` when none fits; `offset: -1` is
  the previous period
- `add_panel` — "save a query you will run again as a preset"
- `update_panel`, `delete_panel`

The server instructions mention panels as presets in place of
`budget_report`.

## CLI

`budget report` is removed. Added:

- `sapphire-ledger panel list`
- `sapphire-ledger panel run <panel> [--anchor YYYY-MM-DD] [--offset N]` — a
  table per plot: one row per bucket, one column per series; `-` for null

## Core

- `panel.rs`: the panel record, `Period`, `panel_range`, structural
  validation.
- `report.rs`: `query_series(ws, &SeriesQuery…, today) -> SeriesResult`
  (core's own types) and `run_panel`. `budget_report`, `BudgetDay`,
  `BudgetReport` and `parse_month` go; `round_amount` stays.
- `ops.rs`: `create_panel`, `update_panel`, `delete_panel`;
  `create_budget(…, create_panels)`.
- `validate.rs`: the panel issues above.
- `workspace.rs` / `repository.rs`: `PANELS_DIR = "panels"`, load and save.

## Desktop

### Charts screen

- **Left list**: the panels, a **New panel** button, and **Budgets…**, which
  opens #43's budget form (pick a budget to edit, or new).
- **Top bar**: the panel's name, `◀ <period label> ▶` (offset), **Edit**,
  **Delete** (with a confirmation).
- **Body**: `run_panel` drawn as stacked plots with a linked x-axis, as #43's
  Daily pace. Each legend entry shows the series' latest value (the last
  non-null). #43's "ahead of / behind pace" text goes.
- **Colours**: a fixed categorical order of 8, starting with #43's blue and
  orange; a plot's 9th and later series are grey. Budget series dashed.
- **States**: loading via the generation counter (as #43); a query error shown
  in the body; with no panels, a pointer to New panel.

### Budget form

New budgets get a **Create panels** checkbox, on by default
(`create_panels`). After saving, the panel list reloads and selects
`<budget name> pace` — so a new budget is shown at once (#43's deferred
"new budget is not selected").

### Panel editor (a window, as the other forms)

- **Template** picker (the five); budget templates ask for the budget. Picking
  one fills the form.
- Name, period, interval, currency.
- Plots: title, stacked; add, remove, move up/down.
- Series rows: measure first; then only its fields — flow/balance: accounts
  (multi-select from the accounts and their parent names), types, tags,
  group by, depth, top; budget: budget, line — and for all: cumulative,
  negate, mark, label. Add, remove, move up/down.
- **Preview** runs the unsaved definition (`run_panel` with `definition`) in
  the body; **Save** calls `add_panel` / `update_panel` and selects it.

## Testing

- **core**: flow and balance with each filter and their combination; name
  matching under a parent with and without an exact account; group by
  account (with depth), account type, tag (multi-tag, untagged); `top` with
  `Other`; budget pace across months of 28/30/31 days; remaining with and
  without balance accounts; currency mismatch; null after today, and only
  postings up to today counted; week buckets clipped at both ends; the 1000
  bucket and 50 series limits; rounding; `panel_range` for each period with
  offsets across a year boundary; structural validation; `create_budget` with
  panels, and with a taken name writing nothing; panel issues in
  `validate_workspace`.
- **api**: serde round trips, including a panel through TOML
  (`[[plots.series]]`); templates produce valid definitions.
- **mcp**: the tool list (19 + 7); `run_panel` by name and by definition;
  `budget_report` is gone.
- **cli**: `panel run` output for a fixture.
- **desktop**: pure functions — template filling, latest value, period label,
  series colour — unit-tested; then a visual check against an isolated server
  and scratch ledger.

## Documentation

`docs/design.md`: a Panel section beside Budget, the query's semantics, the
tool list; `budget_report` removed. README: the Charts screen (#43's deferred
item).
