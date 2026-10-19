//! Ready-made panels, shared by the server (a budget's panels) and the
//! desktop (the editor's templates).

use crate::{
    AccountType, Budget, BudgetLine, GroupBy, Interval, Mark, Measure, PanelDefinition,
    PanelSeries, Period, Plot, SeriesQuery,
};

fn line(query: SeriesQuery) -> PanelSeries {
    PanelSeries { query, mark: None }
}

fn bar(query: SeriesQuery) -> PanelSeries {
    PanelSeries {
        query,
        mark: Some(Mark::Bar),
    }
}

fn labelled(label: &str, query: SeriesQuery) -> SeriesQuery {
    SeriesQuery {
        label: Some(label.to_owned()),
        ..query
    }
}

fn budget_spent(b: &Budget) -> SeriesQuery {
    SeriesQuery {
        tags: vec![b.tag.clone()],
        account_types: vec![AccountType::Expense],
        ..SeriesQuery::new(Measure::Flow)
    }
}

fn budget_series(b: &Budget, line: BudgetLine) -> SeriesQuery {
    SeriesQuery {
        budget: Some(b.id.clone()),
        line: Some(line),
        ..SeriesQuery::new(Measure::Budget)
    }
}

/// The month's tagged spending against the pro-rated budget: cumulated, and
/// day by day.
pub fn budget_pace(b: &Budget) -> PanelDefinition {
    PanelDefinition {
        name: format!("{} pace", b.name),
        period: Period::Month,
        interval: Some(Interval::Day),
        currency: Some(b.currency.clone()),
        plots: vec![
            Plot {
                title: Some("Cumulative".into()),
                stacked: false,
                series: vec![
                    line(labelled(
                        "Spent",
                        SeriesQuery {
                            cumulative: true,
                            ..budget_spent(b)
                        },
                    )),
                    line(labelled(
                        "Budget",
                        SeriesQuery {
                            cumulative: true,
                            ..budget_series(b, BudgetLine::Pace)
                        },
                    )),
                ],
            },
            Plot {
                title: Some("Daily".into()),
                stacked: false,
                series: vec![
                    bar(labelled("Spent", budget_spent(b))),
                    line(labelled("Budget", budget_series(b, BudgetLine::Pace))),
                ],
            },
        ],
    }
}

/// The balance accounts against the planned path. `None` for a budget with
/// no balance accounts: there is nothing to start the plan from.
pub fn budget_balance(b: &Budget) -> Option<PanelDefinition> {
    let accounts: Vec<String> = b
        .balance_accounts
        .iter()
        .filter_map(|r| r.account_id.clone().or_else(|| r.account_name.clone()))
        .collect();
    if accounts.is_empty() {
        return None;
    }
    Some(PanelDefinition {
        name: format!("{} balance", b.name),
        period: Period::Month,
        interval: Some(Interval::Day),
        currency: Some(b.currency.clone()),
        plots: vec![Plot {
            title: None,
            stacked: false,
            series: vec![
                line(labelled(
                    "Balance",
                    SeriesQuery {
                        accounts,
                        ..SeriesQuery::new(Measure::Balance)
                    },
                )),
                line(labelled("Plan", budget_series(b, BudgetLine::Remaining))),
            ],
        }],
    })
}

/// Monthly spending by second-level account, the seven largest and the rest.
pub fn expenses_by_account() -> PanelDefinition {
    PanelDefinition {
        name: "Expenses by account".into(),
        period: Period::Last12Months,
        interval: Some(Interval::Month),
        currency: None,
        plots: vec![Plot {
            title: None,
            stacked: true,
            series: vec![bar(SeriesQuery {
                account_types: vec![AccountType::Expense],
                group_by: Some(GroupBy::Account),
                depth: Some(2),
                top: Some(7),
                ..SeriesQuery::new(Measure::Flow)
            })],
        }],
    }
}

/// Assets and liabilities together, month by month.
pub fn net_worth() -> PanelDefinition {
    PanelDefinition {
        name: "Net worth".into(),
        period: Period::Last12Months,
        interval: Some(Interval::Month),
        currency: None,
        plots: vec![Plot {
            title: None,
            stacked: false,
            series: vec![line(labelled(
                "Net worth",
                SeriesQuery {
                    account_types: vec![AccountType::Asset, AccountType::Liability],
                    ..SeriesQuery::new(Measure::Balance)
                },
            ))],
        }],
    }
}

/// Income (as positive) beside expenses, month by month.
pub fn income_and_expenses() -> PanelDefinition {
    let flow = |ty| SeriesQuery {
        account_types: vec![ty],
        ..SeriesQuery::new(Measure::Flow)
    };
    PanelDefinition {
        name: "Income and expenses".into(),
        period: Period::Last12Months,
        interval: Some(Interval::Month),
        currency: None,
        plots: vec![Plot {
            title: None,
            stacked: false,
            series: vec![
                bar(labelled(
                    "Income",
                    SeriesQuery {
                        negate: true,
                        ..flow(AccountType::Income)
                    },
                )),
                bar(labelled("Expenses", flow(AccountType::Expense))),
            ],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountRef, Measure};

    fn budget(accounts: Vec<AccountRef>) -> Budget {
        Budget {
            id: "b1".into(),
            name: "生活費".into(),
            tag: "living".into(),
            currency: "JPY".into(),
            monthly_amount: "100000".into(),
            balance_accounts: accounts,
        }
    }

    #[test]
    fn the_budget_templates_follow_the_budget() {
        let b = budget(vec![AccountRef {
            account_id: Some("acc".into()),
            account_name: Some("Assets:Bank".into()),
        }]);
        let pace = budget_pace(&b);
        assert_eq!(pace.name, "生活費 pace");
        assert_eq!(pace.currency.as_deref(), Some("JPY"));
        assert_eq!(pace.plots.len(), 2);
        assert_eq!(pace.plots[0].series[0].query.tags, ["living"]);
        assert!(pace.plots[0].series[1].query.cumulative);
        assert_eq!(pace.plots[0].series[1].query.budget.as_deref(), Some("b1"));
        assert_eq!(pace.plots[1].series[0].mark, Some(Mark::Bar));
        let balance = budget_balance(&b).expect("it has an account to start from");
        assert_eq!(balance.name, "生活費 balance");
        assert_eq!(balance.plots[0].series[0].query.accounts, ["acc"]);
        assert_eq!(
            balance.plots[0].series[1].query.line,
            Some(BudgetLine::Remaining)
        );
        // Nothing to start a remaining line from: no balance panel.
        assert!(budget_balance(&budget(vec![])).is_none());
    }

    #[test]
    fn the_general_templates_are_what_the_spec_lists() {
        let e = expenses_by_account();
        let s = &e.plots[0].series[0];
        assert_eq!(
            (s.query.group_by, s.query.depth, s.query.top),
            (Some(GroupBy::Account), Some(2), Some(7))
        );
        assert!(e.plots[0].stacked);
        assert_eq!(
            net_worth().plots[0].series[0].query.measure,
            Measure::Balance
        );
        let ie = income_and_expenses();
        assert!(ie.plots[0].series[0].query.negate);
        assert_eq!(ie.period, Period::Last12Months);
    }
}
