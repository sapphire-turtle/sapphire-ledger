# Budgets and Charts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a household set a monthly budget selected by tag, give accounts default tags copied into new transactions, and compare the budget with actual spending and balance on a new Charts screen on the desktop's rail.

**Architecture:** `sapphire-ledger-core` owns the new records (`Account.default_tags`, `budgets/{id}.toml`), the write ops and a pure `budget_report`. `sapphire-ledger-api` 2.0.0 carries the new fields, methods and records; `sapphire-ledger-mcp` serves them as app-server methods and MCP tools; the CLI gets `budget report`. The egui desktop adds a `ChartsView` that renders `budget_report` with `egui_plot`, plus form changes on the Ledger screen.

**Tech Stack:** Rust 2024, serde/TOML/YAML front matter, `rust_decimal`, `chrono`, rmcp via sapphire-framework, egui 0.36 + `egui_plot` 0.37.

**Spec:** `docs/superpowers/specs/2026-10-16-budgets-and-charts-design.md`

## Global Constraints

- Branch `feat/budgets-charts`, based on `feat/desktop-shell` (PR #42). Do not rebase onto `main` until #42 merges.
- `sapphire-ledger-api` goes to **2.0.0**; `API_VERSION` becomes 2; every dependent's `version = "1.0.0"` requirement on it becomes `"2.0.0"`.
- Money is `rust_decimal::Decimal`, stored and sent as strings; no `f64` except at the final plotting step in the desktop.
- Report amounts on the wire are rounded to 2 dp, midpoint away from zero, then `normalize()`d (`100000`, not `100000.00`).
- Budget file: `budgets/{id}.toml`, TOML, id from `GrainId::random()` checked against existing budgets.
- Default tags are copied on write only; `apply_default_tags` defaults to `true` on the wire; the desktop sends `false`.
- Account references: id authoritative, either half accepted on read, both stored on write.
- Chart colours: actual = `#2a78d6` light / `#3987e5` dark; budget & plan = `#eb6834` light / `#d95926` dark, dashed; lines 2 px. Never two y-scales on one plot.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must pass at the end of every task.
- Commit messages: conventional commits, ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **A month change while a report is loading** — a slow reply for the old month must not overwrite the newly chosen month's chart. (Task 10: generation counter + `stale_replies_are_ignored` test.)
2. **February and 30-day months** — the x-axis and the daily budget follow the month's real length, and the cumulative budget ends exactly at `monthly_amount`. (Task 4: `february_has_28_days_and_ends_on_the_monthly_amount`.)
3. **An account referenced by a stale name or by id in `balance_accounts` of a hand-written budget** — resolves the way postings do; an unknown one is a validation issue, never a crash in the report. (Task 3 validation test, Task 4 `an_unresolvable_balance_account_is_skipped`.)
4. **Removing a default tag in the desktop form** — what the tags field shows is exactly what is stored. (Task 9: `the_form_sends_its_tags_as_shown` asserting `apply_default_tags == false`.)
5. **A budget renamed to an existing budget's name** — refused, nothing rewritten. (Task 3: `update_budget_refuses_a_name_already_taken`.)

---

## File map

| File | Change |
|---|---|
| `crates/sapphire-ledger-core/src/account.rs` | `default_tags` field |
| `crates/sapphire-ledger-core/src/budget.rs` | **new**: `Budget`, `AccountRef` |
| `crates/sapphire-ledger-core/src/report.rs` | **new**: `budget_report`, `parse_month`, `round_amount` |
| `crates/sapphire-ledger-core/src/ops.rs` | default-tag merge; `update_account`; `create_budget`; `update_budget`; `BudgetChanges` |
| `crates/sapphire-ledger-core/src/workspace.rs` | `BUDGETS_DIR`, `budget_relative_path` |
| `crates/sapphire-ledger-core/src/repository.rs` | `Workspace.budgets`, load them |
| `crates/sapphire-ledger-core/src/validate.rs` | budget issues |
| `crates/sapphire-ledger-core/src/lib.rs` | modules and re-exports |
| `crates/sapphire-ledger-core/tests/{ops_create,budgets,report}.rs` | tests |
| `crates/sapphire-ledger-api/src/{lib,params,records}.rs`, `Cargo.toml` | 2.0.0 |
| `crates/sapphire-ledger-mcp/src/{wire,methods,tools}.rs` | new methods and tools |
| `cli/src/{cli,commands,main}.rs`, `cli/tests/{write_commands,mcp_tools}.rs` | `budget report` |
| `desktop/Cargo.toml` | `egui_plot = "0.37"` |
| `desktop/src/form.rs` | tag merge, `TagsForm`, `BudgetForm` |
| `desktop/src/ledger.rs` | default-tag UI, shared client |
| `desktop/src/charts.rs` | **new**: `ChartsView` |
| `desktop/src/main.rs`, `desktop/assets/icons/chart-line.svg` | rail destination |
| `docs/design.md`, `docs/superpowers/specs/2026-09-01-restart-roadmap-design.md` | docs |

---

### Task 1: Account default tags, copied into new transactions (core)

**Files:**
- Modify: `crates/sapphire-ledger-core/src/account.rs` (struct `Account`)
- Modify: `crates/sapphire-ledger-core/src/ops.rs` (`create_account`, `create_transaction`, new helpers)
- Modify call sites: `cli/src/commands.rs:35,61`, `crates/sapphire-ledger-mcp/src/methods.rs:438,474,608,617`, `crates/sapphire-ledger-core/tests/{ops_create,state_reload,migrate,account_refs}.rs`, `crates/sapphire-ledger-mcp/src/wire.rs` tests, `desktop/src/ledger.rs` tests do **not** change yet (they use the API's `Account`)
- Test: `crates/sapphire-ledger-core/tests/ops_create.rs`

**Interfaces:**
- Produces:
  - `Account.default_tags: Vec<String>` (serde default, skipped when empty), declared after `closed_at`, before `description`.
  - `pub fn normalize_tags(tags: Vec<String>) -> Vec<String>` in `ops` — trims, drops empty, drops duplicates, keeps first-seen order.
  - `ops::create_account(root, name, account_type, currencies, opened_at, description, default_tags: Vec<String>)`
  - `ops::create_transaction(root, date, narration, payee, tags, status, postings, comment, apply_default_tags: bool)`

- [ ] **Step 1: Write the failing tests** — append to `crates/sapphire-ledger-core/tests/ops_create.rs` (extend the top `use` to `use sapphire_ledger_core::{Account, AccountType, Balance, Posting, Transaction, load_markdown, ops};`):

```rust
fn food_with_defaults(root: &std::path::Path, defaults: &[&str]) {
    ops::create_account(
        root,
        "Expenses:Food".into(),
        AccountType::Expense,
        vec![],
        "2026-01-01".parse().unwrap(),
        None,
        defaults.iter().map(|t| t.to_string()).collect(),
    )
    .expect("create_account");
}

fn groceries(root: &std::path::Path, tags: &[&str], apply: bool) -> Transaction {
    let (_, dest) = ops::create_transaction(
        root,
        "2026-05-21".parse().unwrap(),
        "groceries".into(),
        None,
        tags.iter().map(|t| t.to_string()).collect(),
        None,
        vec![
            posting_by_name("Expenses:Food", "1200"),
            posting_by_name("Assets:Cash", "-1200"),
        ],
        None,
        apply,
    )
    .expect("create_transaction");
    load_markdown(&dest).expect("read back")
}

#[test]
fn a_transaction_takes_its_accounts_default_tags_after_its_own() {
    let dir = ws();
    food_with_defaults(dir.path(), &["living", "food"]);
    add_account(dir.path(), "Assets:Cash", AccountType::Asset);
    let tx = groceries(dir.path(), &["food", "weekly"], true);
    assert_eq!(tx.tags, ["food", "weekly", "living"]);
}

#[test]
fn default_tags_are_left_out_when_not_applied() {
    let dir = ws();
    food_with_defaults(dir.path(), &["living"]);
    add_account(dir.path(), "Assets:Cash", AccountType::Asset);
    let tx = groceries(dir.path(), &["weekly"], false);
    assert_eq!(tx.tags, ["weekly"]);
}

#[test]
fn an_accounts_default_tags_are_stored_trimmed_and_once() {
    let dir = ws();
    food_with_defaults(dir.path(), &[" living ", "", "living", "food"]);
    let path = dir.path().join("accounts/Expenses/Food.md");
    let account: Account = load_markdown(&path).unwrap();
    assert_eq!(account.default_tags, ["living", "food"]);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("default_tags:"), "got: {text}");
}

#[test]
fn an_account_without_default_tags_does_not_write_the_key() {
    let dir = ws();
    add_account(dir.path(), "Assets:Cash", AccountType::Asset);
    let text = std::fs::read_to_string(dir.path().join("accounts/Assets/Cash.md")).unwrap();
    assert!(!text.contains("default_tags"), "got: {text}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-core --test ops_create`
Expected: compile error — `create_account` takes 6 arguments, `default_tags` no field.

- [ ] **Step 3: Implement**

In `account.rs`, inside `pub struct Account` after `closed_at`:

```rust
    /// Tags copied into a transaction that posts to this account, when it is
    /// recorded. Copied, not inherited: changing them leaves past
    /// transactions alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_tags: Vec<String>,
```

In `ops.rs`, after `resolve_postings`:

```rust
/// Trim each tag, drop empty ones and repeats, keeping first-seen order.
pub fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !out.iter().any(|t| t == tag) {
            out.push(tag.to_owned());
        }
    }
    out
}

/// `tags`, then every default tag of every posting's account not already
/// there, in posting order. The postings must already be resolved.
fn with_default_tags(mut tags: Vec<String>, postings: &[Posting], accounts: &[Account]) -> Vec<String> {
    for posting in postings {
        let Some(account) = accounts
            .iter()
            .find(|a| posting.account_id.as_deref() == Some(a.id.as_str()))
        else {
            continue;
        };
        for tag in &account.default_tags {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}
```

`create_account`: add a last parameter `default_tags: Vec<String>` and set `default_tags: normalize_tags(default_tags),` in the `Account { .. }` literal.

`create_transaction`: add a last parameter `apply_default_tags: bool`; after `let postings = resolve_postings(postings, &accounts)?;` add:

```rust
    let tags = if apply_default_tags {
        with_default_tags(tags, &postings, &accounts)
    } else {
        tags
    };
```

Update every call site (mechanical — the build tells you each one):
- `create_account(...)`: append `vec![]` after the `description` argument (`None` in tests). Files: `cli/src/commands.rs`, `crates/sapphire-ledger-mcp/src/methods.rs` (both the method and the two test calls near line 608/617), `crates/sapphire-ledger-core/tests/{ops_create,state_reload,migrate}.rs`.
- `create_transaction(...)`: append `true` as the last argument. Files: `cli/src/commands.rs`, `crates/sapphire-ledger-mcp/src/methods.rs`, `crates/sapphire-ledger-core/tests/ops_create.rs`.
- `core::Account { .. }` literals: add `default_tags: vec![],`. Files: `crates/sapphire-ledger-core/tests/{account_refs,migrate}.rs`, `crates/sapphire-ledger-mcp/src/wire.rs` (tests' `full`; `bare` uses `..full.clone()` so also set `default_tags: vec![]` there and give `full` `default_tags: vec!["living".into()]`).
- `wire::account` stays as is for now; the `the_wire_records_serialize_as_core_does` test will fail on `full` until Task 5 adds the API field — so in this task give `full` `default_tags: vec![]` and leave the `"living"` value for Task 5.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): copy accounts' default tags into new transactions"
```

---

### Task 2: `update_account` — change an existing account's default tags (core)

**Files:**
- Modify: `crates/sapphire-ledger-core/src/ops.rs`
- Test: `crates/sapphire-ledger-core/tests/ops_create.rs`

**Interfaces:**
- Consumes: `normalize_tags` (Task 1).
- Produces: `pub fn update_account(root: &Path, account: &str, default_tags: Vec<String>) -> Result<(String, PathBuf)>` — `account` is an id or a name (id first); returns the account's id and file path.

- [ ] **Step 1: Write the failing tests** (append to `ops_create.rs`):

```rust
#[test]
fn update_account_sets_default_tags_by_name_or_id_and_keeps_the_body() {
    let dir = ws();
    let (id, path) = ops::create_account(
        dir.path(),
        "Expenses:Food".into(),
        AccountType::Expense,
        vec![],
        "2026-01-01".parse().unwrap(),
        Some("食費".into()),
        vec![],
    )
    .unwrap();

    let (got, at) = ops::update_account(dir.path(), "Expenses:Food", vec!["living".into()]).unwrap();
    assert_eq!((got.as_str(), &at), (id.as_str(), &path));
    let account: Account = load_markdown(&path).unwrap();
    assert_eq!(account.default_tags, ["living"]);
    assert_eq!(account.description.as_deref(), Some("食費"));

    ops::update_account(dir.path(), &id, vec![]).unwrap();
    let account: Account = load_markdown(&path).unwrap();
    assert!(account.default_tags.is_empty());
}

#[test]
fn update_account_refuses_an_unknown_account() {
    let dir = ws();
    let err = ops::update_account(dir.path(), "Expenses:Nope", vec![]).unwrap_err();
    assert!(err.to_string().contains("Expenses:Nope"), "got: {err}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-core --test ops_create update_account`
Expected: compile error — `update_account` not found.

- [ ] **Step 3: Implement** (in `ops.rs`, after `create_account`):

```rust
/// Change an existing account's default tags. `account` is its id or its
/// name; an id wins over a name, as everywhere else.
///
/// The file is rewritten where it is: the path comes from the name, and the
/// name does not change here.
pub fn update_account(
    root: &Path,
    account: &str,
    default_tags: Vec<String>,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let loaded: Vec<(Account, PathBuf)> = walk_markdown_files(&root.join(ACCOUNTS_DIR))?
        .into_iter()
        .map(|p| Ok((load_markdown::<Account>(&p)?, p)))
        .collect::<Result<_>>()?;
    let (mut found, path) = loaded
        .iter()
        .find(|(a, _)| a.id == account)
        .or_else(|| loaded.iter().find(|(a, _)| a.name == account))
        .cloned()
        .ok_or_else(|| Error::Validation(format!("no account {account}")))?;
    found.default_tags = normalize_tags(default_tags);
    save_markdown(&path, &found)?;
    Ok((found.id, path))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sapphire-ledger-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): update an account's default tags"
```

---

### Task 3: Budget record, its writes and its validation (core)

**Files:**
- Create: `crates/sapphire-ledger-core/src/budget.rs`
- Modify: `crates/sapphire-ledger-core/src/{lib,workspace,repository,ops,validate}.rs`
- Modify: `crates/sapphire-ledger-core/tests/account_refs.rs` (`Workspace { .. }` literal gains `budgets: vec![]`)
- Test: `crates/sapphire-ledger-core/tests/budgets.rs` (new)

**Interfaces:**
- Produces:
  - `pub struct AccountRef { pub account_id: Option<String>, pub account_name: Option<String> }`
  - `pub struct Budget { id, name, tag, currency, monthly_amount: Decimal, created_at, updated_at, balance_accounts: Vec<AccountRef> }` (field order matters: `balance_accounts` last so TOML writes the array of tables after the plain keys)
  - `workspace::BUDGETS_DIR = "budgets"`, `workspace::budget_relative_path(id) -> PathBuf`
  - `Workspace.budgets: Vec<Budget>`
  - `ops::create_budget(root, name: String, tag: String, currency: String, monthly_amount: Decimal, balance_accounts: Vec<AccountRef>) -> Result<(String, PathBuf)>`
  - `#[derive(Default)] pub struct ops::BudgetChanges { pub name: Option<String>, pub tag: Option<String>, pub currency: Option<String>, pub monthly_amount: Option<Decimal>, pub balance_accounts: Option<Vec<AccountRef>> }`
  - `ops::update_budget(root, budget: &str, changes: BudgetChanges) -> Result<(String, PathBuf)>` — `budget` is an id or a name.
  - lib re-exports: `pub use budget::{AccountRef, Budget};`, `BUDGETS_DIR`, `budget_relative_path`.

- [ ] **Step 1: Write the failing tests** — `crates/sapphire-ledger-core/tests/budgets.rs`:

```rust
use rust_decimal::Decimal;
use sapphire_ledger_core::ops::{self, BudgetChanges};
use sapphire_ledger_core::{AccountRef, AccountType, Budget, load_toml, load_workspace};

fn ws() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    sapphire_ledger_core::init_workspace(dir.path(), "JPY").expect("init");
    for (name, ty) in [("Assets:Bank", AccountType::Asset), ("Expenses:Food", AccountType::Expense)] {
        ops::create_account(dir.path(), name.into(), ty, vec![], "2026-01-01".parse().unwrap(), None, vec![])
            .expect("account");
    }
    dir
}

fn by_name(name: &str) -> AccountRef {
    AccountRef { account_id: None, account_name: Some(name.into()) }
}

fn living(root: &std::path::Path) -> (String, std::path::PathBuf) {
    ops::create_budget(
        root,
        "生活費".into(),
        "living".into(),
        "JPY".into(),
        Decimal::from(100_000),
        vec![by_name("Assets:Bank")],
    )
    .expect("create_budget")
}

#[test]
fn a_budget_is_written_as_toml_with_resolved_accounts() {
    let dir = ws();
    let (id, dest) = living(dir.path());
    assert_eq!(dest, dir.path().join(format!("budgets/{id}.toml")));
    let budget: Budget = load_toml(&dest).unwrap();
    assert_eq!(budget.monthly_amount, Decimal::from(100_000));
    let bank = &budget.balance_accounts[0];
    assert!(bank.account_id.is_some(), "the id is stored");
    assert_eq!(bank.account_name.as_deref(), Some("Assets:Bank"));
    let text = std::fs::read_to_string(&dest).unwrap();
    assert!(text.contains("monthly_amount = \"100000\""), "got: {text}");

    let loaded = load_workspace(dir.path()).unwrap();
    assert_eq!(loaded.budgets.len(), 1);
    assert!(loaded.validate().is_empty(), "{:?}", loaded.validate());
}

#[test]
fn create_budget_refuses_bad_fields() {
    let dir = ws();
    let root = dir.path();
    living(root);
    let try_make = |name: &str, tag: &str, amount: i64, account: &str| {
        ops::create_budget(root, name.into(), tag.into(), "JPY".into(), Decimal::from(amount), vec![by_name(account)])
            .unwrap_err()
            .to_string()
    };
    assert!(try_make("生活費", "x", 1, "Assets:Bank").contains("already exists"));
    assert!(try_make("other", " ", 1, "Assets:Bank").contains("tag"));
    assert!(try_make("other", "x", 0, "Assets:Bank").contains("positive"));
    assert!(try_make("other", "x", 1, "Assets:Nope").contains("Assets:Nope"));
}

#[test]
fn update_budget_changes_only_what_is_given() {
    let dir = ws();
    let (id, dest) = living(dir.path());
    let before: Budget = load_toml(&dest).unwrap();
    let (got, at) = ops::update_budget(
        dir.path(),
        "生活費",
        BudgetChanges { monthly_amount: Some(Decimal::from(120_000)), ..Default::default() },
    )
    .unwrap();
    assert_eq!((got, at), (id.clone(), dest.clone()));
    let after: Budget = load_toml(&dest).unwrap();
    assert_eq!(after.monthly_amount, Decimal::from(120_000));
    assert_eq!(after.tag, "living");
    assert_eq!(after.created_at, before.created_at);
    assert!(after.updated_at >= before.updated_at);

    ops::update_budget(dir.path(), &id, BudgetChanges { balance_accounts: Some(vec![]), ..Default::default() }).unwrap();
    let after: Budget = load_toml(&dest).unwrap();
    assert!(after.balance_accounts.is_empty());
}

#[test]
fn update_budget_refuses_a_name_already_taken() {
    let dir = ws();
    living(dir.path());
    ops::create_budget(dir.path(), "旅行".into(), "trip".into(), "JPY".into(), Decimal::from(50_000), vec![]).unwrap();
    let err = ops::update_budget(
        dir.path(),
        "旅行",
        BudgetChanges { name: Some("生活費".into()), ..Default::default() },
    )
    .unwrap_err();
    assert!(err.to_string().contains("already exists"), "got: {err}");
}

#[test]
fn validation_reports_broken_hand_written_budgets() {
    let dir = ws();
    let budgets = dir.path().join("budgets");
    std::fs::create_dir_all(&budgets).unwrap();
    let record = |id: &str, name: &str, amount: &str, account: &str| {
        format!(
            "id = \"{id}\"\nname = \"{name}\"\ntag = \"living\"\ncurrency = \"JPY\"\n\
             monthly_amount = \"{amount}\"\ncreated_at = \"2026-10-16T10:00:00+09:00\"\n\
             updated_at = \"2026-10-16T10:00:00+09:00\"\n\n[[balance_accounts]]\naccount_name = \"{account}\"\n"
        )
    };
    std::fs::write(budgets.join("aaaaaaa.toml"), record("aaaaaaa", "a", "100", "Assets:Gone")).unwrap();
    std::fs::write(budgets.join("bbbbbbb.toml"), record("bbbbbbb", "a", "0", "Assets:Bank")).unwrap();
    let messages: Vec<String> = load_workspace(dir.path()).unwrap().validate().into_iter().map(|i| i.message).collect();
    let all = messages.join("\n");
    assert!(all.contains("Assets:Gone"), "{all}");
    assert!(all.contains("duplicate budget name a"), "{all}");
    assert!(all.contains("not positive"), "{all}");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-core --test budgets`
Expected: compile error — `AccountRef`, `Budget`, `create_budget` unknown.

- [ ] **Step 3: Implement**

`crates/sapphire-ledger-core/src/budget.rs`:

```rust
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
```

`workspace.rs`: next to `PRICES_DIR` add `pub const BUDGETS_DIR: &str = "budgets";`, and after `price_relative_path`:

```rust
/// Relative path for a budget: `budgets/{id}.toml`. A budget is not dated,
/// so there is no `{year}/{MM}/`.
pub fn budget_relative_path(id: &str) -> PathBuf {
    PathBuf::from(BUDGETS_DIR).join(format!("{id}.{TOML_EXTENSION}"))
}
```

`lib.rs`: `pub mod budget;`, `pub use budget::{AccountRef, Budget};`, add `BUDGETS_DIR, budget_relative_path` to the `workspace::` re-export.

`repository.rs`: import `crate::budget::Budget` and `BUDGETS_DIR`; add `pub budgets: Vec<Budget>,` to `Workspace` after `prices`; in `load_workspace` before `Ok(Workspace {`:

```rust
    let budgets = walk_toml_files(&root.join(BUDGETS_DIR))?
        .iter()
        .map(|p| load_toml::<Budget>(p))
        .collect::<Result<Vec<_>>>()?;
```

and `budgets,` in the literal.

`ops.rs` (imports: `crate::budget::{AccountRef, Budget}`, `BUDGETS_DIR`, `budget_relative_path`):

```rust
/// Every budget on disk, with its path.
fn load_budgets(root: &Path) -> Result<Vec<(Budget, PathBuf)>> {
    walk_files(&root.join(BUDGETS_DIR), TOML_EXTENSION)?
        .into_iter()
        .map(|p| Ok((load_toml::<Budget>(&p)?, p)))
        .collect()
}

/// Resolve a budget's balance accounts to id and current name, refusing an
/// account that does not exist or cannot hold the budget's currency.
fn resolve_refs(refs: Vec<AccountRef>, currency: &str, accounts: &[Account]) -> Result<Vec<AccountRef>> {
    let by_id: HashMap<&str, &Account> = accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> = accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    refs.into_iter()
        .map(|r| {
            let account = resolve_account(r.account_id.as_deref(), r.account_name.as_deref(), &by_id, &by_name)?;
            if !account.allows_currency(currency) {
                return Err(Error::Validation(format!(
                    "{} cannot hold {currency}; it only allows {}",
                    account.name,
                    account.currencies.join(", "),
                )));
            }
            Ok(AccountRef { account_id: Some(account.id.clone()), account_name: Some(account.name.clone()) })
        })
        .collect()
}

fn check_budget(budget: &Budget) -> Result<()> {
    for (field, value) in [("name", &budget.name), ("tag", &budget.tag), ("currency", &budget.currency)] {
        if value.trim().is_empty() {
            return Err(Error::Validation(format!("a budget needs a {field}")));
        }
    }
    if budget.monthly_amount <= Decimal::ZERO {
        return Err(Error::Validation(format!(
            "budget {}: monthly_amount {} is not positive",
            budget.name, budget.monthly_amount
        )));
    }
    Ok(())
}

/// Write a new budget. Its name must be unused: it is what people type.
pub fn create_budget(
    root: &Path,
    name: String,
    tag: String,
    currency: String,
    monthly_amount: Decimal,
    balance_accounts: Vec<AccountRef>,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let existing = load_budgets(root)?;
    let name = name.trim().to_owned();
    if existing.iter().any(|(b, _)| b.name == name) {
        return Err(Error::Validation(format!("a budget named {name} already exists")));
    }
    let accounts = load_accounts(root)?;
    let balance_accounts = resolve_refs(balance_accounts, currency.trim(), &accounts)?;
    let taken: HashSet<&str> = existing.iter().map(|(b, _)| b.id.as_str()).collect();
    let id = (0..ID_ATTEMPTS)
        .map(|_| new_random_id())
        .find(|candidate| !taken.contains(candidate.as_str()))
        .ok_or_else(|| Error::Validation(format!("could not mint a free budget id after {ID_ATTEMPTS} attempts")))?;
    let dest = root.join(budget_relative_path(&id));
    refuse_existing(&dest)?;
    let timestamp = now();
    let budget = Budget {
        id: id.clone(),
        name,
        tag: tag.trim().to_owned(),
        currency: currency.trim().to_owned(),
        monthly_amount,
        created_at: timestamp,
        updated_at: timestamp,
        balance_accounts,
    };
    check_budget(&budget)?;
    save_toml(&dest, &budget)?;
    Ok((id, dest))
}

/// What `update_budget` changes; `None` leaves a field as it is.
#[derive(Debug, Default)]
pub struct BudgetChanges {
    pub name: Option<String>,
    pub tag: Option<String>,
    pub currency: Option<String>,
    pub monthly_amount: Option<Decimal>,
    pub balance_accounts: Option<Vec<AccountRef>>,
}

/// Change an existing budget, found by id or name, in place.
pub fn update_budget(root: &Path, budget: &str, changes: BudgetChanges) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let existing = load_budgets(root)?;
    let (mut found, path) = existing
        .iter()
        .find(|(b, _)| b.id == budget)
        .or_else(|| existing.iter().find(|(b, _)| b.name == budget))
        .cloned()
        .ok_or_else(|| Error::Validation(format!("no budget {budget}")))?;
    if let Some(name) = changes.name {
        let name = name.trim().to_owned();
        if existing.iter().any(|(b, _)| b.name == name && b.id != found.id) {
            return Err(Error::Validation(format!("a budget named {name} already exists")));
        }
        found.name = name;
    }
    if let Some(tag) = changes.tag {
        found.tag = tag.trim().to_owned();
    }
    if let Some(currency) = changes.currency {
        found.currency = currency.trim().to_owned();
    }
    if let Some(amount) = changes.monthly_amount {
        found.monthly_amount = amount;
    }
    let refs = changes.balance_accounts.unwrap_or(found.balance_accounts);
    found.balance_accounts = resolve_refs(refs, &found.currency, &load_accounts(root)?)?;
    found.updated_at = now();
    check_budget(&found)?;
    save_toml(&path, &found)?;
    Ok((found.id, path))
}
```

`validate.rs`, before `issues` is returned:

```rust
        let mut seen_budget_ids: HashSet<&str> = HashSet::new();
        let mut seen_budget_names: HashSet<&str> = HashSet::new();
        for budget in &self.budgets {
            let issue = |message: String, account: Option<String>| ValidationIssue {
                message,
                transaction_id: None,
                assertion_id: None,
                account,
            };
            if !seen_budget_ids.insert(budget.id.as_str()) {
                issues.push(issue(format!("duplicate budget id {}", budget.id), None));
            }
            if !seen_budget_names.insert(budget.name.as_str()) {
                issues.push(issue(format!("duplicate budget name {}", budget.name), None));
            }
            if budget.monthly_amount <= rust_decimal::Decimal::ZERO {
                issues.push(issue(
                    format!("budget {}: monthly_amount {} is not positive", budget.name, budget.monthly_amount),
                    None,
                ));
            }
            for r in &budget.balance_accounts {
                let (id, name) = (r.account_id.as_deref(), r.account_name.as_deref());
                if let Err(err) = resolve_account(id, name, &by_id, &by_name) {
                    issues.push(issue(
                        format!("budget {}: {} ({})", budget.name, render(&err), describe_ref(id, name)),
                        Some(describe_ref(id, name)),
                    ));
                }
            }
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p sapphire-ledger-core`
Expected: PASS. (If `Assets:Gone` is not in the message, it is in `describe_ref`; the format above includes it.)

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): budget records, their writes and their validation"
```

---

### Task 4: `budget_report` (core)

**Files:**
- Create: `crates/sapphire-ledger-core/src/report.rs`
- Modify: `crates/sapphire-ledger-core/src/lib.rs` (`pub mod report;`)
- Test: `crates/sapphire-ledger-core/tests/report.rs` (new)

**Interfaces:**
- Consumes: `Workspace.budgets`, `Budget`, `resolve_account`, `Posting::balance_contribution`.
- Produces:
  - `pub fn parse_month(raw: &str) -> Result<(i32, u32)>` — `"YYYY-MM"`.
  - `pub fn round_amount(d: Decimal) -> Decimal` — 2 dp, midpoint away from zero, normalized.
  - `pub struct BudgetDay { pub date: NaiveDate, pub spent: Decimal, pub spent_cumulative: Decimal, pub budget_cumulative: Decimal, pub planned_balance: Decimal, pub elapsed: bool, pub actual_balance: Option<Decimal> }`
  - `pub struct BudgetReport { pub daily_budget: Decimal, pub opening_balance: Decimal, pub days: Vec<BudgetDay> }`
  - `pub fn budget_report(ws: &Workspace, budget: &Budget, year: i32, month: u32, today: NaiveDate) -> Result<BudgetReport>`

- [ ] **Step 1: Write the failing tests** — `crates/sapphire-ledger-core/tests/report.rs`:

```rust
use rust_decimal::Decimal;
use sapphire_ledger_core::report::{budget_report, parse_month, round_amount};
use sapphire_ledger_core::{AccountRef, AccountType, Posting, Price, Workspace, load_workspace, ops};

fn d(s: &str) -> chrono::NaiveDate {
    s.parse().unwrap()
}
fn dec(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn posting(account: &str, amount: &str, currency: &str) -> Posting {
    Posting {
        account_id: None,
        account_name: Some(account.into()),
        amount: dec(amount),
        currency: currency.into(),
        price: None,
        memo: None,
    }
}

struct Books(tempfile::TempDir);

impl Books {
    fn new(balance_accounts: &[&str]) -> Books {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sapphire_ledger_core::init_workspace(root, "JPY").unwrap();
        for (name, ty) in [
            ("Assets:Bank", AccountType::Asset),
            ("Assets:Wallet", AccountType::Asset),
            ("Expenses:Food", AccountType::Expense),
            ("Income:Salary", AccountType::Income),
        ] {
            ops::create_account(root, name.into(), ty, vec![], d("2026-01-01"), None, vec![]).unwrap();
        }
        let refs = balance_accounts
            .iter()
            .map(|n| AccountRef { account_id: None, account_name: Some((*n).into()) })
            .collect();
        ops::create_budget(root, "生活費".into(), "living".into(), "JPY".into(), dec("100000"), refs).unwrap();
        Books(dir)
    }

    fn tx(&self, date: &str, tags: &[&str], postings: Vec<Posting>) {
        ops::create_transaction(
            self.0.path(),
            d(date),
            "t".into(),
            None,
            tags.iter().map(|t| t.to_string()).collect(),
            None,
            postings,
            None,
            false,
        )
        .unwrap();
    }

    fn spend(&self, date: &str, tags: &[&str], amount: &str) {
        let negative = format!("-{amount}").replace("--", "");
        self.tx(date, tags, vec![posting("Expenses:Food", amount, "JPY"), posting("Assets:Bank", &negative, "JPY")]);
    }

    fn load(&self) -> Workspace {
        load_workspace(self.0.path()).unwrap()
    }
}

#[test]
fn parse_month_reads_year_and_month() {
    assert_eq!(parse_month("2026-10").unwrap(), (2026, 10));
    assert!(parse_month("2026-13").is_err());
    assert!(parse_month("2026/10").is_err());
}

#[test]
fn round_amount_keeps_two_places_and_drops_trailing_zeros() {
    assert_eq!(round_amount(dec("100000") / dec("31")).to_string(), "3225.81");
    assert_eq!(round_amount(dec("100000.00")).to_string(), "100000");
    assert_eq!(round_amount(dec("0.005")).to_string(), "0.01");
}

#[test]
fn a_31_day_month_spreads_the_budget_and_counts_tagged_spending() {
    let books = Books::new(&["Assets:Bank"]);
    books.tx("2026-04-25", &[], vec![posting("Assets:Bank", "300000", "JPY"), posting("Income:Salary", "-300000", "JPY")]);
    books.spend("2026-05-02", &["living"], "1200");
    books.spend("2026-05-02", &["living"], "800");
    books.spend("2026-05-03", &[], "5000"); // untagged: not the budget's, but still leaves the bank
    books.spend("2026-05-04", &["living"], "-300"); // a refund
    let ws = books.load();
    let r = budget_report(&ws, &ws.budgets[0], 2026, 5, d("2026-05-31")).unwrap();

    assert_eq!(r.days.len(), 31);
    assert_eq!(round_amount(r.daily_budget).to_string(), "3225.81");
    assert_eq!(r.opening_balance, dec("300000"));
    let day = |n: usize| &r.days[n - 1];
    assert_eq!(day(2).spent, dec("2000"));
    assert_eq!(day(3).spent, dec("0"));
    assert_eq!(day(4).spent, dec("-300"));
    assert_eq!(day(4).spent_cumulative, dec("1700"));
    assert_eq!(day(31).budget_cumulative, dec("100000"));
    assert_eq!(day(31).planned_balance, dec("200000"));
    assert_eq!(day(4).actual_balance, Some(dec("293300")));
    assert!(r.days.iter().all(|d| d.elapsed));
}

#[test]
fn february_has_28_days_and_ends_on_the_monthly_amount() {
    let books = Books::new(&[]);
    let ws = books.load();
    let r = budget_report(&ws, &ws.budgets[0], 2026, 2, d("2026-10-16")).unwrap();
    assert_eq!(r.days.len(), 28);
    assert_eq!(r.days[27].budget_cumulative, dec("100000"));
    assert_eq!(r.days[0].date, d("2026-02-01"));
}

#[test]
fn days_after_today_are_not_elapsed_and_have_no_actual_balance() {
    let books = Books::new(&["Assets:Bank"]);
    let ws = books.load();
    let r = budget_report(&ws, &ws.budgets[0], 2026, 10, d("2026-10-16")).unwrap();
    assert!(r.days[15].elapsed);
    assert_eq!(r.days[15].actual_balance, Some(dec("0")));
    assert!(!r.days[16].elapsed);
    assert_eq!(r.days[16].actual_balance, None);
}

#[test]
fn without_balance_accounts_there_is_no_actual_balance() {
    let books = Books::new(&[]);
    let ws = books.load();
    let r = budget_report(&ws, &ws.budgets[0], 2026, 5, d("2026-10-16")).unwrap();
    assert!(r.days.iter().all(|d| d.actual_balance.is_none()));
}

#[test]
fn spending_counts_by_its_contribution_in_the_budget_currency_and_balances_by_raw_amount() {
    let books = Books::new(&["Assets:Bank", "Assets:Wallet"]);
    // 10 USD of food, priced at 150 JPY: 1500 JPY of spending. Paid from a USD wallet: no JPY balance change.
    books.tx(
        "2026-05-10",
        &["living"],
        vec![
            Posting { price: Some(Price { value: dec("150"), currency: "JPY".into() }), ..posting("Expenses:Food", "10", "USD") },
            Posting { price: Some(Price { value: dec("150"), currency: "JPY".into() }), ..posting("Assets:Wallet", "-10", "USD") },
        ],
    );
    let ws = books.load();
    let r = budget_report(&ws, &ws.budgets[0], 2026, 5, d("2026-10-16")).unwrap();
    assert_eq!(r.days[9].spent, dec("1500"));
    assert_eq!(r.days[9].actual_balance, Some(dec("0")));
}

#[test]
fn an_unresolvable_balance_account_is_skipped() {
    let books = Books::new(&["Assets:Bank"]);
    let mut ws = books.load();
    ws.budgets[0].balance_accounts.push(AccountRef { account_id: Some("zzzzzzz".into()), account_name: None });
    let r = budget_report(&ws, &ws.budgets[0], 2026, 5, d("2026-10-16")).unwrap();
    assert_eq!(r.days[0].actual_balance, Some(dec("0")));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-core --test report`
Expected: compile error — `report` module not found.

- [ ] **Step 3: Implement** — `crates/sapphire-ledger-core/src/report.rs`:

```rust
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
    let (year, month) = (y.parse::<i32>().map_err(|_| bad())?, m.parse::<u32>().map_err(|_| bad())?);
    NaiveDate::from_ymd_opt(year, month, 1).ok_or_else(bad)?;
    Ok((year, month))
}

/// Two decimal places, midpoint away from zero, trailing zeros dropped: how
/// a report's amounts are answered.
pub fn round_amount(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero).normalize()
}

pub fn budget_report(ws: &Workspace, budget: &Budget, year: i32, month: u32, today: NaiveDate) -> Result<BudgetReport> {
    let first = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| Error::Validation(format!("no such month {year}-{month:02}")))?;
    let next = first + Months::new(1);
    let n = (next - first).num_days() as usize;

    let by_id: HashMap<&str, &Account> = ws.accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> = ws.accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    let balance_ids: HashSet<&str> = budget
        .balance_accounts
        .iter()
        .filter_map(|r| resolve_account(r.account_id.as_deref(), r.account_name.as_deref(), &by_id, &by_name).ok())
        .map(|a| a.id.as_str())
        .collect();

    let mut spent = vec![Decimal::ZERO; n];
    let mut balance_change = vec![Decimal::ZERO; n];
    let mut opening = Decimal::ZERO;
    for tx in &ws.transactions {
        if tx.date >= next {
            continue;
        }
        let tagged = tx.tags.iter().any(|t| *t == budget.tag);
        for p in &tx.postings {
            let Ok(account) = resolve_account(p.account_id.as_deref(), p.account_name.as_deref(), &by_id, &by_name) else {
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
    Ok(BudgetReport { daily_budget: budget.monthly_amount / total, opening_balance: opening, days })
}
```

Note: with `balance_accounts` empty, `planned_balance` is `0 − budget_cumulative`; clients do not draw it (Task 10).

- [ ] **Step 4: Run tests**

Run: `cargo test -p sapphire-ledger-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): a budget's month as a day-by-day report"
```

---

### Task 5: `sapphire-ledger-api` 2.0.0

**Files:**
- Modify: `crates/sapphire-ledger-api/{Cargo.toml,src/lib.rs,src/params.rs,src/records.rs}`
- Modify version requirements `"1.0.0"` → `"2.0.0"`: `crates/sapphire-ledger-mcp/Cargo.toml`, `cli/Cargo.toml`, `desktop/Cargo.toml`
- Modify: `crates/sapphire-ledger-mcp/src/wire.rs` (map `default_tags`), `crates/sapphire-ledger-mcp/src/methods.rs` (`add_account` passes `p.default_tags`; `add_transaction` passes `p.apply_default_tags`)
- Modify literals: add `default_tags: vec![]` to every `AddAccountParams {}` and `apply_default_tags: true` to every `AddTransactionParams {}` in `crates/sapphire-ledger-mcp/src/methods.rs` tests, `cli/tests/mcp_tools.rs`, `desktop/src/client.rs` test; `default_tags: vec![]` to `Account {}` in `desktop/src/ledger.rs` tests. `desktop/src/form.rs`: `AccountForm::to_params` sets `default_tags: vec![]` and `TxForm::to_params` sets `apply_default_tags: true` **for now** (Task 9 changes both).
- Test: `crates/sapphire-ledger-api/src/params.rs` (new `#[cfg(test)] mod tests`), `crates/sapphire-ledger-api/src/lib.rs`

**Interfaces:**
- Produces (all `Debug, Clone, Serialize, Deserialize`; params also `schemars::JsonSchema`):

```rust
// records.rs
pub struct Account { /* existing */ #[serde(default, skip_serializing_if = "Vec::is_empty")] pub default_tags: Vec<String>, /* before description */ }
pub struct AccountRef { pub account_id: Option<String>, pub account_name: Option<String> } // both skip_serializing_if none, default
pub struct Budget { pub id: String, pub name: String, pub tag: String, pub currency: String, pub monthly_amount: String, #[serde(default)] pub balance_accounts: Vec<AccountRef> }
pub struct BudgetDay { pub date: NaiveDate, pub spent: String, pub spent_cumulative: String, pub budget_cumulative: String, pub planned_balance: String, pub elapsed: bool, #[serde(default)] pub actual_balance: Option<String> }
pub struct BudgetReport { pub budget: Budget, pub month: String, pub daily_budget: String, pub opening_balance: String, pub days: Vec<BudgetDay> }

// params.rs
AddAccountParams { ..., #[serde(default)] pub default_tags: Vec<String> }
AddTransactionParams { ..., #[serde(default = "applies")] pub apply_default_tags: bool }
pub struct UpdateAccountParams { pub account: String, pub default_tags: Option<Vec<String>> }
pub struct AddBudgetParams { pub name: String, pub tag: String, pub currency: String, pub monthly_amount: String, #[serde(default)] pub balance_accounts: Vec<String> }
pub struct UpdateBudgetParams { pub budget: String, pub name: Option<String>, pub tag: Option<String>, pub currency: Option<String>, pub monthly_amount: Option<String>, pub balance_accounts: Option<Vec<String>> }
pub struct BudgetReportParams { pub budget: String, pub month: String }

// lib.rs
pub const UPDATE_ACCOUNT: &str = "sapphire-ledger.update_account";
pub const LIST_BUDGETS: &str = "sapphire-ledger.list_budgets";
pub const ADD_BUDGET: &str = "sapphire-ledger.add_budget";
pub const UPDATE_BUDGET: &str = "sapphire-ledger.update_budget";
pub const BUDGET_REPORT: &str = "sapphire-ledger.budget_report";
```

- [ ] **Step 1: Write the failing tests**

In `crates/sapphire-ledger-api/src/lib.rs` change the existing test to `assert_eq!(API_VERSION, 2, ...)`. Append to `params.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// An agent that has never heard of default tags gets them applied; an
    /// older caller's JSON still parses.
    #[test]
    fn default_tags_apply_unless_a_caller_says_otherwise() {
        let p: AddTransactionParams =
            serde_json::from_str(r#"{"date":"2026-05-21","narration":"n","postings":[]}"#).unwrap();
        assert!(p.apply_default_tags);
        assert!(p.tags.is_empty());
        let p: AddTransactionParams = serde_json::from_str(
            r#"{"date":"2026-05-21","narration":"n","postings":[],"apply_default_tags":false}"#,
        )
        .unwrap();
        assert!(!p.apply_default_tags);
        let a: AddAccountParams =
            serde_json::from_str(r#"{"name":"A","type":"Asset","opened_at":"2026-01-01"}"#).unwrap();
        assert!(a.default_tags.is_empty());
    }
}
```

In `crates/sapphire-ledger-mcp/src/wire.rs` test, set `full`'s `default_tags: vec!["living".into()]` (the `bare` one keeps `vec![]`).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-api`
Expected: compile error (`apply_default_tags` unknown) / version assertion fails.

- [ ] **Step 3: Implement** the types above, with doc comments in the style of the existing ones:

```rust
/// `add_transaction` applies default tags unless told otherwise.
fn applies() -> bool {
    true
}
```

On `AddTransactionParams.apply_default_tags`: `/// Add the posting accounts' default tags to `tags`. On unless set to false: a client that shows the tags before sending (the desktop) sends false and the tags as shown.`
On `UpdateAccountParams.default_tags`: `/// The account's new default tags, replacing the old. Leave out to keep them.`
On `UpdateBudgetParams`: `/// The budget's id or name, and the fields to change; a field left out stays as it is.`
On `BudgetReportParams.month`: `/// `YYYY-MM`.`
`Cargo.toml`: `version = "2.0.0"`. Re-export the new names from `lib.rs` (`pub use params::{..., UpdateAccountParams, AddBudgetParams, UpdateBudgetParams, BudgetReportParams}; pub use records::{..., AccountRef, Budget, BudgetDay, BudgetReport};`). Add the five constants with doc comments (`/// Every budget, as JSON: a list of [`Budget`].`, `/// One budget's month day by day, as JSON: a [`BudgetReport`].`, the writes `/// …; answers with a confirmation to show as it is.`).

`wire.rs`: `default_tags: a.default_tags.clone(),` in `account`.
`methods.rs`: `add_account` passes `p.default_tags` as `create_account`'s last argument; `add_transaction` passes `p.apply_default_tags` instead of `true`.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(api)!: default tags and budgets in the ledger's API, version 2

BREAKING CHANGE: Account and the add parameters gain fields; API_VERSION is 2."
```

---

### Task 6: The new server methods (mcp `methods.rs`, `wire.rs`)

**Files:**
- Modify: `crates/sapphire-ledger-mcp/src/methods.rs`, `crates/sapphire-ledger-mcp/src/wire.rs`
- Test: `crates/sapphire-ledger-mcp/src/methods.rs` tests module

**Interfaces:**
- Consumes: `ops::{update_account, create_budget, update_budget, BudgetChanges}`, `report::{budget_report, parse_month, round_amount}`, API types from Task 5.
- Produces: `LedgerMethods::{update_account, list_budgets, add_budget, update_budget, budget_report}` registered under the API constants; `wire::budget(&core::Budget) -> api::Budget`; `wire::budget_report(&core::Budget, year, month, &core::report::BudgetReport) -> api::BudgetReport`.

- [ ] **Step 1: Write the failing tests** (append inside `mod tests` in `methods.rs`; extend the `use super::*;` imports as needed):

```rust
    fn add_budget(server: &LedgerMethods) -> String {
        server
            .add_budget(api::AddBudgetParams {
                name: "生活費".into(),
                tag: "living".into(),
                currency: "JPY".into(),
                monthly_amount: "100000".into(),
                balance_accounts: vec!["Assets:Cash:JPY".into()],
            })
            .expect("add_budget")
    }

    fn spend(server: &LedgerMethods, date: &str, tags: Vec<String>, amount: &str, apply: bool) {
        server
            .add_transaction(AddTransactionParams {
                date: date.into(),
                narration: "spend".into(),
                payee: None,
                tags,
                status: None,
                postings: vec![
                    posting_param("Expenses:Food", amount),
                    posting_param("Assets:Cash:JPY", &format!("-{amount}")),
                ],
                comment: None,
                apply_default_tags: apply,
            })
            .expect("add_transaction");
    }

    #[test]
    fn a_budget_is_listed_and_reported_by_day() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");
        assert!(add_budget(&server).contains("created budget"));
        spend(&server, "2026-05-21", vec!["living".into()], "1200", true);

        let budgets: Vec<api::Budget> =
            serde_json::from_str(&server.list_budgets(EmptyParams {}).unwrap()).unwrap();
        assert_eq!(budgets[0].name, "生活費");
        assert_eq!(budgets[0].balance_accounts[0].account_name.as_deref(), Some("Assets:Cash:JPY"));

        let report: api::BudgetReport = serde_json::from_str(
            &server
                .budget_report(api::BudgetReportParams { budget: "生活費".into(), month: "2026-05".into() })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(report.month, "2026-05");
        assert_eq!(report.daily_budget, "3225.81");
        assert_eq!(report.days.len(), 31);
        assert_eq!(report.days[20].spent_cumulative, "1200");
        assert_eq!(report.days[30].budget_cumulative, "100000");
        assert_eq!(report.days[20].actual_balance.as_deref(), Some("-1200"));
    }

    #[test]
    fn update_budget_changes_the_amount_and_a_bad_month_is_refused() {
        let (_dir, server) = test_server();
        add_account(&server, "Assets:Cash:JPY", "Asset");
        add_budget(&server);
        let msg = server
            .update_budget(api::UpdateBudgetParams {
                budget: "生活費".into(),
                name: None,
                tag: None,
                currency: None,
                monthly_amount: Some("120000".into()),
                balance_accounts: None,
            })
            .unwrap();
        assert!(msg.contains("updated budget"), "got: {msg}");
        let budgets: Vec<api::Budget> = serde_json::from_str(&server.list_budgets(EmptyParams {}).unwrap()).unwrap();
        assert_eq!(budgets[0].monthly_amount, "120000");

        let err = server
            .budget_report(api::BudgetReportParams { budget: "生活費".into(), month: "May".into() })
            .unwrap_err();
        assert!(err.contains("YYYY-MM"), "got: {err}");
        let err = server
            .budget_report(api::BudgetReportParams { budget: "nope".into(), month: "2026-05".into() })
            .unwrap_err();
        assert!(err.contains("no budget nope"), "got: {err}");
    }

    #[test]
    fn default_tags_set_by_update_account_reach_new_transactions() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");
        let msg = server
            .update_account(api::UpdateAccountParams {
                account: "Expenses:Food".into(),
                default_tags: Some(vec!["living".into()]),
            })
            .unwrap();
        assert!(msg.contains("updated account"), "got: {msg}");
        let accounts: Vec<api::Account> = serde_json::from_str(&server.list_accounts(EmptyParams {}).unwrap()).unwrap();
        let food = accounts.iter().find(|a| a.name == "Expenses:Food").unwrap();
        assert_eq!(food.default_tags, ["living"]);

        spend(&server, "2026-05-21", vec![], "100", true);
        spend(&server, "2026-05-22", vec![], "100", false);
        let state = server.shared_state();
        let guard = state.lock().unwrap();
        let mut tags: Vec<Vec<String>> = guard.workspace().transactions.iter().map(|t| t.tags.clone()).collect();
        tags.sort();
        assert_eq!(tags, vec![vec![], vec!["living".to_string()]]);
    }

    #[test]
    fn update_account_with_nothing_to_change_is_refused() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        let err = server
            .update_account(api::UpdateAccountParams { account: "Expenses:Food".into(), default_tags: None })
            .unwrap_err();
        assert!(err.contains("nothing to change"), "got: {err}");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-mcp`
Expected: compile error — methods not found.

- [ ] **Step 3: Implement**

`wire.rs`:

```rust
pub fn budget(b: &core::Budget) -> api::Budget {
    api::Budget {
        id: b.id.clone(),
        name: b.name.clone(),
        tag: b.tag.clone(),
        currency: b.currency.clone(),
        monthly_amount: b.monthly_amount.to_string(),
        balance_accounts: b
            .balance_accounts
            .iter()
            .map(|r| api::AccountRef { account_id: r.account_id.clone(), account_name: r.account_name.clone() })
            .collect(),
    }
}

pub fn budget_report(b: &core::Budget, year: i32, month: u32, r: &core::report::BudgetReport) -> api::BudgetReport {
    let amount = |d: rust_decimal::Decimal| core::report::round_amount(d).to_string();
    api::BudgetReport {
        budget: budget(b),
        month: format!("{year:04}-{month:02}"),
        daily_budget: amount(r.daily_budget),
        opening_balance: amount(r.opening_balance),
        days: r
            .days
            .iter()
            .map(|d| api::BudgetDay {
                date: d.date,
                spent: amount(d.spent),
                spent_cumulative: amount(d.spent_cumulative),
                budget_cumulative: amount(d.budget_cumulative),
                planned_balance: amount(d.planned_balance),
                elapsed: d.elapsed,
                actual_balance: d.actual_balance.map(amount),
            })
            .collect(),
    }
}
```

`methods.rs`:

1. Give `write_result` a leading `verb: &str` parameter (`format!("{verb} {kind} {id}: {}", ...)`) and pass `"created"` at the four existing call sites.
2. Register the five methods in `register` (same `route(...)` pattern, before the final `route(... ADD_PRICE ...)`; keep `ADD_PRICE` last returning the router).
3. Add a helper and the methods:

```rust
fn find_budget<'a>(budgets: &'a [sapphire_ledger_core::Budget], needle: &str) -> anyhow::Result<&'a sapphire_ledger_core::Budget> {
    budgets
        .iter()
        .find(|b| b.id == needle)
        .or_else(|| budgets.iter().find(|b| b.name == needle))
        .with_context(|| format!("no budget {needle}"))
}

fn account_refs(needles: Vec<String>, accounts: &[sapphire_ledger_core::Account]) -> Vec<sapphire_ledger_core::AccountRef> {
    needles
        .iter()
        .map(|n| {
            let (account_id, account_name) = split_account_ref(n, accounts);
            sapphire_ledger_core::AccountRef { account_id, account_name }
        })
        .collect()
}
```

```rust
    fn update_account(&self, p: UpdateAccountParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let tags = p.default_tags.context("nothing to change: give default_tags")?;
            let mut guard = self.lock_state();
            let (id, dest) = ops::update_account(guard.root(), &p.account, tags)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("updated", "account", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn list_budgets(&self, _: EmptyParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let guard = self.lock_state();
            let budgets: Vec<api::Budget> = guard.workspace().budgets.iter().map(wire::budget).collect();
            Ok(serde_json::to_string_pretty(&budgets)?)
        })()
        .map_err(render_error)
    }

    fn add_budget(&self, p: AddBudgetParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let amount = parse_decimal(&p.monthly_amount)?;
            let mut guard = self.lock_state();
            // See add_transaction: classify references against fresh accounts.
            guard.reload()?;
            let refs = account_refs(p.balance_accounts, &guard.workspace().accounts);
            let (id, dest) = ops::create_budget(guard.root(), p.name, p.tag, p.currency, amount, refs)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("created", "budget", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn update_budget(&self, p: UpdateBudgetParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let monthly_amount = p.monthly_amount.as_deref().map(parse_decimal).transpose()?;
            let mut guard = self.lock_state();
            guard.reload()?;
            let balance_accounts = p.balance_accounts.map(|n| account_refs(n, &guard.workspace().accounts));
            let changes = ops::BudgetChanges { name: p.name, tag: p.tag, currency: p.currency, monthly_amount, balance_accounts };
            let (id, dest) = ops::update_budget(guard.root(), &p.budget, changes)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("updated", "budget", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn budget_report(&self, p: BudgetReportParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let (year, month) = sapphire_ledger_core::report::parse_month(&p.month)?;
            let guard = self.lock_state();
            let ws = guard.workspace();
            let budget = find_budget(&ws.budgets, &p.budget)?;
            let today = chrono::Local::now().date_naive();
            let report = sapphire_ledger_core::report::budget_report(ws, budget, year, month, today)?;
            Ok(serde_json::to_string_pretty(&wire::budget_report(budget, year, month, &report))?)
        })()
        .map_err(render_error)
    }
```

(Import `AddBudgetParams, BudgetReportParams, UpdateAccountParams, UpdateBudgetParams` from `sapphire_ledger_api`.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p sapphire-ledger-mcp`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(mcp): budget and account-update methods on the app server"
```

---

### Task 7: MCP tools for budgets and account updates

**Files:**
- Modify: `crates/sapphire-ledger-mcp/src/tools.rs`
- Modify: `cli/tests/mcp_tools.rs`
- Modify: `docs/design.md` (Tools table and count)

**Interfaces:**
- Consumes: the five API constants and parameter types.
- Produces: tools `list_budgets`, `budget_report` (read-only), `add_budget`, `update_budget`, `update_account`.

- [ ] **Step 1: Write the failing tests**

In `tools.rs` tests, change the count to `assert_eq!(names.len(), 14 + 7, "{names:?}");`, update its doc comment to "The ledger's fourteen", and add `"list_budgets", "budget_report", "add_budget", "update_budget", "update_account"` to the `expected` list.

In `cli/tests/mcp_tools.rs`, add `AddBudgetParams, BudgetReportParams` to the `sapphire_ledger_api` import and insert before the `// A refused write comes back as the ledger's reason.` comment:

```rust
    tools
        .add_budget(Parameters(AddBudgetParams {
            name: "生活費".into(),
            tag: "living".into(),
            currency: "JPY".into(),
            monthly_amount: "100000".into(),
            balance_accounts: vec!["Assets:Cash".into()],
        }))
        .await
        .expect("add_budget");
    let report = tools
        .budget_report(Parameters(BudgetReportParams { budget: "生活費".into(), month: "2026-05".into() }))
        .await
        .expect("budget_report");
    assert!(report.contains("\"daily_budget\": \"3225.81\""), "got: {report}");
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-mcp tools && cargo test -p sapphire-ledger --test mcp_tools`
Expected: count assertion fails / `add_budget` not a method of `LedgerTools`.

- [ ] **Step 3: Implement** in the `#[tool_router] impl LedgerTools` block:

```rust
    #[tool(
        description = "List every budget: its id, name, the tag that selects its \
        transactions, currency, monthly amount and the accounts it is paid from.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_budgets(&self, Parameters(p): Parameters<EmptyParams>) -> Result<String, String> {
        self.call(api::LIST_BUDGETS, p).await
    }

    #[tool(
        description = "One budget's month, day by day: spending of the transactions \
        carrying the budget's tag (to Expense accounts, in its currency), cumulated, \
        against the monthly amount spread evenly over the month; and the paid-from \
        accounts' balance against the month-start balance minus that daily budget. \
        `budget` is its id or name, `month` is YYYY-MM. Days after today are marked \
        `elapsed: false`.",
        annotations(read_only_hint = true)
    )]
    pub async fn budget_report(&self, Parameters(p): Parameters<BudgetReportParams>) -> Result<String, String> {
        self.call(api::BUDGET_REPORT, p).await
    }

    #[tool(
        description = "Create a monthly budget. Transactions carrying `tag` count \
        against it; `balance_accounts` (ids or names) are what it is paid from. \
        Fails if a budget with that name exists."
    )]
    pub async fn add_budget(&self, Parameters(p): Parameters<AddBudgetParams>) -> Result<String, String> {
        self.call(api::ADD_BUDGET, p).await
    }

    #[tool(
        description = "Change a budget, by id or name. Only the fields given change; \
        a new amount also redraws past months."
    )]
    pub async fn update_budget(&self, Parameters(p): Parameters<UpdateBudgetParams>) -> Result<String, String> {
        self.call(api::UPDATE_BUDGET, p).await
    }

    #[tool(
        description = "Change an account's default tags, by id or name. Default tags \
        are copied into each transaction recorded afterwards that posts to the \
        account; past transactions keep the tags they have."
    )]
    pub async fn update_account(&self, Parameters(p): Parameters<UpdateAccountParams>) -> Result<String, String> {
        self.call(api::UPDATE_ACCOUNT, p).await
    }
```

Change `add_transaction`'s description to: `"Record a transaction. Postings must sum to zero per currency, and every account named must already exist. The posting accounts' default tags are added to `tags` unless `apply_default_tags` is false. Nothing is written if validation fails."` and `add_account`'s to add: `" `default_tags` are copied into the transactions later recorded against it."`. In `get_info`, append to the instructions sentence: `"list_budgets and budget_report show how a month is going against a budget."`

In `docs/design.md`'s MCP Tools section: "Nine tools of the ledger's own … five read, four write" → "Fourteen tools of the ledger's own … seven read, seven write", and add rows:

```markdown
| `list_budgets` | List every budget: name, tag, currency, monthly amount and the accounts it is paid from. |
| `budget_report` | One budget's month day by day: tagged spending against the even daily budget, and the paid-from balance against plan. |
| `update_account` | Change an account's default tags; later transactions copy them. |
| `add_budget` | Create a monthly budget selected by a tag. |
| `update_budget` | Change a budget's fields; only those given change. |
```

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(mcp): budget tools and update_account for agents"
```

---

### Task 8: CLI `budget report`

**Files:**
- Modify: `cli/src/cli.rs` (`Command::Budget`, `BudgetCommand`), `cli/src/commands.rs` (`budget`), `cli/src/main.rs` (dispatch)
- Test: `cli/tests/write_commands.rs`

**Interfaces:**
- Consumes: `load_workspace`, `report::{budget_report, parse_month, round_amount}`.
- Produces: `sapphire-ledger budget report <BUDGET> [--month YYYY-MM]`.

- [ ] **Step 1: Write the failing test** (append to `cli/tests/write_commands.rs`):

```rust
#[test]
fn budget_report_prints_a_hand_written_budgets_month() {
    let dir = seeded();
    let root = dir.path();
    // No CLI `budget add` yet: a budget written by hand, naming its account by name only.
    std::fs::create_dir_all(root.join("budgets")).unwrap();
    std::fs::write(
        root.join("budgets/bud0001.toml"),
        "id = \"bud0001\"\nname = \"living\"\ntag = \"grocery\"\ncurrency = \"JPY\"\n\
         monthly_amount = \"31000\"\ncreated_at = \"2026-05-01T00:00:00+09:00\"\n\
         updated_at = \"2026-05-01T00:00:00+09:00\"\n\n[[balance_accounts]]\naccount_name = \"Assets:Cash\"\n",
    )
    .unwrap();
    ok(root, &["tx", "add", "--date", "2026-05-22", "--narration", "veg", "--tag", "grocery",
        "--posting", "Expenses:Food 500 JPY", "--posting", "Assets:Cash -500 JPY"]);

    let out = ok(root, &["budget", "report", "living", "--month", "2026-05"]);
    assert!(out.contains("1000 / day"), "got: {out}");
    let line = out.lines().find(|l| l.starts_with("2026-05-22")).expect("a row per day");
    let cols: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(cols, ["2026-05-22", "500", "500", "22000", "-22000", "-1700"], "got: {line}");
    assert_eq!(out.lines().filter(|l| l.starts_with("2026-05-")).count(), 31);

    let err = run(root, &["budget", "report", "nope"]);
    assert!(!err.status.success());
    assert!(String::from_utf8_lossy(&err.stderr).contains("no budget nope"));
}
```

(`seeded()` spends 1200 from `Assets:Cash` on 2026-05-21 without the tag, so the actual balance on the 22nd is −1700 and `opening` is 0.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger --test write_commands budget_report`
Expected: FAIL — unrecognized subcommand `budget`.

- [ ] **Step 3: Implement**

`cli.rs`, in `enum Command` after `Price`:

```rust
    /// Read budgets.
    #[command(subcommand)]
    Budget(BudgetCommand),
```

and

```rust
#[derive(Subcommand)]
pub enum BudgetCommand {
    /// One budget's month, day by day: spent, cumulative spent, cumulative
    /// budget, planned and actual balance.
    Report {
        /// Budget name or id.
        budget: String,
        /// `YYYY-MM`. Defaults to this month.
        #[arg(long, value_name = "MONTH")]
        month: Option<String>,
    },
}
```

`main.rs`: `Command::Budget(c) => commands::budget(c, &resolve_root(cli.ledger_dir)?),`

`commands.rs` (import `BudgetCommand`, `sapphire_ledger_core::report`):

```rust
pub fn budget(command: BudgetCommand, root: &Path) -> anyhow::Result<()> {
    let BudgetCommand::Report { budget, month } = command;
    let (year, month) = match month.as_deref() {
        Some(raw) => report::parse_month(raw)?,
        None => {
            let t = today();
            (chrono::Datelike::year(&t), chrono::Datelike::month(&t))
        }
    };
    let workspace = sapphire_ledger_core::load_workspace(root)?;
    let found = workspace
        .budgets
        .iter()
        .find(|b| b.id == budget)
        .or_else(|| workspace.budgets.iter().find(|b| b.name == budget))
        .with_context(|| format!("no budget {budget}"))?;
    let r = report::budget_report(&workspace, found, year, month, today())?;
    let a = |d: rust_decimal::Decimal| report::round_amount(d).to_string();
    println!(
        "{} {year:04}-{month:02}: {} {} / month, {} / day, opening balance {}",
        found.name, a(found.monthly_amount), found.currency, a(r.daily_budget), a(r.opening_balance),
    );
    println!("{:<10}  {:>12}  {:>12}  {:>12}  {:>12}  {:>12}", "date", "spent", "spent total", "budget total", "planned", "actual");
    for d in &r.days {
        let actual = d.actual_balance.map(a).unwrap_or_else(|| "-".into());
        println!(
            "{}  {:>12}  {:>12}  {:>12}  {:>12}  {:>12}",
            d.date, a(d.spent), a(d.spent_cumulative), a(d.budget_cumulative), a(d.planned_balance), actual,
        );
    }
    Ok(())
}
```

(`today()` already exists in `commands.rs` and returns `NaiveDate`; check and reuse it. Add `rust_decimal` to `cli/Cargo.toml` `[dependencies]` as `rust_decimal.workspace = true` if it is not there.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p sapphire-ledger`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(cli): budget report"
```

---

### Task 9: Desktop — default tags on the Ledger screen

**Files:**
- Modify: `desktop/src/form.rs` (`split_list`, `TxForm::add_tags`, `AccountForm.default_tags`, `TagsForm`, `TxForm::to_params` sends `apply_default_tags: false`)
- Modify: `desktop/src/ledger.rs` (`LedgerView::new` takes `Arc<LedgerClient>`, account form field, "Edit tags" window, posting account choice merges tags)
- Modify: `desktop/src/main.rs` (wrap the client in `Arc`)
- Test: `desktop/src/form.rs` tests

**Interfaces:**
- Consumes: `api::{Account (default_tags), UpdateAccountParams, UPDATE_ACCOUNT}`.
- Produces:
  - `pub fn split_list(raw: &str) -> Vec<String>` (comma-separated, trimmed, non-empty) in `form.rs`
  - `TxForm::add_tags(&mut self, tags: &[String])`
  - `AccountForm.default_tags: String`
  - `pub struct TagsForm { pub account_id: String, pub account_name: String, pub tags: String }` with `TagsForm::new(&Account)` and `to_params(&self) -> UpdateAccountParams`
  - `LedgerView::new(runtime, client: Arc<LedgerClient>, repaint)`

- [ ] **Step 1: Write the failing tests** (append inside `form.rs` `mod tests`):

```rust
    #[test]
    fn choosing_an_account_adds_its_default_tags_once() {
        let mut form = tx(&[("Expenses:Food", "1", "JPY"), ("Assets:Cash", "-1", "JPY")]);
        form.tags = "weekly".into();
        form.add_tags(&["living".into(), "weekly".into()]);
        assert_eq!(form.tags, "weekly, living");
        form.add_tags(&["living".into()]);
        assert_eq!(form.tags, "weekly, living", "nothing new, nothing rewritten");
    }

    #[test]
    fn the_form_sends_its_tags_as_shown() {
        let mut form = tx(&[("Expenses:Food", "1", "JPY"), ("Assets:Cash", "-1", "JPY")]);
        form.add_tags(&["living".into()]);
        form.tags = form.tags.replace("living", "");
        let p = form.to_params().unwrap();
        assert_eq!(p.tags, ["food", "weekly"]);
        assert!(!p.apply_default_tags, "the server must not add back what was removed");
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-desktop form`
Expected: compile error — `add_tags`, `TagsForm` not found.

- [ ] **Step 3: Implement**

`form.rs`:

```rust
/// A comma-separated field as its items: trimmed, empty ones dropped.
pub fn split_list(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect()
}
```

Use it in `TxForm::to_params` (`tags: split_list(&self.tags)`) and `AccountForm::to_params` (`currencies: split_list(&self.currencies)`, `default_tags: split_list(&self.default_tags)`). In `TxForm::to_params` set `apply_default_tags: false,` with the comment `// The tags field already shows the accounts' defaults; what it shows is what is stored.`

```rust
impl TxForm {
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

/// Editing one account's default tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagsForm {
    pub account_id: String,
    pub account_name: String,
    /// Comma-separated.
    pub tags: String,
}

impl TagsForm {
    pub fn new(account: &sapphire_ledger_api::Account) -> TagsForm {
        TagsForm {
            account_id: account.id.clone(),
            account_name: account.name.clone(),
            tags: account.default_tags.join(", "),
        }
    }

    pub fn to_params(&self) -> UpdateAccountParams {
        UpdateAccountParams { account: self.account_id.clone(), default_tags: Some(split_list(&self.tags)) }
    }
}
```

`AccountForm`: field `/// Comma-separated; copied into transactions posting here. pub default_tags: String,` initialized `String::new()`.

`ledger.rs`:
- `LedgerView::new(runtime, client: Arc<LedgerClient>, repaint)`; field init `client,`.
- Add `tags_form: Option<TagsForm>` (init `None`) and `Sent::Tags`. In `drain_replies`'s `Written(sent, Ok(_))` arm, `Sent::Tags => self.tags_form = None,`.
- `fn send_tags(&mut self)`: like `send_account`, but `let params = form.to_params();` (infallible) and `client.call(api::UPDATE_ACCOUNT, params)`, `Reply::Written(Sent::Tags, result)`.
- In `postings()` after the heading/description, when `account` is `Some(a)`:

```rust
        if let Some(a) = account {
            ui.horizontal(|ui| {
                ui.label(if a.default_tags.is_empty() {
                    "No default tags".to_owned()
                } else {
                    format!("Default tags: {}", a.default_tags.join(", "))
                });
                if ui.small_button("Edit tags").clicked() {
                    edit_tags = Some(TagsForm::new(a));
                }
            });
        }
```

  declaring `let mut edit_tags = None;` at the top of `postings()` and, at its end (after the `books` borrow ends — restructure so `books` is taken via `let Some(books) = &self.books` inside a block, then `if let Some(f) = edit_tags { self.tags_form = Some(f); self.form_error = None; }`).
- `fn tags_window(&mut self, ctx)`: an `egui::Window::new("Default tags")` with a label of `form.account_name`, a `TextEdit::singleline(&mut form.tags).hint_text("living, food")`, a weak label `"Copied into each transaction recorded against this account from now on."`, the `form_error` line and a `Save` button disabled while sending — same open/submit structure as `account_window`. Call it from `ui()` after `account_window`.
- Account window: add a `Default tags` row after `Currencies`: `ui.add(egui::TextEdit::singleline(&mut form.default_tags).hint_text("living"));`.
- Transaction window: replace the `accounts: Vec<String>` with `accounts: Vec<Account>` (cloned, sorted by name). Change `posting_row` to take `accounts: &[Account]` and return `Option<&Account>`, the account newly chosen this frame:

```rust
fn posting_row<'a>(ui: &mut egui::Ui, i: usize, row: &mut PostingRow, accounts: &'a [Account]) -> Option<&'a Account> {
    let mut chosen = None;
    egui::ComboBox::from_id_salt(("posting_account", i))
        .width(220.0)
        .selected_text(if row.account.is_empty() { "Account…" } else { row.account.as_str() })
        .show_ui(ui, |ui| {
            for account in accounts {
                if ui.selectable_value(&mut row.account, account.name.clone(), &account.name).clicked() {
                    chosen = Some(account);
                }
            }
        });
    // amount, currency, memo fields unchanged
    chosen
}
```

  and in the postings grid: collect `let mut picked: Vec<Vec<String>> = vec![];` and push `a.default_tags.clone()` for each `Some(a)` returned; after the grid, `for tags in picked { form.add_tags(&tags); }`.
- In `ledger.rs` tests, `account()` gains `default_tags: vec![]` (already done in Task 5).

`main.rs`: `let ledger_client = Arc::new(LedgerClient::new(...));` (add `use std::sync::Arc;`), pass `ledger_client` to `LedgerView::new`.

- [ ] **Step 4: Run tests and look**

Run: `cargo test -p sapphire-ledger-desktop && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(desktop): default tags on accounts and in the transaction form"
```

---

### Task 10: Desktop — the Charts screen

**Files:**
- Modify: `desktop/Cargo.toml` (`egui_plot = "0.37"` with a comment `# Plots for the Charts screen; 0.37 renders with egui 0.36, the framework's.`)
- Create: `desktop/assets/icons/chart-line.svg`
- Create: `desktop/src/charts.rs`
- Modify: `desktop/src/form.rs` (`BudgetForm`)
- Modify: `desktop/src/main.rs` (`Screen::Charts`, `mod charts;`)
- Modify: `desktop/src/client.rs` (client test gains a budget round trip)
- Test: `desktop/src/form.rs`, `desktop/src/charts.rs` tests, `desktop/src/client.rs`

**Interfaces:**
- Consumes: `api::{Budget, BudgetReport, BudgetDay, AddBudgetParams, UpdateBudgetParams, BudgetReportParams, LIST_BUDGETS, LIST_ACCOUNTS, BUDGET_REPORT, ADD_BUDGET, UPDATE_BUDGET}`, `form::{parse_amount, split_list}`.
- Produces:
  - `BudgetForm { pub id: Option<String>, pub name, pub tag, pub currency, pub monthly_amount: String, pub balance_accounts: Vec<String> /* account ids */ }`, `BudgetForm::new(currency: &str)`, `BudgetForm::edit(&Budget, &[Account])`, `toggle(&mut self, id: &str)`, `to_add_params() -> Result<AddBudgetParams, String>`, `to_update_params() -> Result<UpdateBudgetParams, String>`
  - `ChartsView::new(runtime, client: Arc<LedgerClient>, repaint)`, `ChartsView::ui(&mut self, ui)`

- [ ] **Step 1: Write the failing tests**

`form.rs` tests:

```rust
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
        assert_eq!((p.name.as_str(), p.monthly_amount.as_str()), ("生活費", "100000"));
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
            balance_accounts: vec![sapphire_ledger_api::AccountRef { account_id: None, account_name: Some("Assets:Bank".into()) }],
        };
        let form = BudgetForm::edit(&budget, &accounts);
        assert_eq!(form.balance_accounts, ["acc0001"]);
        let p = form.to_update_params().unwrap();
        assert_eq!(p.budget, "bud0001");
        assert_eq!(p.monthly_amount.as_deref(), Some("100000"));
        assert_eq!(p.balance_accounts, Some(vec!["acc0001".to_string()]));
    }
```

`charts.rs` tests (written with the module in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn day(n: u32, spent: &str, spent_total: &str, budget_total: &str, elapsed: bool, actual: Option<&str>) -> BudgetDay {
        BudgetDay {
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, n).unwrap(),
            spent: spent.into(),
            spent_cumulative: spent_total.into(),
            budget_cumulative: budget_total.into(),
            planned_balance: "0".into(),
            elapsed,
            actual_balance: actual.map(Into::into),
        }
    }

    fn report(days: Vec<BudgetDay>) -> BudgetReport {
        BudgetReport {
            budget: Budget {
                id: "b".into(),
                name: "生活費".into(),
                tag: "living".into(),
                currency: "JPY".into(),
                monthly_amount: "100000".into(),
                balance_accounts: vec![],
            },
            month: "2026-10".into(),
            daily_budget: "3225.81".into(),
            opening_balance: "0".into(),
            days,
        }
    }

    #[test]
    fn the_actual_lines_stop_at_the_last_elapsed_day() {
        let r = report(vec![
            day(1, "1000", "1000", "3225.81", true, Some("-1000")),
            day(2, "0", "1000", "6451.61", true, Some("-1000")),
            day(3, "0", "1000", "9677.42", false, None),
        ]);
        let s = pace_series(&r);
        assert_eq!(s.budget.len(), 3);
        assert_eq!(s.spent, vec![[1.0, 1000.0], [2.0, 1000.0]]);
        assert_eq!(s.daily, vec![[1.0, 1000.0], [2.0, 0.0]]);
        assert!((s.daily_budget - 3225.81).abs() < 1e-9);
        let b = balance_series(&r).expect("there are actual balances");
        assert_eq!(b.actual.len(), 2);
        assert_eq!(b.planned.len(), 3);
    }

    #[test]
    fn no_actual_balance_means_no_balance_chart() {
        let r = report(vec![day(1, "0", "0", "3225.81", true, None)]);
        assert!(balance_series(&r).is_none());
    }

    #[test]
    fn the_summary_says_where_today_stands() {
        let r = report(vec![
            day(1, "1000", "1000", "3225.81", true, None),
            day(2, "8000", "9000", "6451.61", true, None),
            day(3, "0", "9000", "9677.42", false, None),
        ]);
        assert_eq!(summary(&r), "9,000 / 100,000 JPY · 3,225.81 / day · 2,548.39 over pace");
        let r = report(vec![day(1, "0", "0", "3225.81", false, None)]);
        assert_eq!(summary(&r), "0 / 100,000 JPY · 3,225.81 / day");
    }

    #[test]
    fn thousands_are_grouped_and_months_roll_over() {
        assert_eq!(grouped("-1234567.5"), "-1,234,567.5");
        assert_eq!(grouped("999"), "999");
        assert_eq!(shift_month((2026, 12), 1), (2027, 1));
        assert_eq!(shift_month((2026, 1), -1), (2025, 12));
        assert_eq!(month_label((2026, 3)), "2026-03");
    }

    #[test]
    fn stale_replies_are_ignored() {
        let mut generation = Generation::default();
        let old = generation.next();
        let new = generation.next();
        assert!(!generation.is_current(old));
        assert!(generation.is_current(new));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p sapphire-ledger-desktop`
Expected: compile error — `BudgetForm`, module `charts` not found.

- [ ] **Step 3: Implement**

`desktop/assets/icons/chart-line.svg` (Lucide `chart-line`, same attributes as `wallet.svg`):

```xml
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="#ffffff"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M3 3v16a2 2 0 0 0 2 2h16" />
  <path d="m19 9-5 5-4-4-3 3" />
</svg>
```

`form.rs` — `BudgetForm`:

```rust
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
        Ok(AddBudgetParams { name, tag, currency, monthly_amount, balance_accounts: self.balance_accounts.clone() })
    }

    pub fn to_update_params(&self) -> Result<UpdateBudgetParams, String> {
        let (name, tag, currency, monthly_amount) = self.checked()?;
        Ok(UpdateBudgetParams {
            budget: self.id.clone().ok_or("this budget has not been created yet")?,
            name: Some(name),
            tag: Some(tag),
            currency: Some(currency),
            monthly_amount: Some(monthly_amount),
            balance_accounts: Some(self.balance_accounts.clone()),
        })
    }
}
```

(Extend the `sapphire_ledger_api` import in `form.rs` with `Account, AddBudgetParams, Budget, UpdateAccountParams, UpdateBudgetParams`.)

`desktop/src/charts.rs`:

```rust
//! The Charts screen: one budget's month as the server's `budget_report`
//! answers it — the daily pace, and the balance path.
//!
//! Like the Ledger screen it is a client of the running server: calls run on
//! the tokio runtime and come back through a channel, and the view never
//! blocks. Everything it draws is computed by the server; the helpers here
//! only turn the answer into plot points and text.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use chrono::Datelike;
use egui_plot::{Bar, BarChart, HLine, HoverPosition, Legend, Line, LineStyle, Plot, PlotPoints};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use sapphire_framework::gui::egui::{self, Color32};
use sapphire_ledger_api::{self as api, Account, Budget, BudgetDay, BudgetReport, BudgetReportParams, EmptyParams};

use crate::client::LedgerClient;
use crate::form::{self, BudgetForm};

/// As the Ledger screen: pick up what the CLI, an agent or sync wrote.
const REFRESH_EVERY: Duration = Duration::from_secs(10);

/// Numbers each load, so a reply for a budget or month no longer shown is
/// dropped instead of drawn.
#[derive(Default)]
struct Generation(u64);

impl Generation {
    fn next(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
    fn is_current(&self, n: u64) -> bool {
        self.0 == n
    }
}

struct Loaded {
    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    report: Option<BudgetReport>,
}

enum Reply {
    Loaded(u64, Result<Loaded, String>),
    Written(Result<String, String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Pace,
    Balance,
}

pub struct ChartsView {
    runtime: tokio::runtime::Handle,
    client: Arc<LedgerClient>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    replies: (mpsc::Sender<Reply>, mpsc::Receiver<Reply>),
    generation: Generation,

    budgets: Vec<Budget>,
    accounts: Vec<Account>,
    /// The budget shown, by id; `None` until the first load picks one.
    selected: Option<String>,
    month: (i32, u32),
    report: Option<BudgetReport>,
    tab: Tab,
    load_error: Option<String>,
    loading: bool,
    last_refresh: Option<Instant>,

    form: Option<BudgetForm>,
    sending: bool,
    form_error: Option<String>,
}

impl ChartsView {
    pub fn new(runtime: tokio::runtime::Handle, client: Arc<LedgerClient>, repaint: Arc<dyn Fn() + Send + Sync>) -> ChartsView {
        let today = chrono::Local::now().date_naive();
        ChartsView {
            runtime,
            client,
            repaint,
            replies: mpsc::channel(),
            generation: Generation::default(),
            budgets: vec![],
            accounts: vec![],
            selected: None,
            month: (today.year(), today.month()),
            report: None,
            tab: Tab::Pace,
            load_error: None,
            loading: false,
            last_refresh: None,
            form: None,
            sending: false,
            form_error: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.drain_replies();
        if !self.loading && self.last_refresh.is_none_or(|t| t.elapsed() >= REFRESH_EVERY) {
            self.refresh();
        }
        ui.ctx().request_repaint_after(REFRESH_EVERY);

        self.toolbar(ui);
        ui.separator();
        match &self.report {
            None if self.budgets.is_empty() && !self.loading => {
                ui.label("No budgets yet. A budget counts the transactions carrying its tag: start with New budget.");
            }
            None => {
                ui.spinner();
            }
            Some(report) => {
                ui.label(summary(report));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Pace, "Daily pace");
                    ui.selectable_value(&mut self.tab, Tab::Balance, "Balance path");
                });
                let colors = Colors::for_mode(ui.visuals().dark_mode);
                match self.tab {
                    Tab::Pace => pace_plots(ui, report, colors),
                    Tab::Balance => match balance_series(report) {
                        Some(series) => balance_plot(ui, series, colors),
                        None => {
                            ui.label("This budget has no accounts it is paid from. Choose some with Edit to see its balance path.");
                        }
                    },
                }
            }
        }
        self.form_window(ui.ctx());
    }

    // ── server calls ────────────────────────────────────────────────────────

    fn refresh(&mut self) {
        self.loading = true;
        self.last_refresh = Some(Instant::now());
        let n = self.generation.next();
        let client = Arc::clone(&self.client);
        let selected = self.selected.clone();
        let month = month_label(self.month);
        self.spawn(async move {
            let loaded = async {
                let budgets: Vec<Budget> = client.read(api::LIST_BUDGETS, EmptyParams {}).await?;
                let accounts: Vec<Account> = client.read(api::LIST_ACCOUNTS, EmptyParams {}).await?;
                let shown = selected
                    .filter(|id| budgets.iter().any(|b| &b.id == id))
                    .or_else(|| budgets.first().map(|b| b.id.clone()));
                let report = match shown {
                    Some(budget) => Some(client.read(api::BUDGET_REPORT, BudgetReportParams { budget, month }).await?),
                    None => None,
                };
                Ok(Loaded { budgets, accounts, report })
            };
            Reply::Loaded(n, loaded.await)
        });
    }

    /// Show another budget or month now, not at the next tick.
    fn reload_now(&mut self) {
        self.report = None;
        self.loading = false;
        self.last_refresh = None;
    }

    fn send_form(&mut self) {
        let Some(form) = &self.form else { return };
        let client = Arc::clone(&self.client);
        let call = match &form.id {
            None => form.to_add_params().map(|p| (api::ADD_BUDGET, serde_json::to_value(p))),
            Some(_) => form.to_update_params().map(|p| (api::UPDATE_BUDGET, serde_json::to_value(p))),
        };
        match call {
            Ok((method, Ok(params))) => {
                self.sending = true;
                self.form_error = None;
                self.spawn(async move { Reply::Written(client.call(method, params).await) });
            }
            Ok((_, Err(e))) => self.form_error = Some(e.to_string()),
            Err(e) => self.form_error = Some(e),
        }
    }

    fn spawn(&self, call: impl Future<Output = Reply> + Send + 'static) {
        let tx = self.replies.0.clone();
        let repaint = Arc::clone(&self.repaint);
        self.runtime.spawn(async move {
            let _ = tx.send(call.await);
            repaint();
        });
    }

    fn drain_replies(&mut self) {
        while let Ok(reply) = self.replies.1.try_recv() {
            match reply {
                Reply::Loaded(n, _) if !self.generation.is_current(n) => {}
                Reply::Loaded(_, Ok(loaded)) => {
                    self.selected = loaded.report.as_ref().map(|r| r.budget.id.clone());
                    self.budgets = loaded.budgets;
                    self.accounts = loaded.accounts;
                    self.report = loaded.report;
                    self.load_error = None;
                    self.loading = false;
                }
                Reply::Loaded(_, Err(e)) => {
                    // Keep the last chart: a server restart should not blank it.
                    self.load_error = Some(e);
                    self.loading = false;
                }
                Reply::Written(Ok(_)) => {
                    self.sending = false;
                    self.form = None;
                    self.reload_now();
                }
                Reply::Written(Err(e)) => {
                    self.sending = false;
                    self.form_error = Some(e);
                }
            }
        }
    }

    // ── panels ──────────────────────────────────────────────────────────────

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Owned, so the arms below can call `&mut self` methods.
            let shown: Option<Budget> =
                self.budgets.iter().find(|b| Some(&b.id) == self.selected.as_ref()).cloned();
            let mut picked = self.selected.clone();
            egui::ComboBox::from_id_salt("charts_budget")
                .selected_text(shown.as_ref().map_or("Budget…", |b| b.name.as_str()))
                .show_ui(ui, |ui| {
                    for b in &self.budgets {
                        ui.selectable_value(&mut picked, Some(b.id.clone()), &b.name);
                    }
                });
            let mut month = self.month;
            if ui.small_button("◀").on_hover_text("Previous month").clicked() {
                month = shift_month(month, -1);
            }
            ui.monospace(month_label(month));
            if ui.small_button("▶").on_hover_text("Next month").clicked() {
                month = shift_month(month, 1);
            }
            if picked != self.selected || month != self.month {
                self.selected = picked;
                self.month = month;
                self.reload_now();
            }
            let can_write = self.load_error.is_none();
            if ui.add_enabled(can_write, egui::Button::new("New budget")).clicked() {
                let currency = shown.as_ref().map_or("JPY", |b| b.currency.as_str()).to_owned();
                self.form = Some(BudgetForm::new(&currency));
                self.form_error = None;
            }
            if ui.add_enabled(can_write && shown.is_some(), egui::Button::new("Edit")).clicked()
                && let Some(b) = &shown
            {
                self.form = Some(BudgetForm::edit(b, &self.accounts));
                self.form_error = None;
            }
            if self.loading {
                ui.spinner();
            }
        });
        if let Some(e) = &self.load_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }

    fn form_window(&mut self, ctx: &egui::Context) {
        let Some(form) = &mut self.form else { return };
        let mut assets: Vec<&Account> =
            self.accounts.iter().filter(|a| a.account_type == api::AccountType::Asset).collect();
        assets.sort_by(|a, b| a.name.cmp(&b.name));
        let title = if form.id.is_some() { "Edit budget" } else { "New budget" };
        let (mut open, mut submit) = (true, false);
        egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            egui::Grid::new("budget_fields").num_columns(2).show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut form.name).hint_text("生活費"));
                ui.end_row();
                ui.label("Tag");
                ui.add(egui::TextEdit::singleline(&mut form.tag).hint_text("living"));
                ui.end_row();
                ui.label("Currency");
                ui.add(egui::TextEdit::singleline(&mut form.currency).desired_width(60.0));
                ui.end_row();
                ui.label("Per month");
                ui.add(egui::TextEdit::singleline(&mut form.monthly_amount).hint_text("100000"));
                ui.end_row();
            });
            ui.separator();
            ui.label("Paid from (for the balance path)");
            if assets.is_empty() {
                ui.weak("No asset accounts yet.");
            }
            for a in &assets {
                let mut on = form.balance_accounts.contains(&a.id);
                if ui.checkbox(&mut on, &a.name).changed() {
                    form.toggle(&a.id);
                }
            }
            ui.separator();
            if let Some(e) = &self.form_error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            ui.horizontal(|ui| {
                submit = ui.add_enabled(!self.sending, egui::Button::new("Save")).clicked();
                if self.sending {
                    ui.spinner();
                }
            });
        });
        if !open {
            self.form = None;
            self.form_error = None;
        } else if submit {
            self.send_form();
        }
    }
}

