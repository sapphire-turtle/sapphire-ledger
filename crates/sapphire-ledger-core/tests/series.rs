use chrono::NaiveDate;
use rust_decimal::Decimal;
use sapphire_ledger_core::series::{
    BudgetLine, GroupBy, Interval, Measure, SeriesQuery, SeriesRequest, buckets, check_query,
    query_series,
};
use sapphire_ledger_core::{AccountRef, AccountType, Posting, Workspace, load_workspace, ops};

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn vals(raw: &[Option<&str>]) -> Vec<Option<Decimal>> {
    raw.iter().map(|v| v.map(|s| s.parse().unwrap())).collect()
}

fn posting(account: &str, amount: &str, currency: &str) -> Posting {
    Posting {
        account_id: None,
        account_name: Some(account.into()),
        amount: amount.parse().unwrap(),
        currency: currency.into(),
        price: None,
        memo: None,
    }
}

const TODAY: &str = "2026-10-04";

/// Salary in September; food (living), lunch (living, eatout), rent
/// (untagged) on Oct 1–3; food after "today"; a USD expense to ignore.
struct Books(tempfile::TempDir);

impl Books {
    fn new() -> Books {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sapphire_ledger_core::init_workspace(root, "JPY").unwrap();
        for (name, ty) in [
            ("Assets:Bank", AccountType::Asset),
            ("Assets:Wallet", AccountType::Asset),
            ("Liabilities:Card", AccountType::Liability),
            ("Expenses:Food", AccountType::Expense),
            ("Expenses:Food:Lunch", AccountType::Expense),
            ("Expenses:Rent", AccountType::Expense),
            ("Income:Salary", AccountType::Income),
        ] {
            ops::create_account(root, name.into(), ty, vec![], d("2026-01-01"), None, vec![])
                .unwrap();
        }
        ops::create_budget(
            root,
            "生活費".into(),
            "living".into(),
            "JPY".into(),
            "100000".parse().unwrap(),
            vec![AccountRef {
                account_id: None,
                account_name: Some("Assets:Bank".into()),
            }],
        )
        .unwrap();
        ops::create_budget(
            root,
            "Loose".into(),
            "loose".into(),
            "JPY".into(),
            "1000".parse().unwrap(),
            vec![],
        )
        .unwrap();
        let books = Books(dir);
        books.tx(
            "2026-09-30",
            &[],
            ("Assets:Bank", "300000"),
            ("Income:Salary", "-300000"),
            "JPY",
        );
        books.tx(
            "2026-10-01",
            &["living"],
            ("Expenses:Food", "1000"),
            ("Assets:Bank", "-1000"),
            "JPY",
        );
        books.tx(
            "2026-10-02",
            &["living", "eatout"],
            ("Expenses:Food:Lunch", "500"),
            ("Assets:Wallet", "-500"),
            "JPY",
        );
        books.tx(
            "2026-10-03",
            &[],
            ("Expenses:Rent", "80000"),
            ("Assets:Bank", "-80000"),
            "JPY",
        );
        books.tx(
            "2026-10-05",
            &["living"],
            ("Expenses:Food", "300"),
            ("Assets:Bank", "-300"),
            "JPY",
        );
        books.tx(
            "2026-10-02",
            &["living"],
            ("Expenses:Food", "10"),
            ("Liabilities:Card", "-10"),
            "USD",
        );
        books
    }

    fn tx(&self, date: &str, tags: &[&str], a: (&str, &str), b: (&str, &str), currency: &str) {
        ops::create_transaction(
            self.0.path(),
            d(date),
            "t".into(),
            None,
            tags.iter().map(|t| t.to_string()).collect(),
            None,
            vec![posting(a.0, a.1, currency), posting(b.0, b.1, currency)],
            None,
            false,
        )
        .unwrap();
    }

    fn ws(&self) -> Workspace {
        load_workspace(self.0.path()).unwrap()
    }
}

fn request(from: &str, to: &str, interval: Interval, series: Vec<SeriesQuery>) -> SeriesRequest {
    SeriesRequest {
        from: d(from),
        to: d(to),
        interval,
        currency: None,
        series,
    }
}

fn expenses() -> SeriesQuery {
    SeriesQuery {
        account_types: vec![AccountType::Expense],
        ..SeriesQuery::new(Measure::Flow)
    }
}

#[test]
fn flow_sums_per_day_in_the_base_currency_and_stops_at_today() {
    let ws = Books::new().ws();
    let r = query_series(
        &ws,
        &request("2026-10-01", "2026-10-07", Interval::Day, vec![expenses()]),
        d(TODAY),
    )
    .unwrap();
    assert_eq!(r.currency, "JPY");
    assert_eq!(r.buckets.len(), 7);
    assert_eq!(r.series.len(), 1);
    assert_eq!(r.series[0].label, "Flow");
    assert_eq!(r.series[0].group, None);
    assert_eq!(
        r.series[0].values,
        vals(&[
            Some("1000"),
            Some("500"),
            Some("80000"),
            Some("0"),
            None,
            None,
            None
        ])
    );
}

