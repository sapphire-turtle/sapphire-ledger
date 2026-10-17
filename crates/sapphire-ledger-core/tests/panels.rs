use chrono::NaiveDate;
use sapphire_ledger_core::panel::{Mark, PanelSeries, Period, Plot, panel_range, run_panel};
use sapphire_ledger_core::series::{BudgetLine, Measure, SeriesQuery};
use sapphire_ledger_core::{AccountRef, AccountType, PanelDefinition, load_workspace, ops};

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn ledger() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    sapphire_ledger_core::init_workspace(dir.path(), "JPY").unwrap();
    for (name, ty) in [
        ("Assets:Bank", AccountType::Asset),
        ("Expenses:Food", AccountType::Expense),
    ] {
        ops::create_account(
            dir.path(),
            name.into(),
            ty,
            vec![],
            d("2026-01-01"),
            None,
            vec![],
        )
        .unwrap();
    }
    dir
}

fn spent() -> PanelSeries {
    PanelSeries {
        query: SeriesQuery {
            account_types: vec![AccountType::Expense],
            label: Some("Spent".into()),
            ..SeriesQuery::new(Measure::Flow)
        },
        mark: Some(Mark::Bar),
    }
}

fn definition(name: &str) -> PanelDefinition {
    PanelDefinition {
        name: name.into(),
        period: Period::Month,
        interval: None,
        currency: None,
        plots: vec![Plot {
            title: Some("Daily".into()),
            stacked: false,
            series: vec![spent()],
        }],
    }
}

#[test]
fn a_period_and_offset_name_a_range() {
    let a = d("2026-10-17");
    assert_eq!(
        panel_range(Period::Month, a, 0).unwrap(),
        (d("2026-10-01"), d("2026-10-31"))
    );
    assert_eq!(
        panel_range(Period::Month, a, -10).unwrap(),
        (d("2025-12-01"), d("2025-12-31"))
    );
    assert_eq!(
        panel_range(Period::Month, a, 4).unwrap(),
        (d("2027-02-01"), d("2027-02-28"))
    );
    assert_eq!(
        panel_range(Period::Year, a, -1).unwrap(),
        (d("2025-01-01"), d("2025-12-31"))
    );
    assert_eq!(
        panel_range(Period::Last12Months, a, 0).unwrap(),
        (d("2025-11-01"), d("2026-10-31"))
    );
    assert_eq!(
        panel_range(Period::Last12Months, a, -1).unwrap(),
        (d("2025-10-01"), d("2026-09-30"))
    );
    assert!(panel_range(Period::Year, a, i32::MAX).is_err());
}

#[test]
fn a_panel_round_trips_through_its_file() {
    let dir = ledger();
    let mut def = definition("Food");
    def.plots[0].series.push(PanelSeries {
        query: SeriesQuery {
            accounts: vec!["Expenses:Food".into()],
            cumulative: true,
            ..SeriesQuery::new(Measure::Flow)
        },
        mark: None,
    });
    let (id, path) = ops::create_panel(dir.path(), def.clone()).unwrap();
    assert_eq!(path, dir.path().join("panels").join(format!("{id}.toml")));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[[plots.series]]"), "{text}");
    assert!(text.contains("period = \"month\""), "{text}");
    let ws = load_workspace(dir.path()).unwrap();
    assert_eq!(ws.panels.len(), 1);
    assert_eq!(ws.panels[0].id, id);
    assert_eq!(ws.panels[0].definition(), def);
}

#[test]
fn a_bad_definition_is_refused() {
    let dir = ledger();
    ops::create_panel(dir.path(), definition("Food")).unwrap();
    let err = |def| ops::create_panel(dir.path(), def).unwrap_err().to_string();
    assert!(err(definition("Food")).contains("a panel named Food already exists"));
    assert!(err(definition(" ")).contains("needs a name"));
    assert!(
        err(PanelDefinition {
            plots: vec![],
            ..definition("A")
        })
        .contains("has no plots")
    );
    let mut empty = definition("B");
    empty.plots[0].series.clear();
    assert!(err(empty).contains("plot 1 has no series"));
    let mut wrong = definition("C");
    wrong.plots[0].series[0].query = SeriesQuery {
        budget: Some("b".into()),
        tags: vec!["x".into()],
        ..SeriesQuery::new(Measure::Budget)
    };
    assert!(err(wrong).contains("panel C: plot 1: `tags` does not apply to a budget series"));
}

#[test]
fn update_keeps_the_id_and_delete_removes_the_file() {
    let dir = ledger();
    let (id, path) = ops::create_panel(dir.path(), definition("Food")).unwrap();
    ops::create_panel(dir.path(), definition("Other")).unwrap();
    let created = load_workspace(dir.path())
        .unwrap()
        .panels
        .into_iter()
        .find(|p| p.id == id)
        .unwrap();
    let (same, _) = ops::update_panel(dir.path(), "Food", definition("Groceries")).unwrap();
    assert_eq!(same, id);
    let ws = load_workspace(dir.path()).unwrap();
    let updated = ws.panels.iter().find(|p| p.id == id).unwrap();
    assert_eq!(updated.name, "Groceries");
    assert_eq!(updated.created_at, created.created_at);
    let err = ops::update_panel(dir.path(), &id, definition("Other")).unwrap_err();
    assert!(
        err.to_string()
            .contains("a panel named Other already exists")
    );
    let (gone, gone_path) = ops::delete_panel(dir.path(), "Groceries").unwrap();
    assert_eq!((gone, gone_path), (id, path.clone()));
    assert!(!path.exists());
    assert!(
        ops::delete_panel(dir.path(), "Groceries")
            .unwrap_err()
            .to_string()
            .contains("no panel Groceries")
    );
}