// ── plots ───────────────────────────────────────────────────────────────────

/// Actual and plan, stepped per theme; validated as a pair with the dataviz
/// validator in both modes. Plan is also dashed, so colour is never alone.
#[derive(Clone, Copy)]
struct Colors {
    actual: Color32,
    plan: Color32,
}

impl Colors {
    fn for_mode(dark: bool) -> Colors {
        if dark {
            Colors { actual: Color32::from_rgb(0x39, 0x87, 0xe5), plan: Color32::from_rgb(0xd9, 0x59, 0x26) }
        } else {
            Colors { actual: Color32::from_rgb(0x2a, 0x78, 0xd6), plan: Color32::from_rgb(0xeb, 0x68, 0x34) }
        }
    }
}

/// A plot of one month: day 1..=n on x, nothing to pan or zoom.
fn month_plot<'a>(id: &'a str, days: usize) -> Plot<'a> {
    Plot::new(id)
        .legend(Legend::default())
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .include_x(0.5)
        .include_x(days as f64 + 0.5)
        .include_y(0.0)
        .link_axis("budget_month", [true, false])
        .link_cursor("budget_month", [true, false])
        .x_axis_formatter(|mark, _| format!("{}", mark.value.round() as i64))
        .y_axis_formatter(|mark, _| grouped(&format!("{}", mark.value.round() as i64)))
        .label_formatter(|pos: &HoverPosition<'_>| match pos {
            HoverPosition::NearDataPoint { plot_name, position, .. } => Some(format!(
                "{plot_name}\nday {}: {}",
                position.x.round() as i64,
                grouped(&format!("{:.2}", position.y)),
            )),
            HoverPosition::Elsewhere { .. } => None,
        })
}

