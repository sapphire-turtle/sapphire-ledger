//! The write commands, driven through the real binary.
//!
//! At this level because that is what a person actually runs, and because the
//! point of these commands is that they are thin: the rules live in
//! `core::ops`, and what is worth testing here is that the CLI hands `ops` the
//! right things and reports what came back.

use std::path::Path;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_sapphire-ledger")
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .arg("--ledger-dir")
        .arg(root)
        .args(args)
        .output()
        .expect("run")
}

fn ok(root: &Path, args: &[&str]) -> String {
    let out = run(root, args);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn ledger() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = Command::new(bin())
        .arg("init")
        .arg(dir.path())
        .output()
        .expect("init");
    assert!(out.status.success(), "init failed");
    dir
}

/// Two accounts and one balanced transaction between them.
fn seeded() -> tempfile::TempDir {
    let dir = ledger();
    let root = dir.path();
    ok(
        root,
        &["account", "add", "Expenses:Food", "--type", "Expense"],
    );
    ok(root, &["account", "add", "Assets:Cash", "--type", "Asset"]);
    ok(
        root,
        &[
            "tx",
            "add",
            "--date",
            "2026-05-21",
            "--narration",
            "groceries",
            "--posting",
            "Expenses:Food 1200 JPY",
            "--posting",
            "Assets:Cash -1200 JPY",
        ],
    );
    dir
}

#[test]
fn a_ledger_written_entirely_from_the_cli_passes_check() {
    let dir = seeded();
    let out = ok(dir.path(), &["check"]);
    assert!(out.contains("OK"), "check should be clean, got: {out}");
    assert!(
        out.contains("2 account"),
        "check should see both accounts, got: {out}"
    );
}

#[test]
fn tx_add_reports_the_id_and_the_path() {
    let dir = ledger();
    let root = dir.path();
    ok(
        root,
        &["account", "add", "Expenses:Food", "--type", "Expense"],
    );
    ok(root, &["account", "add", "Assets:Cash", "--type", "Asset"]);

    let out = ok(
        root,
        &[
            "tx",
            "add",
            "--date",
            "2026-05-21",
            "--narration",
            "groceries",
            "--posting",
            "Expenses:Food 1200 JPY",
            "--posting",
            "Assets:Cash -1200 JPY",
        ],
    );

    assert!(out.contains("created transaction"), "got: {out}");
    assert!(
        out.contains("transactions"),
        "the path is how you find the file you just wrote, got: {out}"
    );
}

