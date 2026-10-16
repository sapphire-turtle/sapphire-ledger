use rust_decimal::Decimal;
use sapphire_ledger_core::{
    Account, AccountType, Balance, Posting, Transaction, load_markdown, ops,
};

fn ws() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    sapphire_ledger_core::init_workspace(dir.path(), "JPY").expect("init");
    dir
}

fn add_account(root: &std::path::Path, name: &str, ty: AccountType) -> String {
    let (id, _) = ops::create_account(
        root,
        name.to_string(),
        ty,
        vec![],
        "2026-01-01".parse().unwrap(),
        None,
        vec![],
    )
    .expect("create_account");
    id
}

fn posting_by_name(name: &str, amount: &str) -> Posting {
    Posting {
        account_id: None,
        account_name: Some(name.to_string()),
        amount: amount.parse::<Decimal>().unwrap(),
        currency: "JPY".into(),
        price: None,
        memo: None,
    }
}

#[test]
fn create_account_writes_the_hierarchical_path_and_mints_an_id() {
    let dir = ws();
    let (id, dest) = ops::create_account(
        dir.path(),
        "Assets:Cash:JPY".into(),
        AccountType::Asset,
        vec![],
        "2026-01-01".parse().unwrap(),
        None,
        vec![],
    )
    .expect("create");
    assert_eq!(id.chars().count(), 7);
    assert!(
        dest.ends_with("accounts/Assets/Cash/JPY.md"),
        "got {}",
        dest.display()
    );
    assert!(std::fs::read_to_string(&dest).unwrap().contains(&id));
}

#[test]
fn create_account_refuses_a_duplicate_name() {
    let dir = ws();
    add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);
    let err = ops::create_account(
        dir.path(),
        "Assets:Cash:JPY".into(),
        AccountType::Asset,
        vec![],
        "2026-01-01".parse().unwrap(),
        None,
        vec![],
    )
    .expect_err("must refuse");
    assert!(err.to_string().contains("already exists"), "got: {err}");
}

#[test]
fn create_account_rejects_a_malformed_name() {
    let dir = ws();
    let err = ops::create_account(
        dir.path(),
        "Assets::Cash".into(),
        AccountType::Asset,
        vec![],
        "2026-01-01".parse().unwrap(),
        None,
        vec![],
    )
    .expect_err("must reject");
    assert!(err.to_string().contains("empty segment"), "got: {err}");
}

#[test]
fn create_transaction_resolves_names_to_ids_on_disk() {
    let dir = ws();
    let food = add_account(dir.path(), "Expenses:Food", AccountType::Expense);
    add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);

    let (id, dest) = ops::create_transaction(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "イオン買い物".into(),
        Some("イオン".into()),
        vec!["grocery".into()],
        None,
        vec![
            posting_by_name("Expenses:Food", "1200"),
            posting_by_name("Assets:Cash:JPY", "-1200"),
        ],
        None,
        true,
    )
    .expect("create");

    assert!(
        dest.ends_with(format!("transactions/2026/05/{id}.md")),
        "got {}",
        dest.display()
    );
    let written: sapphire_ledger_core::Transaction =
        sapphire_ledger_core::load_markdown(&dest).expect("read back");
    assert_eq!(
        written.postings[0].account_id.as_deref(),
        Some(food.as_str()),
        "a name-only posting must be written with its resolved id"
    );
    assert_eq!(
        written.postings[0].account_name.as_deref(),
        Some("Expenses:Food")
    );
}

#[test]
fn create_transaction_rejects_an_unknown_account() {
    let dir = ws();
    add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);
    let err = ops::create_transaction(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "orphan".into(),
        None,
        vec![],
        None,
        vec![
            posting_by_name("Expenses:Nowhere", "1200"),
            posting_by_name("Assets:Cash:JPY", "-1200"),
        ],
        None,
        true,
    )
    .expect_err("must reject");
    assert!(err.to_string().contains("undefined account"), "got: {err}");
}