fn pace_plots(ui: &mut egui::Ui, report: &BudgetReport, c: Colors) {
    let s = pace_series(report);
    let n = report.days.len();
    let tall = (ui.available_height() * 0.62).max(160.0);
    month_plot("budget_pace", n).height(tall).show(ui, |p| {
        p.line(Line::new("Budget so far", PlotPoints::from(s.budget)).color(c.plan).width(2.0).style(LineStyle::dashed_loose()));
        p.line(Line::new("Spent so far", PlotPoints::from(s.spent)).color(c.actual).width(2.0));
    });
    month_plot("budget_daily", n).height(ui.available_height().max(100.0)).show(ui, |p| {
        let bars = s.daily.iter().map(|[x, y]| Bar::new(*x, *y).width(0.7)).collect();
        p.bar_chart(BarChart::new("Spent that day", bars).color(c.actual));
        p.hline(HLine::new("Daily budget", s.daily_budget).color(c.plan).width(2.0).style(LineStyle::dashed_loose()));
    });
}

fn balance_plot(ui: &mut egui::Ui, s: BalanceSeries, c: Colors) {
    month_plot("budget_balance", s.planned.len()).height(ui.available_height().max(160.0)).show(ui, |p| {
        p.line(Line::new("Planned balance", PlotPoints::from(s.planned)).color(c.plan).width(2.0).style(LineStyle::dashed_loose()));
        p.line(Line::new("Actual balance", PlotPoints::from(s.actual)).color(c.actual).width(2.0));
    });
}