#[test]
fn an_unbalanced_transaction_is_refused_and_writes_nothing() {
    let dir = ledger();
    let root = dir.path();
    ok(
        root,
        &["account", "add", "Expenses:Food", "--type", "Expense"],
    );
    ok(root, &["account", "add", "Assets:Cash", "--type", "Asset"]);

    let out = run(
        root,
        &[
            "tx",
            "add",
            "--date",
            "2026-05-21",
            "--narration",
            "wrong",
            "--posting",
            "Expenses:Food 1200 JPY",
            "--posting",
            "Assets:Cash -999 JPY",
        ],
    );

    assert!(!out.status.success(), "an unbalanced transaction must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("does not balance"), "got: {err}");

    let month = root.join("transactions/2026/05");
    assert_eq!(
        std::fs::read_dir(&month).map(|d| d.count()).unwrap_or(0),
        0,
        "a rejected transaction must leave no file behind"
    );
}

#[test]
fn a_transaction_naming_an_unknown_account_is_refused() {
    let dir = ledger();
    let root = dir.path();
    ok(root, &["account", "add", "Assets:Cash", "--type", "Asset"]);

    let out = run(
        root,
        &[
            "tx",
            "add",
            "--narration",
            "orphan",
            "--posting",
            "Expenses:Nowhere 10 JPY",
            "--posting",
            "Assets:Cash -10 JPY",
        ],
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("undefined account"),
        "got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn account_add_refuses_a_duplicate_name() {
    let dir = ledger();
    let root = dir.path();
    ok(root, &["account", "add", "Assets:Cash", "--type", "Asset"]);

    let out = run(root, &["account", "add", "Assets:Cash", "--type", "Asset"]);
    assert!(!out.status.success(), "a duplicate name must be refused");
}

#[test]
fn account_add_rejects_an_unknown_type() {
    let dir = ledger();
    let out = run(
        dir.path(),
        &["account", "add", "Assets:Cash", "--type", "Bogus"],
    );
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Bogus"),
        "the error should name the bad input"
    );
}

#[test]
fn assertion_and_price_add_land_and_check_stays_clean() {
    let dir = seeded();
    let root = dir.path();

    ok(
        root,
        &[
            "assertion",
            "add",
            "Assets:Cash",
            "--date",
            "2026-05-31",
            "--balance",
            "-1200 JPY",
        ],
    );
    ok(
        root,
        &[
            "price",
            "add",
            "--date",
            "2026-05-21",
            "--base",
            "USD",
            "--quote",
            "JPY",
            "--rate",
            "150",
        ],
    );

    let out = ok(root, &["check"]);
    assert!(out.contains("OK"), "got: {out}");
    assert!(out.contains("1 assertion"), "got: {out}");
}

#[test]
fn tx_list_shows_what_was_written_and_filters_by_account() {
    let dir = seeded();
    let root = dir.path();

    let all = ok(root, &["tx", "list"]);
    assert!(all.contains("groceries"), "got: {all}");

    let filtered = ok(root, &["tx", "list", "--account", "Expenses:Food"]);
    assert!(filtered.contains("groceries"), "got: {filtered}");

    let other = ok(root, &["tx", "list", "--account", "Assets:Cash"]);
    assert!(other.contains("groceries"), "got: {other}");

    let none = ok(root, &["tx", "list", "--date-from", "2027-01-01"]);
    assert!(
        !none.contains("groceries"),
        "a date filter past the record must exclude it, got: {none}"
    );
}

#[test]
fn a_cross_currency_posting_balances_through_its_inline_price() {
    let dir = ledger();
    let root = dir.path();
    ok(
        root,
        &["account", "add", "Assets:Cash:USD", "--type", "Asset"],
    );
    ok(
        root,
        &["account", "add", "Assets:Cash:JPY", "--type", "Asset"],
    );

    ok(
        root,
        &[
            "tx",
            "add",
            "--date",
            "2026-05-21",
            "--narration",
            "bought dollars",
            "--posting",
            "Assets:Cash:USD 100 USD @ 150 JPY",
            "--posting",
            "Assets:Cash:JPY -15000 JPY",
        ],
    );

    let out = ok(root, &["check"]);
    assert!(out.contains("OK"), "got: {out}");
}

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
            "tx",
            "add",
            "--date",
            "2026-05-22",
            "--narration",
            "veg",
            "--tag",
            "grocery",
            "--posting",
            "Expenses:Food 500 JPY",
            "--posting",
            "Assets:Cash -500 JPY",
        ],
    );

    let list = ok(root, &["panel", "list"]);
    assert!(
        list.contains("pan0001  Living pace  (month)"),
        "got: {list}"
    );

    let out = ok(
        root,
        &["panel", "run", "Living pace", "--anchor", "2026-05-10"],
    );
    assert!(
        out.starts_with("Living pace · 2026-05-01 – 2026-05-31 · JPY"),
        "got: {out}"
    );
    assert!(out.contains("== Cumulative =="), "got: {out}");
    let line = out
        .lines()
        .find(|l| l.starts_with("2026-05-22"))
        .expect("a row per day");
    let cols: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(cols, ["2026-05-22", "500", "22000"], "got: {line}");
    assert_eq!(
        out.lines().filter(|l| l.starts_with("2026-05-")).count(),
        31
    );

    let prev = ok(
        root,
        &[
            "panel",
            "run",
            "pan0001",
            "--anchor",
            "2026-06-10",
            "--offset",
            "-1",
        ],
    );
    assert!(prev.contains("2026-05-01 – 2026-05-31"), "got: {prev}");

    let err = run(root, &["panel", "run", "nope"]);
    assert!(!err.status.success());
    assert!(String::from_utf8_lossy(&err.stderr).contains("no panel nope"));
    let gone = run(root, &["budget", "report", "living"]);
    assert!(!gone.status.success(), "`budget report` is gone");
}