#[test]
fn a_budget_and_its_panels_are_written_together_or_not_at_all() {
    let dir = ledger();
    let bank = vec![AccountRef {
        account_id: None,
        account_name: Some("Assets:Bank".into()),
    }];
    let panels = |b: &sapphire_ledger_core::Budget| {
        Ok(vec![PanelDefinition {
            name: format!("{} pace", b.name),
            plots: vec![Plot {
                title: None,
                stacked: false,
                series: vec![PanelSeries {
                    query: SeriesQuery {
                        budget: Some(b.id.clone()),
                        line: Some(BudgetLine::Pace),
                        ..SeriesQuery::new(Measure::Budget)
                    },
                    mark: None,
                }],
            }],
            ..definition("x")
        }])
    };
    let (id, _, made) = ops::create_budget_with_panels(
        dir.path(),
        "生活費".into(),
        "living".into(),
        "JPY".into(),
        "100000".parse().unwrap(),
        bank.clone(),
        panels,
    )
    .unwrap();
    assert_eq!(made.len(), 1);
    let ws = load_workspace(dir.path()).unwrap();
    assert_eq!(ws.panels[0].name, "生活費 pace");
    assert_eq!(
        ws.panels[0].plots[0].series[0].query.budget.as_deref(),
        Some(id.as_str())
    );

    // A taken panel name: neither the budget nor its panel is written.
    let err = ops::create_budget_with_panels(
        dir.path(),
        "生活費 2".into(),
        "x".into(),
        "JPY".into(),
        "1".parse().unwrap(),
        bank,
        |_| Ok(vec![definition("生活費 pace")]),
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("a panel named 生活費 pace already exists"),
        "{err}"
    );
    let ws = load_workspace(dir.path()).unwrap();
    assert_eq!(ws.budgets.len(), 1);
    assert_eq!(ws.panels.len(), 1);
}

#[test]
fn validate_reports_a_panel_naming_what_is_not_there() {
    let dir = ledger();
    let mut def = definition("Broken");
    def.plots[0].series.push(PanelSeries {
        query: SeriesQuery {
            budget: Some("nope".into()),
            ..SeriesQuery::new(Measure::Budget)
        },
        mark: None,
    });
    def.plots[0].series.push(PanelSeries {
        query: SeriesQuery {
            accounts: vec!["Nowhere".into()],
            ..SeriesQuery::new(Measure::Flow)
        },
        mark: None,
    });
    ops::create_panel(dir.path(), def).unwrap();
    let ws = load_workspace(dir.path()).unwrap();
    let messages: Vec<String> = ws.validate().into_iter().map(|i| i.message).collect();
    assert!(
        messages.contains(&"panel Broken: no budget nope".to_string()),
        "{messages:?}"
    );
    assert!(
        messages.contains(&"panel Broken: no account matches Nowhere".to_string()),
        "{messages:?}"
    );
    let err = run_panel(
        &ws,
        &ws.panels[0].definition(),
        d("2026-10-17"),
        0,
        d("2026-10-17"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("no budget nope"), "{err}");
}

#[test]
fn run_panel_places_each_series_in_its_plot() {
    let dir = ledger();
    ops::create_budget(
        dir.path(),
        "生活費".into(),
        "living".into(),
        "JPY".into(),
        "31000".parse().unwrap(),
        vec![],
    )
    .unwrap();
    let def = PanelDefinition {
        plots: vec![
            Plot {
                title: Some("Daily".into()),
                stacked: true,
                series: vec![spent()],
            },
            Plot {
                title: None,
                stacked: false,
                series: vec![PanelSeries {
                    query: SeriesQuery {
                        budget: Some("生活費".into()),
                        cumulative: true,
                        ..SeriesQuery::new(Measure::Budget)
                    },
                    mark: None,
                }],
            },
        ],
        ..definition("Two")
    };
    let ws = load_workspace(dir.path()).unwrap();
    let run = run_panel(&ws, &def, d("2026-10-17"), -1, d("2026-10-17")).unwrap();
    assert_eq!((run.from, run.to), (d("2026-09-01"), d("2026-09-30")));
    assert_eq!(run.buckets.len(), 30, "a month panel defaults to days");
    assert_eq!(run.currency, "JPY");
    assert_eq!(run.plots.len(), 2);
    assert!(run.plots[0].stacked);
    assert_eq!(run.plots[0].series[0].mark, Mark::Bar);
    assert!(!run.plots[0].series[0].dashed);
    assert_eq!(run.plots[1].series[0].mark, Mark::Line);
    assert!(run.plots[1].series[0].dashed);
    assert_eq!(
        run.plots[1].series[0].values[29],
        Some("31000".parse().unwrap())
    );
}
