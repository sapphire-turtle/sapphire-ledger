use std::fs;
use std::path::Path;

use sapphire_ledger_core::{
    Account, AccountType, CURRENT_SCHEMA_VERSION, Transaction, init_workspace, load_markdown,
    load_workspace, migrate_workspace, ops,
};

/// A ledger as a schema-1 build left it: TOML everywhere.
fn v1_ledger(root: &Path) {
    init_workspace(root, "JPY").expect("init");
    let config = root.join(".sapphire-ledger/config.toml");
    let text = fs::read_to_string(&config).unwrap().replace(
        &format!("schema_version = {CURRENT_SCHEMA_VERSION}"),
        "schema_version = 1",
    );
    fs::write(&config, text).unwrap();

    write(
        root,
        "accounts/Assets/Cash/JPY.toml",
        r#"id = "acct001"
name = "Assets:Cash:JPY"
type = "Asset"
currencies = ["JPY"]
opened_at = "2026-05-21"
description = """
財布の現金。

- 小銭も含む
"""
"#,
    );
    write(
        root,
        "accounts/Expenses/Food.toml",
        r#"# groceries and eating out
id = "acct002"
name = "Expenses:Food"
type = "Expense"
opened_at = "2026-05-21"
"#,
    );
    write(
        root,
        "transactions/2026/05/tx0001.toml",
        r#"id = "tx0001"
date = "2026-05-21"
narration = "イオン買い物"
payee = "イオン"
tags = ["grocery", "yes"]
status = "cleared"
created_at = "2026-05-21T18:30:00+09:00"
updated_at = "2026-05-21T18:30:00+09:00"

[[postings]]
account_id = "acct002"
account_name = "Expenses:Food"
amount = "1200.50"
currency = "JPY"
memo = "room #3"

[[postings]]
account_id = "acct001"
account_name = "Assets:Cash:JPY"
amount = "-1200.50"
currency = "JPY"
"#,
    );
    write(
        root,
        "assertions/2026/05/as0001.toml",
        r#"id = "as0001"
account_id = "acct001"
date = "2026-05-31"
created_at = "2026-05-31T23:59:00+09:00"
updated_at = "2026-05-31T23:59:00+09:00"

[[balances]]
amount = "-1200.50"
currency = "JPY"
"#,
    );
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn schema_version(root: &Path) -> String {
    let text = fs::read_to_string(root.join(".sapphire-ledger/config.toml")).unwrap();
    text.lines()
        .find(|l| l.starts_with("schema_version"))
        .unwrap()
        .to_owned()
}

#[test]
fn a_v1_ledger_is_refused_until_migrated() {
    let dir = tempfile::tempdir().unwrap();
    v1_ledger(dir.path());

    let err = load_workspace(dir.path()).expect_err("schema 1 must not load");
    let chain = format!("{err}: {}", std::error::Error::source(&err).unwrap());
    assert!(chain.contains("sapphire-ledger migrate"), "got: {chain}");

    let err = ops::create_account(
        dir.path(),
        "Assets:Bank".into(),
        AccountType::Asset,
        vec![],
        "2026-05-21".parse().unwrap(),
        None,
    )
    .expect_err("a write into a schema-1 ledger must be refused");
    let chain = format!("{err}: {}", std::error::Error::source(&err).unwrap());
    assert!(chain.contains("sapphire-ledger migrate"), "got: {chain}");
    assert!(!dir.path().join("accounts/Assets/Bank.md").exists());
}

#[test]
fn migrate_moves_accounts_and_transactions_to_markdown_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    v1_ledger(root);
    let before_account: Account =
        sapphire_ledger_core::load_toml(&root.join("accounts/Assets/Cash/JPY.toml")).unwrap();
    let before_tx: Transaction =
        sapphire_ledger_core::load_toml(&root.join("transactions/2026/05/tx0001.toml")).unwrap();

    let report = migrate_workspace(root, false).expect("migrate");
    assert_eq!((report.from_version, report.to_version), (1, 2));
    assert_eq!(report.converted.len(), 3);
    assert_eq!(
        report.comments_dropped,
        vec![root.join("accounts/Expenses/Food.toml")],
        "a `#` inside a string is not a comment"
    );
    assert_eq!(schema_version(root), "schema_version = 2");

    for rel in [
        "accounts/Assets/Cash/JPY",
        "accounts/Expenses/Food",
        "transactions/2026/05/tx0001",
    ] {
        assert!(!root.join(format!("{rel}.toml")).exists(), "{rel}.toml");
        assert!(root.join(format!("{rel}.md")).exists(), "{rel}.md");
    }
    assert!(
        root.join("assertions/2026/05/as0001.toml").exists(),
        "assertions stay TOML"
    );

    let account: Account = load_markdown(&root.join("accounts/Assets/Cash/JPY.md")).unwrap();
    assert_eq!(
        account.description.as_deref(),
        Some("財布の現金。\n\n- 小銭も含む")
    );
    assert_eq!(
        Account {
            description: None,
            ..account
        },
        Account {
            description: None,
            ..before_account
        }
    );
    let tx: Transaction = load_markdown(&root.join("transactions/2026/05/tx0001.md")).unwrap();
    assert_eq!(tx, before_tx);

    let text = fs::read_to_string(root.join("accounts/Assets/Cash/JPY.md")).unwrap();
    assert!(
        text.starts_with("---\nid: acct001\n")
            && text.ends_with("---\n\n財布の現金。\n\n- 小銭も含む\n"),
        "got:\n{text}"
    );

    let workspace = load_workspace(root).expect("a migrated ledger loads");
    assert!(
        workspace.validate().is_empty(),
        "{:?}",
        workspace.validate()
    );
}