#[test]
fn filters_match_tags_types_and_accounts_under_a_name() {
    let books = Books::new();
    let ws = books.ws();
    let run = |q: SeriesQuery| {
        query_series(
            &ws,
            &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]),
            d(TODAY),
        )
        .map(|r| r.series[0].values.clone())
    };
    let living = SeriesQuery {
        tags: vec!["living".into()],
        ..expenses()
    };
    assert_eq!(
        run(living).unwrap(),
        vals(&[Some("1000"), Some("500"), Some("0"), Some("0")])
    );
    let food = SeriesQuery {
        accounts: vec!["Expenses:Food".into()],
        ..SeriesQuery::new(Measure::Flow)
    };
    assert_eq!(
        run(food).unwrap(),
        vals(&[Some("1000"), Some("500"), Some("0"), Some("0")])
    );
    let lunch = SeriesQuery {
        accounts: vec!["Expenses:Food:Lunch".into()],
        ..SeriesQuery::new(Measure::Flow)
    };
    assert_eq!(
        run(lunch).unwrap(),
        vals(&[Some("0"), Some("500"), Some("0"), Some("0")])
    );
    // No account is named exactly `Expenses`; the name still matches what is under it.
    let all = SeriesQuery {
        accounts: vec!["Expenses".into()],
        ..SeriesQuery::new(Measure::Flow)
    };
    assert_eq!(
        run(all).unwrap(),
        vals(&[Some("1000"), Some("500"), Some("80000"), Some("0")])
    );
    let food_id = ws
        .accounts
        .iter()
        .find(|a| a.name == "Expenses:Food")
        .unwrap()
        .id
        .clone();
    let by_id = SeriesQuery {
        accounts: vec![food_id],
        ..SeriesQuery::new(Measure::Flow)
    };
    assert_eq!(
        run(by_id).unwrap(),
        vals(&[Some("1000"), Some("500"), Some("0"), Some("0")])
    );
    let nope = SeriesQuery {
        accounts: vec!["Nope".into()],
        ..SeriesQuery::new(Measure::Flow)
    };
    assert!(
        run(nope)
            .unwrap_err()
            .to_string()
            .contains("no account matches Nope")
    );
}

#[test]
fn balance_starts_from_everything_before_the_range() {
    let ws = Books::new().ws();
    let bank = SeriesQuery {
        accounts: vec!["Assets:Bank".into()],
        ..SeriesQuery::new(Measure::Balance)
    };
    let assets = SeriesQuery {
        account_types: vec![AccountType::Asset, AccountType::Liability],
        label: Some("Net worth".into()),
        ..SeriesQuery::new(Measure::Balance)
    };
    let r = query_series(
        &ws,
        &request(
            "2026-10-01",
            "2026-10-05",
            Interval::Day,
            vec![bank, assets],
        ),
        d(TODAY),
    )
    .unwrap();
    assert_eq!(
        r.series[0].values,
        vals(&[
            Some("299000"),
            Some("299000"),
            Some("219000"),
            Some("219000"),
            None
        ])
    );
    // The USD card posting is not in JPY, so it is not counted.
    assert_eq!(
        r.series[1].values,
        vals(&[
            Some("299000"),
            Some("298500"),
            Some("218500"),
            Some("218500"),
            None
        ])
    );
    assert_eq!(r.series[1].label, "Net worth");
    assert_eq!(r.series[1].index, 1);
}

#[test]
fn group_by_account_at_a_depth_orders_by_name() {
    let ws = Books::new().ws();
    let q = SeriesQuery {
        group_by: Some(GroupBy::Account),
        depth: Some(2),
        label: Some("Spent".into()),
        ..expenses()
    };
    let r = query_series(
        &ws,
        &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]),
        d(TODAY),
    )
    .unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["Expenses:Food", "Expenses:Rent"]);
    assert_eq!(r.series[0].label, "Spent — Expenses:Food");
    assert_eq!(
        r.series[0].values,
        vals(&[Some("1000"), Some("500"), Some("0"), Some("0")])
    );
    assert_eq!(
        r.series[1].values,
        vals(&[Some("0"), Some("0"), Some("80000"), Some("0")])
    );
}

