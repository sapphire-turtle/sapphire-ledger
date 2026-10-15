use std::fs;
use std::path::Path;

use chrono::NaiveDate;

use sapphire_ledger_core::{
    Workspace, account_name_from_relative_path, account_name_segments, account_relative_path,
    assertion_relative_path, init_workspace, load_workspace, transaction_relative_path,
    walk_toml_files,
};

#[test]
fn account_name_to_path_roundtrip() {
    let name = "Assets:Cash:USD";
    let rel = account_relative_path(name).unwrap();
    assert_eq!(rel, Path::new("accounts/Assets/Cash/USD.md"));
    let back = account_name_from_relative_path(&rel).unwrap();
    assert_eq!(back, name);
}

#[test]
fn account_name_single_segment() {
    let name = "Equity";
    let rel = account_relative_path(name).unwrap();
    assert_eq!(rel, Path::new("accounts/Equity.md"));
    assert_eq!(account_name_from_relative_path(&rel).unwrap(), name);
}

#[test]
fn account_name_rejects_empty_and_traversal() {
    assert!(account_name_segments("").is_err());
    assert!(account_name_segments("Assets::Cash").is_err());
    assert!(account_name_segments("Assets:..").is_err());
    assert!(account_name_segments("Assets:.").is_err());
    assert!(account_name_segments("Assets:/etc/passwd").is_err());
    assert!(account_name_segments(r"Assets:C:\Windows").is_err());
}

#[test]
fn transaction_path_format() {
    let date = NaiveDate::from_ymd_opt(2026, 5, 21).unwrap();
    let path = transaction_relative_path(date, "0a1b2c3");
    assert_eq!(path, Path::new("transactions/2026/05/0a1b2c3.md"));
}

#[test]
fn assertion_path_format() {
    let date = NaiveDate::from_ymd_opt(2026, 1, 7).unwrap();
    let path = assertion_relative_path(date, "as0001");
    assert_eq!(path, Path::new("assertions/2026/01/as0001.toml"));
}

#[test]
fn walk_finds_nested_toml_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("a/b/c")).unwrap();
    fs::write(root.join("one.toml"), "x = 1").unwrap();
    fs::write(root.join("a/two.toml"), "x = 2").unwrap();
    fs::write(root.join("a/b/c/three.toml"), "x = 3").unwrap();
    fs::write(root.join("a/skip.md"), "not toml").unwrap();

    let files = walk_toml_files(root).unwrap();
    let mut names: Vec<_> = files
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["one.toml", "three.toml", "two.toml"]);
    assert_eq!(files.len(), 3);
}

#[test]
fn walk_missing_dir_returns_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("does-not-exist");
    let files = walk_toml_files(&root).unwrap();
    assert!(files.is_empty());
}

#[test]
fn init_creates_expected_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();

    assert!(root.join(".sapphire-ledger").is_dir());
    assert!(root.join(".sapphire-ledger/config.toml").is_file());
    assert!(root.join(".sapphire-ledger/.gitignore").is_file());
    assert!(root.join("transactions").is_dir());
    assert!(root.join("accounts").is_dir());
    assert!(root.join("assertions").is_dir());

    let gitignore = fs::read_to_string(root.join(".sapphire-ledger/.gitignore")).unwrap();
    assert!(gitignore.contains("cache.sqlite"));

    let workspace: Workspace = load_workspace(root).unwrap();
    assert_eq!(workspace.config.base_currency, "JPY");
    assert!(workspace.accounts.is_empty());
    assert!(workspace.transactions.is_empty());
    assert!(workspace.assertions.is_empty());
}

#[test]
fn init_refuses_existing_workspace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    let err = init_workspace(root, "JPY").unwrap_err();
    assert!(format!("{err}").contains("already contains"));
}

/// `sapphire-ledger workspace init` makes the marker and writes only the
/// framework's registry into `config.toml`; `init` turns that into a ledger
/// and keeps the registry.
#[test]
fn init_completes_a_marker_the_framework_made() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join(".sapphire-ledger")).unwrap();
    fs::write(
        root.join(".sapphire-ledger/config.toml"),
        "[workspace.home]\nroot = \"/somewhere\"\n",
    )
    .unwrap();

    init_workspace(root, "USD").unwrap();

    let workspace: Workspace = load_workspace(root).unwrap();
    assert_eq!(workspace.config.base_currency, "USD");
    assert!(root.join("transactions").is_dir());
    let text = fs::read_to_string(root.join(".sapphire-ledger/config.toml")).unwrap();
    assert!(
        text.contains("[workspace.home]"),
        "the registry must survive: {text}"
    );

    let err = init_workspace(root, "USD").unwrap_err();
    assert!(format!("{err}").contains("already contains"));
}

/// Since sapphire-framework 0.20, `workspace init` makes the marker and no
/// `config.toml`; `init` turns that into a ledger too.
#[test]
fn init_completes_a_bare_marker() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join(".sapphire-ledger")).unwrap();

    init_workspace(root, "USD").unwrap();

    let workspace: Workspace = load_workspace(root).unwrap();
    assert_eq!(workspace.config.base_currency, "USD");
    assert!(root.join("transactions").is_dir());

    let err = init_workspace(root, "USD").unwrap_err();
    assert!(format!("{err}").contains("already contains"));
}

#[test]
fn load_workspace_reads_records() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();

    fs::create_dir_all(root.join("accounts/Assets/Cash")).unwrap();
    fs::write(
        root.join("accounts/Assets/Cash/JPY.md"),
        r#"---
id: acct001
name: Assets:Cash:JPY
type: Asset
opened_at: 2026-05-21
---

The wallet.
"#,
    )
    .unwrap();

    fs::create_dir_all(root.join("accounts/Expenses")).unwrap();
    fs::write(
        root.join("accounts/Expenses/Food.md"),
        r#"---
id: acct002
name: Expenses:Food
type: Expense
opened_at: 2026-05-21
---
"#,
    )
    .unwrap();

    fs::create_dir_all(root.join("transactions/2026/05")).unwrap();
    fs::write(
        root.join("transactions/2026/05/tx0001.md"),
        r#"---
id: tx0001
date: 2026-05-21
narration: lunch
created_at: 2026-05-21T12:00:00+09:00
updated_at: 2026-05-21T12:00:00+09:00
postings:
- account_name: Expenses:Food
  amount: 1000
  currency: JPY
- account_name: Assets:Cash:JPY
  amount: -1000
  currency: JPY
---

Split with a colleague.

- paid in cash
"#,
    )
    .unwrap();

    let workspace = load_workspace(root).unwrap();
    assert_eq!(workspace.accounts.len(), 2);
    assert_eq!(workspace.transactions.len(), 1);
    assert_eq!(workspace.transactions[0].narration, "lunch");
    assert_eq!(
        workspace.transactions[0].comment.as_deref(),
        Some("Split with a colleague.\n\n- paid in cash")
    );
    let wallet = workspace
        .accounts
        .iter()
        .find(|a| a.id == "acct001")
        .unwrap();
    assert_eq!(wallet.description.as_deref(), Some("The wallet."));
    let food = workspace
        .accounts
        .iter()
        .find(|a| a.id == "acct002")
        .unwrap();
    assert_eq!(food.description, None, "an empty body is no description");
}