#[test]
fn a_dry_run_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    v1_ledger(dir.path());

    let report = migrate_workspace(dir.path(), true).expect("dry run");
    assert_eq!(report.converted.len(), 3);
    assert_eq!(schema_version(dir.path()), "schema_version = 1");
    assert!(dir.path().join("accounts/Expenses/Food.toml").exists());
    assert!(!dir.path().join("accounts/Expenses/Food.md").exists());
}

#[test]
fn migrate_is_safe_to_rerun_and_finishes_an_interrupted_run() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    v1_ledger(root);

    // Interrupted after writing one Markdown file but before removing its
    // TOML, and before the version bump.
    let toml = root.join("transactions/2026/05/tx0001.toml");
    let tx: Transaction = sapphire_ledger_core::load_toml(&toml).unwrap();
    sapphire_ledger_core::save_markdown(&toml.with_extension("md"), &tx).unwrap();

    migrate_workspace(root, false).expect("finish the migration");
    assert!(!toml.exists());
    load_workspace(root).expect("loads");

    let again = migrate_workspace(root, false).expect("re-run");
    assert!(again.converted.is_empty());
    assert_eq!((again.from_version, again.to_version), (2, 2));
}

#[test]
fn a_conflicting_markdown_file_stops_the_migration_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    v1_ledger(root);

    let toml = root.join("transactions/2026/05/tx0001.toml");
    let mut tx: Transaction = sapphire_ledger_core::load_toml(&toml).unwrap();
    tx.narration = "edited elsewhere".into();
    sapphire_ledger_core::save_markdown(&toml.with_extension("md"), &tx).unwrap();

    let err = migrate_workspace(root, false).expect_err("must refuse");
    assert!(err.to_string().contains("tx0001.md"), "got: {err}");
    assert!(toml.exists());
    assert!(
        root.join("accounts/Expenses/Food.toml").exists(),
        "nothing moves when anything conflicts"
    );
    assert_eq!(schema_version(root), "schema_version = 1");
}

/// A client still on schema 1 syncs a TOML transaction into a ledger another
/// machine already migrated. Skipping it would leave the books quietly wrong.
#[test]
fn a_toml_record_in_a_migrated_ledger_is_refused_and_swept_up_by_migrate() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    v1_ledger(root);
    migrate_workspace(root, false).unwrap();

    write(
        root,
        "transactions/2026/06/late01.toml",
        r#"id = "late01"
date = "2026-06-01"
narration = "from an old client"
created_at = "2026-06-01T09:00:00+09:00"
updated_at = "2026-06-01T09:00:00+09:00"

[[postings]]
account_id = "acct002"
amount = "100"
currency = "JPY"

[[postings]]
account_id = "acct001"
amount = "-100"
currency = "JPY"
"#,
    );
    let err = load_workspace(root).expect_err("a stray TOML record must fail the load");
    assert!(err.to_string().contains("late01.toml"), "got: {err}");

    let report = migrate_workspace(root, false).expect("sweep");
    assert_eq!(report.converted.len(), 1);
    assert_eq!(load_workspace(root).unwrap().transactions.len(), 2);
}

#[test]
fn a_newer_ledger_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_workspace(root, "JPY").unwrap();
    let config = root.join(".sapphire-ledger/config.toml");
    let text = fs::read_to_string(&config).unwrap().replace(
        &format!("schema_version = {CURRENT_SCHEMA_VERSION}"),
        "schema_version = 99",
    );
    fs::write(&config, text).unwrap();

    for err in [
        load_workspace(root).map(|_| ()).unwrap_err(),
        migrate_workspace(root, false).map(|_| ()).unwrap_err(),
    ] {
        let chain = format!("{err}: {}", std::error::Error::source(&err).unwrap());
        assert!(chain.contains("upgrade sapphire-ledger"), "got: {chain}");
    }
}