// ── pure helpers ────────────────────────────────────────────────────────────

struct PaceSeries {
    budget: Vec<[f64; 2]>,
    /// Elapsed days only: the line stops at today.
    spent: Vec<[f64; 2]>,
    daily: Vec<[f64; 2]>,
    daily_budget: f64,
}

struct BalanceSeries {
    planned: Vec<[f64; 2]>,
    actual: Vec<[f64; 2]>,
}

fn number(raw: &str) -> f64 {
    form::parse_amount(raw).and_then(|d| d.to_f64()).unwrap_or_default()
}

fn x(d: &BudgetDay) -> f64 {
    f64::from(d.date.day())
}

fn pace_series(r: &BudgetReport) -> PaceSeries {
    let elapsed = || r.days.iter().filter(|d| d.elapsed);
    PaceSeries {
        budget: r.days.iter().map(|d| [x(d), number(&d.budget_cumulative)]).collect(),
        spent: elapsed().map(|d| [x(d), number(&d.spent_cumulative)]).collect(),
        daily: elapsed().map(|d| [x(d), number(&d.spent)]).collect(),
        daily_budget: number(&r.daily_budget),
    }
}

/// `None` when nothing has an actual balance: no accounts to draw.
fn balance_series(r: &BudgetReport) -> Option<BalanceSeries> {
    let actual: Vec<[f64; 2]> = r
        .days
        .iter()
        .filter_map(|d| d.actual_balance.as_deref().map(|a| [x(d), number(a)]))
        .collect();
    if actual.is_empty() && r.budget.balance_accounts.is_empty() {
        return None;
    }
    Some(BalanceSeries { planned: r.days.iter().map(|d| [x(d), number(&d.planned_balance)]).collect(), actual })
}

