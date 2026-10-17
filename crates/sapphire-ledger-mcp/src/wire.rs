//! The ledger's records as the API answers with them: core's file records
//! turned into `sapphire_ledger_api`'s.
//!
//! The two are kept apart so a change to the files is not a change to the
//! API; the tests here pin that, today, they serialize alike.

use sapphire_ledger_api as api;
use sapphire_ledger_core as core;

pub fn budget(b: &core::Budget) -> api::Budget {
    api::Budget {
        id: b.id.clone(),
        name: b.name.clone(),
        tag: b.tag.clone(),
        currency: b.currency.clone(),
        monthly_amount: b.monthly_amount.to_string(),
        balance_accounts: b
            .balance_accounts
            .iter()
            .map(|r| api::AccountRef {
                account_id: r.account_id.clone(),
                account_name: r.account_name.clone(),
            })
            .collect(),
    }
}

pub fn budget_report(
    b: &core::Budget,
    year: i32,
    month: u32,
    r: &core::report::BudgetReport,
) -> api::BudgetReport {
    let amount = |d: rust_decimal::Decimal| core::report::round_amount(d).to_string();
    api::BudgetReport {
        budget: budget(b),
        month: format!("{year:04}-{month:02}"),
        daily_budget: amount(r.daily_budget),
        opening_balance: amount(r.opening_balance),
        days: r
            .days
            .iter()
            .map(|d| api::BudgetDay {
                date: d.date,
                spent: amount(d.spent),
                spent_cumulative: amount(d.spent_cumulative),
                budget_cumulative: amount(d.budget_cumulative),
                planned_balance: amount(d.planned_balance),
                elapsed: d.elapsed,
                actual_balance: d.actual_balance.map(amount),
            })
            .collect(),
    }
}

pub fn account(a: &core::Account) -> api::Account {
    api::Account {
        id: a.id.clone(),
        name: a.name.clone(),
        account_type: account_type(a.account_type),
        currencies: a.currencies.clone(),
        opened_at: a.opened_at,
        closed_at: a.closed_at,
        default_tags: a.default_tags.clone(),
        description: a.description.clone(),
    }
}

fn account_type(t: core::AccountType) -> api::AccountType {
    match t {
        core::AccountType::Asset => api::AccountType::Asset,
        core::AccountType::Liability => api::AccountType::Liability,
        core::AccountType::Equity => api::AccountType::Equity,
        core::AccountType::Income => api::AccountType::Income,
        core::AccountType::Expense => api::AccountType::Expense,
    }
}

pub fn transaction(t: &core::Transaction) -> api::Transaction {
    api::Transaction {
        id: t.id.clone(),
        date: t.date,
        narration: t.narration.clone(),
        payee: t.payee.clone(),
        tags: t.tags.clone(),
        status: t.status.map(|s| match s {
            core::TransactionStatus::Cleared => api::TransactionStatus::Cleared,
            core::TransactionStatus::Pending => api::TransactionStatus::Pending,
        }),
        created_at: t.created_at,
        updated_at: t.updated_at,
        postings: t.postings.iter().map(posting).collect(),
        comment: t.comment.clone(),
    }
}

fn posting(p: &core::Posting) -> api::Posting {
    api::Posting {
        account_id: p.account_id.clone(),
        account_name: p.account_name.clone(),
        amount: p.amount,
        currency: p.currency.clone(),
        price: p.price.as_ref().map(|price| api::Price {
            value: price.value,
            currency: price.currency.clone(),
        }),
        memo: p.memo.clone(),
    }
}

pub fn price_entry(e: &core::PriceEntry) -> api::PriceEntry {
    api::PriceEntry {
        id: e.id.clone(),
        date: e.date,
        base: e.base.clone(),
        quote: e.quote.clone(),
        rate: e.rate,
        source: e.source.clone(),
        created_at: e.created_at,
        updated_at: e.updated_at,
    }
}

pub fn validation_issue(i: core::ValidationIssue) -> api::ValidationIssue {
    api::ValidationIssue {
        message: i.message,
        transaction_id: i.transaction_id,
        assertion_id: i.assertion_id,
        account: i.account,
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use super::*;

    fn json(v: &impl Serialize) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    fn at(s: &str) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(s).unwrap()
    }

    /// The API's records answer with the JSON core's did before they were
    /// split: the API's records still serialize as core's do, every field set,
    /// then none.
    #[test]
    fn the_wire_records_serialize_as_core_does() {
        let full = core::Account {
            id: "acct001".into(),
            name: "Assets:Cash:USD".into(),
            account_type: core::AccountType::Asset,
            currencies: vec!["USD".into()],
            opened_at: "2026-01-01".parse().unwrap(),
            closed_at: Some("2026-12-31".parse().unwrap()),
            default_tags: vec!["living".into()],
            description: Some("cash".into()),
        };
        let bare = core::Account {
            currencies: vec![],
            closed_at: None,
            default_tags: vec![],
            description: None,
            ..full.clone()
        };
        for a in [&full, &bare] {
            assert_eq!(json(&account(a)), json(a));
        }

        let posting = |price| core::Posting {
            account_id: Some("acct001".into()),
            account_name: Some("Assets:Cash:USD".into()),
            amount: "-12.50".parse().unwrap(),
            currency: "USD".into(),
            price,
            memo: Some("change".into()),
        };
        let full = core::Transaction {
            id: "tx00001".into(),
            date: "2026-05-21".parse().unwrap(),
            narration: "groceries".into(),
            payee: Some("shop".into()),
            tags: vec!["food".into()],
            status: Some(core::TransactionStatus::Pending),
            created_at: at("2026-05-21T09:00:00+09:00"),
            updated_at: at("2026-05-21T10:00:00+09:00"),
            postings: vec![
                posting(Some(core::Price {
                    value: "150".parse().unwrap(),
                    currency: "JPY".into(),
                })),
                core::Posting {
                    account_id: None,
                    memo: None,
                    ..posting(None)
                },
            ],
            comment: Some("notes".into()),
        };
        let bare = core::Transaction {
            payee: None,
            tags: vec![],
            status: None,
            comment: None,
            ..full.clone()
        };
        for t in [&full, &bare] {
            assert_eq!(json(&transaction(t)), json(t));
        }

        let full = core::PriceEntry {
            id: "pr00001".into(),
            date: "2026-05-21".parse().unwrap(),
            base: "USD".into(),
            quote: "JPY".into(),
            rate: "150.25".parse().unwrap(),
            source: Some("manual".into()),
            created_at: at("2026-05-21T09:00:00+09:00"),
            updated_at: at("2026-05-21T09:00:00+09:00"),
        };
        let bare = core::PriceEntry {
            source: None,
            ..full.clone()
        };
        for e in [&full, &bare] {
            assert_eq!(json(&price_entry(e)), json(e));
        }

        let full = core::ValidationIssue {
            message: "unbalanced".into(),
            transaction_id: Some("tx00001".into()),
            assertion_id: Some("as00001".into()),
            account: Some("Assets:Cash".into()),
        };
        let bare = core::ValidationIssue {
            message: "unbalanced".into(),
            transaction_id: None,
            assertion_id: None,
            account: None,
        };
        for i in [full, bare] {
            assert_eq!(json(&validation_issue(i.clone())), json(&i));
        }
    }
}