#[test]
fn group_by_tag_counts_each_tag_and_the_untagged() {
    let ws = Books::new().ws();
    let q = SeriesQuery {
        group_by: Some(GroupBy::Tag),
        cumulative: true,
        ..expenses()
    };
    let r = query_series(
        &ws,
        &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]),
        d(TODAY),
    )
    .unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["(untagged)", "eatout", "living"]);
    assert_eq!(
        r.series[0].values,
        vals(&[Some("0"), Some("0"), Some("80000"), Some("80000")])
    );
    assert_eq!(
        r.series[1].values,
        vals(&[Some("0"), Some("500"), Some("500"), Some("500")])
    );
    assert_eq!(
        r.series[2].values,
        vals(&[Some("1000"), Some("1500"), Some("1500"), Some("1500")])
    );
    assert_eq!(r.series[2].label, "living");

    // Filtered on `living`, the lunch's other tag makes no group of its own.
    let q = SeriesQuery {
        group_by: Some(GroupBy::Tag),
        tags: vec!["living".into()],
        ..expenses()
    };
    let r = query_series(
        &ws,
        &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]),
        d(TODAY),
    )
    .unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["living"]);
}

#[test]
fn top_keeps_the_largest_and_folds_the_rest_into_other() {
    let ws = Books::new().ws();
    let q = SeriesQuery {
        group_by: Some(GroupBy::Account),
        top: Some(1),
        ..expenses()
    };
    let r = query_series(
        &ws,
        &request("2026-10-01", "2026-10-04", Interval::Day, vec![q]),
        d(TODAY),
    )
    .unwrap();
    let groups: Vec<_> = r.series.iter().map(|s| s.group.clone().unwrap()).collect();
    assert_eq!(groups, ["Expenses:Rent", "Other"]);
    assert_eq!(
        r.series[1].values,
        vals(&[Some("1000"), Some("500"), Some("0"), Some("0")])
    );
}

#[test]
fn group_by_account_type_and_negate() {
    let ws = Books::new().ws();
    let income = SeriesQuery {
        account_types: vec![AccountType::Income],
        negate: true,
        label: Some("Income".into()),
        ..SeriesQuery::new(Measure::Flow)
    };
    let by_type = SeriesQuery {
        group_by: Some(GroupBy::AccountType),
        ..SeriesQuery::new(Measure::Flow)
    };
    let r = query_series(
        &ws,
        &request(
            "2026-09-01",
            "2026-10-31",
            Interval::Month,
            vec![income, by_type],
        ),
        d(TODAY),
    )
    .unwrap();
    assert_eq!(r.buckets, [d("2026-09-01"), d("2026-10-01")]);
    assert_eq!(r.series[0].values, vals(&[Some("300000"), Some("0")]));
    let groups: Vec<_> = r.series[1..]
        .iter()
        .map(|s| s.group.clone().unwrap())
        .collect();
    assert_eq!(groups, ["Asset", "Expense", "Income"]);
}

#[test]
fn budget_pace_follows_each_months_length() {
    let ws = Books::new().ws();
    let pace = SeriesQuery {
        budget: Some("生活費".into()),
        ..SeriesQuery::new(Measure::Budget)
    };
    let r = query_series(
        &ws,
        &request(
            "2026-01-01",
            "2026-03-31",
            Interval::Month,
            vec![pace.clone()],
        ),
        d(TODAY),
    )
    .unwrap();
    assert_eq!(
        r.series[0].values,
        vals(&[Some("100000"), Some("100000"), Some("100000")])
    );
    assert_eq!(r.series[0].label, "生活費 pace");
    let cumulative = SeriesQuery {
        cumulative: true,
        ..pace.clone()
    };
    let r = query_series(
        &ws,
        &request(
            "2026-02-01",
            "2026-02-28",
            Interval::Day,
            vec![pace, cumulative],
        ),
        d(TODAY),
    )
    .unwrap();
    assert_eq!(r.series[0].values[0], Some("3571.43".parse().unwrap()));
    assert_eq!(r.series[1].values[27], Some("100000".parse().unwrap()));
    // Budget lines are drawn past today.
    assert!(r.series[0].values.iter().all(Option::is_some));
}

