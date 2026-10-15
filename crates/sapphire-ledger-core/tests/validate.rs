use std::fs;

use sapphire_ledger_core::{Account, Transaction, init_workspace, load_workspace, save_markdown};

// Account and transaction fixtures are spelled in TOML, the shortest way to
// write a record inline, and stored as the Markdown the ledger reads.

fn write_account(root: &std::path::Path, rel: &str, body: &str) {
    let account: Account = toml::from_str(body).unwrap();
    save_markdown(&root.join(rel), &account).unwrap();
}

fn write_transaction(root: &std::path::Path, rel: &str, body: &str) {
    let transaction: Transaction = toml::from_str(body).unwrap();
    save_markdown(&root.join(rel), &transaction).unwrap();
}

fn write_assertion(root: &std::path::Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

const ACCOUNT_CASH_JPY: &str = r#"
id = "acct001"
name = "Assets:Cash:JPY"
type = "Asset"
currencies = ["JPY"]
opened_at = "2026-05-21"
"#;

const ACCOUNT_FOOD: &str = r#"
id = "acct002"
name = "Expenses:Food"
type = "Expense"
opened_at = "2026-05-21"
"#;

#[test]
fn clean_workspace_reports_no_issues() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    write_account(root, "accounts/Expenses/Food.md", ACCOUNT_FOOD);
    write_transaction(
        root,
        "transactions/2026/05/tx01.md",
        r#"
id = "tx01"
date = "2026-05-21"
narration = "lunch"
created_at = "2026-05-21T12:00:00+09:00"
updated_at = "2026-05-21T12:00:00+09:00"
[[postings]]
account_name = "Expenses:Food"
amount = "1000"
currency = "JPY"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "-1000"
currency = "JPY"
"#,
    );

    let workspace = load_workspace(root).unwrap();
    assert!(workspace.validate().is_empty());
}

#[test]
fn a_posting_with_account_id_and_a_stale_name_resolves_on_disk() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    write_transaction(
        root,
        "transactions/2026/05/idlink.md",
        r#"
id = "idlink"
date = "2026-05-21"
narration = "renamed account, old file untouched"
created_at = "2026-05-21T12:00:00+09:00"
updated_at = "2026-05-21T12:00:00+09:00"
[[postings]]
account_id = "acct001"
account_name = "Assets:Cash:Old:Stale:Name"
amount = "1000"
currency = "JPY"
[[postings]]
account_id = "acct001"
account_name = "Assets:Cash:Old:Stale:Name"
amount = "-1000"
currency = "JPY"
"#,
    );

    let issues = load_workspace(root).unwrap().validate();
    assert!(
        issues.is_empty(),
        "account_id must resolve on disk even though account_name is stale: {issues:?}"
    );
}

#[test]
fn flags_undefined_account_in_transaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    write_transaction(
        root,
        "transactions/2026/05/bad01.md",
        r#"
id = "bad01"
date = "2026-05-21"
narration = "typo"
created_at = "2026-05-21T12:00:00+09:00"
updated_at = "2026-05-21T12:00:00+09:00"
[[postings]]
account_name = "Expenses:Foood"
amount = "1000"
currency = "JPY"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "-1000"
currency = "JPY"
"#,
    );

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("undefined account"));
    assert!(issues[0].message.contains("Expenses:Foood"));
    assert_eq!(issues[0].transaction_id.as_deref(), Some("bad01"));
}

#[test]
fn flags_currency_constraint_violation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    write_account(root, "accounts/Expenses/Food.md", ACCOUNT_FOOD);
    write_transaction(
        root,
        "transactions/2026/05/cur01.md",
        r#"
id = "cur01"
date = "2026-05-21"
narration = "wrong currency"
created_at = "2026-05-21T12:00:00+09:00"
updated_at = "2026-05-21T12:00:00+09:00"
[[postings]]
account_name = "Expenses:Food"
amount = "10"
currency = "USD"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "-10"
currency = "USD"
"#,
    );

    let issues = load_workspace(root).unwrap().validate();
    // Cash:JPY has currencies=["JPY"], so USD posting is invalid.
    // Expenses:Food has no constraint so its USD posting is fine.
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("only allows JPY"));
    assert_eq!(issues[0].account.as_deref(), Some("Assets:Cash:JPY"));
}

#[test]
fn flags_unbalanced_transaction() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    write_account(root, "accounts/Expenses/Food.md", ACCOUNT_FOOD);
    write_transaction(
        root,
        "transactions/2026/05/unbal.md",
        r#"
id = "unbal"
date = "2026-05-21"
narration = "off by one"
created_at = "2026-05-21T12:00:00+09:00"
updated_at = "2026-05-21T12:00:00+09:00"
[[postings]]
account_name = "Expenses:Food"
amount = "1000"
currency = "JPY"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "-999"
currency = "JPY"
"#,
    );

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("does not balance"));
    assert_eq!(issues[0].transaction_id.as_deref(), Some("unbal"));
}