/// `9,000 / 100,000 JPY · 3,225.81 / day · 2,548.39 over pace`. The pace
/// part is left out until a day has elapsed.
fn summary(r: &BudgetReport) -> String {
    let last = r.days.iter().rev().find(|d| d.elapsed);
    let spent = last.map_or("0", |d| d.spent_cumulative.as_str());
    let mut text = format!(
        "{} / {} {} · {} / day",
        grouped(spent),
        grouped(&r.budget.monthly_amount),
        r.budget.currency,
        grouped(&r.daily_budget),
    );
    if let Some(d) = last {
        let ahead = form::parse_amount(&d.budget_cumulative).unwrap_or_default()
            - form::parse_amount(&d.spent_cumulative).unwrap_or_default();
        let word = if ahead < Decimal::ZERO { "over" } else { "under" };
        text.push_str(&format!(" · {} {word} pace", grouped(&ahead.abs().normalize().to_string())));
    }
    text
}

/// `-1234567.5` → `-1,234,567.5`.
fn grouped(raw: &str) -> String {
    let (sign, digits) = raw.strip_prefix('-').map_or(("", raw), |d| ("-", d));
    let (int, frac) = digits.split_once('.').map_or((digits, None), |(i, f)| (i, Some(f)));
    let mut out = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

fn shift_month((year, month): (i32, u32), by: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + by;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

fn month_label((year, month): (i32, u32)) -> String {
    format!("{year:04}-{month:02}")
}
```

(The test module from Step 1 goes at the bottom. `summary`'s test expects `2,548.39 over pace`: `6451.61 − 9000 = −2548.39`.)

`main.rs`:
- `mod charts;`, `use crate::charts::ChartsView;`
- `enum Screen { Ledger, Charts }`; `struct App { shell, ledger, charts: ChartsView }`
- in `ui`: `let (ledger, charts) = (&mut self.ledger, &mut self.charts);` and the match arm `Screen::Charts => { egui::CentralPanel::default().show(ui, |ui| charts.ui(ui)); }`
- destinations: after Ledger, `Destination::new(Screen::Charts, egui::include_image!("../assets/icons/chart-line.svg"), "Charts")`
- construct: `charts: ChartsView::new(setup.runtime.clone(), Arc::clone(&ledger_client), Arc::clone(&setup.repaint)),` and pass clones to `LedgerView::new` the same way (whatever `setup.runtime`/`setup.repaint` are — a `Handle` and an `Arc` — clone them for the first view).
- Update the module doc's second paragraph: "The Ledger screen … ; the Charts screen draws a budget's month from `budget_report`; Settings …".

`client.rs` test — after the `issues` check, add (with `form::BudgetForm` and `api::{BudgetReport, BudgetReportParams}` imported):

```rust
        let mut budget = BudgetForm::new("JPY");
        budget.name = "生活費".into();
        budget.tag = "living".into();
        budget.monthly_amount = "100000".into();
        let cash = accounts.iter().find(|a| a.name == "Assets:Cash").unwrap();
        budget.toggle(&cash.id);
        client.call(api::ADD_BUDGET, budget.to_add_params().unwrap()).await.expect("add_budget");
        let report: BudgetReport = client
            .read(api::BUDGET_REPORT, BudgetReportParams { budget: "生活費".into(), month: "2026-05".into() })
            .await
            .expect("the report parses");
        assert_eq!(report.days.len(), 31);
        assert_eq!(report.days[20].actual_balance.as_deref(), Some("-1200"));
```

- [ ] **Step 4: Run tests, lint, and look at it**

Run: `cargo test -p sapphire-ledger-desktop && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.
Then invoke the `run` skill (or `cargo run -p sapphire-ledger-desktop` with `sapphire-ledger serve` running on a scratch ledger that has a budget and a few tagged transactions this month) and check, in light and dark: the Charts icon on the rail; both tabs; the actual lines stop at today; hover labels; month arrows; the legend not covering the lines; New/Edit budget save and redraw.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(desktop): Charts screen comparing a budget with spending and balance"
```

---

### Task 11: Documentation and final verification

**Files:**
- Modify: `docs/design.md`
- Modify: `docs/superpowers/specs/2026-09-01-restart-roadmap-design.md`

- [ ] **Step 1: `docs/design.md`**
  - Directory tree: add `└── budgets/\n    └── {grain-id}.toml` after `prices/`.
  - Account section: after the YAML example add a paragraph: "`default_tags` (optional) are copied into a transaction that posts to the account when it is recorded — on write, not on read, so the file's tags stay the whole truth and changing an account's defaults leaves past transactions alone. `add_transaction` applies them unless `apply_default_tags` is false; the desktop shows them in the form and sends false."
  - New section `### Budget ([`budget.rs`](../crates/sapphire-ledger-core/src/budget.rs))` after "Price log", with the TOML example from the spec and the report rules (the "The report" bullet list from the spec, condensed), and the note that a budget is not dated so its path is `budgets/{id}.toml` with a random id.
  - Validation list: add "Two budgets share an `id` or a `name`; a budget's balance account does not exist; a budget's `monthly_amount` is not positive."
- [ ] **Step 2: Roadmap** — under "Explicitly **not** planned", append: "*(2026-10-16: budgets were built after all — see [budgets and charts](2026-10-16-budgets-and-charts-design.md).)*"
- [ ] **Step 3: Full verification**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all PASS, no diff from `cargo fmt` beyond what is committed.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "docs: budgets, default tags and the Charts screen in the design"
```
