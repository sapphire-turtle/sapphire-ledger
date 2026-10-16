use rust_decimal::Decimal;
use sapphire_ledger_core::report::{budget_report, parse_month, round_amount};
use sapphire_ledger_core::{
    AccountRef, AccountType, Posting, Price, Workspace, load_workspace, ops,
};

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
            ops::create_account(root, name.into(), ty, vec![], d("2026-01-01"), None, vec![])
                .unwrap();
        }
        let refs = balance_accounts
            .iter()
            .map(|n| AccountRef {
                account_id: None,
                account_name: Some((*n).into()),
            })
            .collect();
        ops::create_budget(
            root,
            "生活費".into(),
            "living".into(),
            "JPY".into(),
            dec("100000"),
            refs,
        )
        .unwrap();
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
        self.tx(
            date,
            tags,
            vec![
                posting("Expenses:Food", amount, "JPY"),
                posting("Assets:Bank", &negative, "JPY"),
            ],
        );
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
    assert_eq!(
        round_amount(dec("100000") / dec("31")).to_string(),
        "3225.81"
    );
    assert_eq!(round_amount(dec("100000.00")).to_string(), "100000");
    assert_eq!(round_amount(dec("0.005")).to_string(), "0.01");
}

#[test]
fn a_31_day_month_spreads_the_budget_and_counts_tagged_spending() {
    let books = Books::new(&["Assets:Bank"]);
    books.tx(
        "2026-04-25",
        &[],
        vec![
            posting("Assets:Bank", "300000", "JPY"),
            posting("Income:Salary", "-300000", "JPY"),
        ],
    );
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
            Posting {
                price: Some(Price {
                    value: dec("150"),
                    currency: "JPY".into(),
                }),
                ..posting("Expenses:Food", "10", "USD")
            },
            Posting {
                price: Some(Price {
                    value: dec("150"),
                    currency: "JPY".into(),
                }),
                ..posting("Assets:Wallet", "-10", "USD")
            },
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
    ws.budgets[0].balance_accounts.push(AccountRef {
        account_id: Some("zzzzzzz".into()),
        account_name: None,
    });
    let r = budget_report(&ws, &ws.budgets[0], 2026, 5, d("2026-10-16")).unwrap();
    assert_eq!(r.days[0].actual_balance, Some(dec("0")));
}

#[test]
fn a_month_at_the_end_of_the_calendar_is_an_error_not_a_panic() {
    let books = Books::new(&[]);
    let ws = books.load();
    let r = budget_report(&ws, &ws.budgets[0], 262142, 12, d("2026-10-16"));
    assert!(r.is_err());
}
