use rust_decimal::Decimal;
use sapphire_ledger_core::ops::{self, BudgetChanges};
use sapphire_ledger_core::{AccountRef, AccountType, Budget, load_toml, load_workspace};

fn ws() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    sapphire_ledger_core::init_workspace(dir.path(), "JPY").expect("init");
    for (name, ty) in [
        ("Assets:Bank", AccountType::Asset),
        ("Expenses:Food", AccountType::Expense),
    ] {
        ops::create_account(
            dir.path(),
            name.into(),
            ty,
            vec![],
            "2026-01-01".parse().unwrap(),
            None,
            vec![],
        )
        .expect("account");
    }
    dir
}

fn by_name(name: &str) -> AccountRef {
    AccountRef {
        account_id: None,
        account_name: Some(name.into()),
    }
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
        ops::create_budget(
            root,
            name.into(),
            tag.into(),
            "JPY".into(),
            Decimal::from(amount),
            vec![by_name(account)],
        )
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
        BudgetChanges {
            monthly_amount: Some(Decimal::from(120_000)),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((got, at), (id.clone(), dest.clone()));
    let after: Budget = load_toml(&dest).unwrap();
    assert_eq!(after.monthly_amount, Decimal::from(120_000));
    assert_eq!(after.tag, "living");
    assert_eq!(after.created_at, before.created_at);
    assert!(after.updated_at >= before.updated_at);

    ops::update_budget(
        dir.path(),
        &id,
        BudgetChanges {
            balance_accounts: Some(vec![]),
            ..Default::default()
        },
    )
    .unwrap();
    let after: Budget = load_toml(&dest).unwrap();
    assert!(after.balance_accounts.is_empty());
}

#[test]
fn update_budget_refuses_a_name_already_taken() {
    let dir = ws();
    living(dir.path());
    ops::create_budget(
        dir.path(),
        "旅行".into(),
        "trip".into(),
        "JPY".into(),
        Decimal::from(50_000),
        vec![],
    )
    .unwrap();
    let err = ops::update_budget(
        dir.path(),
        "旅行",
        BudgetChanges {
            name: Some("生活費".into()),
            ..Default::default()
        },
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
    std::fs::write(
        budgets.join("aaaaaaa.toml"),
        record("aaaaaaa", "a", "100", "Assets:Gone"),
    )
    .unwrap();
    std::fs::write(
        budgets.join("bbbbbbb.toml"),
        record("bbbbbbb", "a", "0", "Assets:Bank"),
    )
    .unwrap();
    let messages: Vec<String> = load_workspace(dir.path())
        .unwrap()
        .validate()
        .into_iter()
        .map(|i| i.message)
        .collect();
    let all = messages.join("\n");
    assert!(all.contains("Assets:Gone"), "{all}");
    assert!(all.contains("duplicate budget name a"), "{all}");
    assert!(all.contains("not positive"), "{all}");
}