#[test]
fn budget_remaining_starts_from_the_balance_accounts() {
    let ws = Books::new().ws();
    let remaining = SeriesQuery {
        budget: Some("生活費".into()),
        line: Some(BudgetLine::Remaining),
        ..SeriesQuery::new(Measure::Budget)
    };
    let r = query_series(
        &ws,
        &request("2026-10-01", "2026-10-03", Interval::Day, vec![remaining]),
        d(TODAY),
    )
    .unwrap();
    assert_eq!(
        r.series[0].values,
        vals(&[Some("296774.19"), Some("293548.39"), Some("290322.58")])
    );
    assert_eq!(r.series[0].label, "生活費 remaining");

    let loose = SeriesQuery {
        budget: Some("Loose".into()),
        line: Some(BudgetLine::Remaining),
        ..SeriesQuery::new(Measure::Budget)
    };
    let err = query_series(
        &ws,
        &request("2026-10-01", "2026-10-03", Interval::Day, vec![loose]),
        d(TODAY),
    )
    .unwrap_err();
    assert!(err.to_string().contains("no balance accounts"), "{err}");

    let usd = SeriesRequest {
        currency: Some("USD".into()),
        ..request(
            "2026-10-01",
            "2026-10-03",
            Interval::Day,
            vec![SeriesQuery {
                budget: Some("生活費".into()),
                ..SeriesQuery::new(Measure::Budget)
            }],
        )
    };
    let err = query_series(&ws, &usd, d(TODAY)).unwrap_err();
    assert!(err.to_string().contains("is in JPY"), "{err}");

    let missing = SeriesQuery {
        budget: Some("nope".into()),
        ..SeriesQuery::new(Measure::Budget)
    };
    let err = query_series(
        &ws,
        &request("2026-10-01", "2026-10-03", Interval::Day, vec![missing]),
        d(TODAY),
    )
    .unwrap_err();
    assert!(err.to_string().contains("no budget nope"), "{err}");
}

#[test]
fn a_currency_other_than_the_base_reads_only_that_currency() {
    let ws = Books::new().ws();
    let usd = SeriesRequest {
        currency: Some("USD".into()),
        ..request("2026-10-01", "2026-10-03", Interval::Day, vec![expenses()])
    };
    let r = query_series(&ws, &usd, d(TODAY)).unwrap();
    assert_eq!(r.currency, "USD");
    assert_eq!(
        r.series[0].values,
        vals(&[Some("0"), Some("10"), Some("0")])
    );
}

#[test]
fn week_buckets_start_on_monday_and_are_clipped_to_the_range() {
    // 2026-10-01 is a Thursday.
    assert_eq!(
        buckets(d("2026-10-01"), d("2026-10-14"), Interval::Week).unwrap(),
        [d("2026-10-01"), d("2026-10-05"), d("2026-10-12")]
    );
    assert_eq!(
        buckets(d("2026-11-15"), d("2027-02-01"), Interval::Year).unwrap(),
        [d("2026-11-15"), d("2027-01-01")]
    );
    assert!(buckets(d("2026-10-02"), d("2026-10-01"), Interval::Day).is_err());
}

#[test]
fn the_limits_are_errors() {
    let ws = Books::new().ws();
    let err = query_series(
        &ws,
        &request("2020-01-01", "2026-01-01", Interval::Day, vec![expenses()]),
        d(TODAY),
    )
    .unwrap_err();
    assert!(err.to_string().contains("1000"), "{err}");
    let many = vec![expenses(); 51];
    let err = query_series(
        &ws,
        &request("2026-10-01", "2026-10-02", Interval::Day, many),
        d(TODAY),
    )
    .unwrap_err();
    assert!(err.to_string().contains("50"), "{err}");
    let err = query_series(
        &ws,
        &request("2026-10-01", "2026-10-02", Interval::Day, vec![]),
        d(TODAY),
    )
    .unwrap_err();
    assert!(err.to_string().contains("at least one series"), "{err}");
}

#[test]
fn a_field_that_does_not_apply_is_an_error() {
    let budget = || SeriesQuery {
        budget: Some("b".into()),
        ..SeriesQuery::new(Measure::Budget)
    };
    let msg = |q: SeriesQuery| check_query(&q).unwrap_err().to_string();
    assert!(
        msg(SeriesQuery {
            tags: vec!["x".into()],
            ..budget()
        })
        .contains("`tags` does not apply to a budget series")
    );
    assert!(msg(SeriesQuery::new(Measure::Budget)).contains("needs `budget`"));
    assert!(
        msg(SeriesQuery {
            line: Some(BudgetLine::Remaining),
            cumulative: true,
            ..budget()
        })
        .contains("always cumulative")
    );
    assert!(
        msg(SeriesQuery {
            budget: Some("b".into()),
            ..expenses()
        })
        .contains("`budget` does not apply to a flow series")
    );
    assert!(
        msg(SeriesQuery {
            cumulative: true,
            ..SeriesQuery::new(Measure::Balance)
        })
        .contains("already cumulative")
    );
    assert!(
        msg(SeriesQuery {
            depth: Some(2),
            ..expenses()
        })
        .contains("`depth` needs group_by = account")
    );
    assert!(
        msg(SeriesQuery {
            top: Some(2),
            ..expenses()
        })
        .contains("`top` needs a group_by")
    );
    assert!(
        msg(SeriesQuery {
            group_by: Some(GroupBy::Account),
            depth: Some(0),
            ..expenses()
        })
        .contains("start at 1")
    );
    assert!(
        check_query(&SeriesQuery {
            group_by: Some(GroupBy::Account),
            depth: Some(2),
            top: Some(7),
            ..expenses()
        })
        .is_ok()
    );
}