#[test]
fn create_transaction_rejects_an_unbalanced_entry_and_writes_nothing() {
    let dir = ws();
    add_account(dir.path(), "Expenses:Food", AccountType::Expense);
    add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);

    let err = ops::create_transaction(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "wrong".into(),
        None,
        vec![],
        None,
        vec![
            posting_by_name("Expenses:Food", "1200"),
            posting_by_name("Assets:Cash:JPY", "-999"),
        ],
        None,
        true,
    )
    .expect_err("must reject");
    assert!(err.to_string().contains("does not balance"), "got: {err}");

    let month = dir.path().join("transactions/2026/05");
    assert_eq!(
        std::fs::read_dir(&month).map(|d| d.count()).unwrap_or(0),
        0,
        "a rejected transaction must leave no file behind"
    );
}

#[test]
fn create_assertion_and_price_produce_dated_paths() {
    let dir = ws();
    let cash = add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);

    let (aid, apath) = ops::create_assertion(
        dir.path(),
        Some(cash),
        None,
        "2026-05-31".parse().unwrap(),
        vec![Balance {
            amount: "5000".parse().unwrap(),
            currency: "JPY".into(),
        }],
    )
    .expect("assertion");
    assert!(
        apath.ends_with(format!("assertions/2026/05/{aid}.toml")),
        "got {}",
        apath.display()
    );

    let (pid, ppath) = ops::create_price(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "USD".into(),
        "JPY".into(),
        "150".parse().unwrap(),
        Some("manual".into()),
    )
    .expect("price");
    assert!(
        ppath.ends_with(format!("prices/2026/05/{pid}.toml")),
        "got {}",
        ppath.display()
    );
}

#[test]
fn created_records_reload_and_validate_clean() {
    let dir = ws();
    add_account(dir.path(), "Expenses:Food", AccountType::Expense);
    add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);
    ops::create_transaction(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "イオン買い物".into(),
        None,
        vec![],
        None,
        vec![
            posting_by_name("Expenses:Food", "1200"),
            posting_by_name("Assets:Cash:JPY", "-1200"),
        ],
        None,
        true,
    )
    .expect("create");

    let loaded = sapphire_ledger_core::load_workspace(dir.path()).expect("load");
    assert_eq!(loaded.accounts.len(), 2);
    assert_eq!(loaded.transactions.len(), 1);
    assert!(
        loaded.validate().is_empty(),
        "issues: {:?}",
        loaded.validate()
    );
}

#[test]
fn create_transaction_twice_in_the_same_decisecond_mints_distinct_ids() {
    let dir = ws();
    add_account(dir.path(), "Expenses:Food", AccountType::Expense);
    add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);

    let (id_a, _) = ops::create_transaction(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "first".into(),
        None,
        vec![],
        None,
        vec![
            posting_by_name("Expenses:Food", "1200"),
            posting_by_name("Assets:Cash:JPY", "-1200"),
        ],
        None,
        true,
    )
    .expect("create first");

    let (id_b, _) = ops::create_transaction(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "second".into(),
        None,
        vec![],
        None,
        vec![
            posting_by_name("Expenses:Food", "500"),
            posting_by_name("Assets:Cash:JPY", "-500"),
        ],
        None,
        true,
    )
    .expect("create second");

    assert_ne!(
        id_a, id_b,
        "two records minted back to back must not collide"
    );
}

#[test]
fn create_transaction_rejects_a_currency_the_account_does_not_allow() {
    let dir = ws();
    ops::create_account(
        dir.path(),
        "Assets:Cash:JPY".into(),
        AccountType::Asset,
        vec!["JPY".into()],
        "2026-01-01".parse().unwrap(),
        None,
        vec![],
    )
    .expect("create_account");
    add_account(dir.path(), "Expenses:Food", AccountType::Expense);

    let err = ops::create_transaction(
        dir.path(),
        "2026-05-21".parse().unwrap(),
        "wrong currency".into(),
        None,
        vec![],
        None,
        vec![
            Posting {
                account_id: None,
                account_name: Some("Assets:Cash:JPY".into()),
                amount: "-1000".parse().unwrap(),
                currency: "USD".into(),
                price: None,
                memo: None,
            },
            posting_by_name("Expenses:Food", "1000"),
        ],
        None,
        true,
    )
    .expect_err("must reject");
    assert!(err.to_string().contains("only allows"), "got: {err}");
}