#[test]
fn flags_undefined_account_in_assertion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_assertion(
        root,
        "assertions/2026/05/as01.toml",
        r#"
id = "as01"
account_name = "Assets:Phantom"
date = "2026-05-31"
created_at = "2026-05-31T23:59:00+09:00"
updated_at = "2026-05-31T23:59:00+09:00"
[[balances]]
amount = "100"
currency = "JPY"
"#,
    );

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("undefined account"));
    assert_eq!(issues[0].assertion_id.as_deref(), Some("as01"));
}

#[test]
fn reports_multiple_issues_without_short_circuiting() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    // unbalanced
    write_transaction(
        root,
        "transactions/2026/05/a.md",
        r#"
id = "a"
date = "2026-05-21"
narration = ""
created_at = "2026-05-21T12:00:00+09:00"
updated_at = "2026-05-21T12:00:00+09:00"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "100"
currency = "JPY"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "-99"
currency = "JPY"
"#,
    );
    // undefined account
    write_transaction(
        root,
        "transactions/2026/05/b.md",
        r#"
id = "b"
date = "2026-05-21"
narration = ""
created_at = "2026-05-21T12:00:00+09:00"
updated_at = "2026-05-21T12:00:00+09:00"
[[postings]]
account_name = "Mystery"
amount = "1"
currency = "JPY"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "-1"
currency = "JPY"
"#,
    );

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 2);
    let ids: Vec<&str> = issues
        .iter()
        .filter_map(|i| i.transaction_id.as_deref())
        .collect();
    assert!(ids.contains(&"a"));
    assert!(ids.contains(&"b"));
}

fn write_price(root: &std::path::Path, rel: &str, body: &str) {
    write_assertion(root, rel, body)
}

/// Sync resolves per path, last-writer-wins, so a record moved between months
/// -- a corrected date, a merge -- is a delete-plus-add that can race and
/// leave the same id at two paths. The argument the design already makes for
/// accounts applies verbatim to every other kind.
#[test]
fn flags_a_duplicate_transaction_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    write_account(root, "accounts/Expenses/Food.md", ACCOUNT_FOOD);
    let body = |month: &str| {
        format!(
            r#"
id = "dup01"
date = "2026-{month}-21"
narration = "same id, two months"
created_at = "2026-{month}-21T12:00:00+09:00"
updated_at = "2026-{month}-21T12:00:00+09:00"
[[postings]]
account_name = "Expenses:Food"
amount = "1000"
currency = "JPY"
[[postings]]
account_name = "Assets:Cash:JPY"
amount = "-1000"
currency = "JPY"
"#
        )
    };
    write_transaction(root, "transactions/2026/05/dup01.md", &body("05"));
    write_transaction(root, "transactions/2026/06/dup01.md", &body("06"));

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 1, "issues: {issues:?}");
    assert_eq!(issues[0].message, "duplicate transaction id dup01");
    assert_eq!(issues[0].transaction_id.as_deref(), Some("dup01"));
}

#[test]
fn flags_a_duplicate_assertion_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Assets/Cash/JPY.md", ACCOUNT_CASH_JPY);
    let body = |month: &str| {
        format!(
            r#"
id = "dupas"
account_id = "acct001"
date = "2026-{month}-28"
created_at = "2026-{month}-28T23:59:00+09:00"
updated_at = "2026-{month}-28T23:59:00+09:00"
[[balances]]
amount = "100"
currency = "JPY"
"#
        )
    };
    write_assertion(root, "assertions/2026/05/dupas.toml", &body("05"));
    write_assertion(root, "assertions/2026/06/dupas.toml", &body("06"));

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 1, "issues: {issues:?}");
    assert_eq!(issues[0].message, "duplicate assertion id dupas");
    assert_eq!(issues[0].assertion_id.as_deref(), Some("dupas"));
}

#[test]
fn flags_a_duplicate_price_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    let body = |month: &str| {
        format!(
            r#"
id = "duppr"
date = "2026-{month}-15"
base = "USD"
quote = "JPY"
rate = "150"
created_at = "2026-{month}-15T12:00:00+09:00"
updated_at = "2026-{month}-15T12:00:00+09:00"
"#
        )
    };
    write_price(root, "prices/2026/05/duppr.toml", &body("05"));
    write_price(root, "prices/2026/06/duppr.toml", &body("06"));

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 1, "issues: {issues:?}");
    assert_eq!(issues[0].message, "duplicate price id duppr");
}

/// Unlike a stale `account_name` on a posting, two accounts sharing a name is
/// genuinely broken: a name-only posting resolves to whichever one happened to
/// win the `by_name` insert.
#[test]
fn flags_a_duplicate_account_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    write_account(root, "accounts/Expenses/Food.md", ACCOUNT_FOOD);
    // Same name, different id, parked at a path that does not match it -- a
    // hand edit or a half-applied rename.
    write_account(
        root,
        "accounts/Expenses/Groceries.md",
        r#"
id = "acct003"
name = "Expenses:Food"
type = "Expense"
opened_at = "2026-05-21"
"#,
    );

    let issues = load_workspace(root).unwrap().validate();
    assert_eq!(issues.len(), 1, "issues: {issues:?}");
    assert_eq!(issues[0].message, "duplicate account name Expenses:Food");
    assert_eq!(issues[0].account.as_deref(), Some("Expenses:Food"));
}