#[test]
fn create_assertion_rejects_a_currency_the_account_does_not_allow() {
    let dir = ws();
    ops::create_account(
        dir.path(),
        "Assets:Cash:JPY".into(),
        AccountType::Asset,
        vec!["JPY".into()],
        "2026-01-01".parse().unwrap(),
        None,
        vec![],
    )
    .expect("create_account");

    let err = ops::create_assertion(
        dir.path(),
        None,
        Some("Assets:Cash:JPY".into()),
        "2026-05-31".parse().unwrap(),
        vec![Balance {
            amount: "100".parse().unwrap(),
            currency: "USD".into(),
        }],
    )
    .expect_err("must reject");
    assert!(err.to_string().contains("only allows"), "got: {err}");
}

/// Ids must be unique per *record kind*, not per month directory.
///
/// `new_id()` is a pure function of the current decisecond, so two records
/// minted inside one decisecond ask for the same id. When their dates fall in
/// different months their destination paths differ, so a per-path collision
/// check sees nothing and both writes land with the same id.
///
/// The clock is not controllable, so each round brackets its two writes with
/// `new_id()` and only asserts on rounds that provably stayed inside one
/// decisecond. That makes the test deterministic rather than timing-dependent.
#[test]
fn transactions_dated_in_different_months_never_share_an_id() {
    let dir = ws();
    add_account(dir.path(), "Expenses:Food", AccountType::Expense);
    add_account(dir.path(), "Assets:Cash:JPY", AccountType::Asset);

    let mut sampled = 0usize;
    for round in 0..40 {
        let before = ops::new_id();
        let (id_may, _) = ops::create_transaction(
            dir.path(),
            "2026-05-21".parse().unwrap(),
            format!("may {round}"),
            None,
            vec![],
            None,
            vec![
                posting_by_name("Expenses:Food", "1200"),
                posting_by_name("Assets:Cash:JPY", "-1200"),
            ],
            None,
            true,
        )
        .expect("create may");
        let (id_june, _) = ops::create_transaction(
            dir.path(),
            "2026-06-03".parse().unwrap(),
            format!("june {round}"),
            None,
            vec![],
            None,
            vec![
                posting_by_name("Expenses:Food", "500"),
                posting_by_name("Assets:Cash:JPY", "-500"),
            ],
            None,
            true,
        )
        .expect("create june");
        let after = ops::new_id();

        // Both writes provably happened inside a single decisecond, so both
        // first attempts asked `new_id()` for the same string.
        if before == after {
            sampled += 1;
            assert_ne!(
                id_may, id_june,
                "two transactions minted in one decisecond took the same id \
                 because their months differ (round {round})"
            );
        }
    }
    assert!(
        sampled > 0,
        "no round stayed inside one decisecond; the test never exercised the collision window"
    );
}

/// `create_account`'s contract is "a duplicate name is a hard error", and the
/// refuse-to-overwrite check only delivers that while every account file sits
/// at the path its name implies. A hand edit, a half-applied rename or a sync
/// artifact breaks that assumption, and then the name check has to come from
/// the accounts already loaded -- which `create_account` holds anyway for its
/// id-uniqueness check.
#[test]
fn create_account_refuses_a_duplicate_name_parked_at_another_path() {
    let dir = ws();
    let stray = dir.path().join("accounts/Assets/Bar.md");
    std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
    std::fs::write(
        &stray,
        "---\nid: acct999\nname: Assets:Foo\ntype: Asset\nopened_at: 2026-01-01\n---\n",
    )
    .unwrap();

    let err = ops::create_account(
        dir.path(),
        "Assets:Foo".into(),
        AccountType::Asset,
        vec![],
        "2026-01-01".parse().unwrap(),
        None,
        vec![],
    )
    .expect_err("a second account named Assets:Foo must be refused");
    let msg = err.to_string();
    assert!(
        msg.contains("Assets:Foo") && msg.contains("already"),
        "unhelpful error: {msg}"
    );
    assert!(
        !dir.path().join("accounts/Assets/Foo.toml").exists(),
        "the duplicate was written anyway"
    );
}

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
