//! `GET /api/series` against a fixture ledger, without the token layer: the
//! route's half — parameters in, rows out, errors as JSON. The token in front
//! of it is `http_auth.rs`'s.

use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::{Datelike, Months, NaiveDate};
use sapphire_ledger::http;
use sapphire_ledger_core::{AccountType, LedgerState, Posting, ops};
use serde_json::{Value, json};
use tower::ServiceExt as _;

struct Fixture {
    dir: tempfile::TempDir,
    state: Arc<Mutex<LedgerState>>,
    router: axum::Router,
}

fn date(raw: &str) -> NaiveDate {
    raw.parse().unwrap()
}

fn posting(account: &str, amount: &str) -> Posting {
    serde_json::from_value(json!({"account_name": account, "amount": amount, "currency": "JPY"}))
        .unwrap()
}

fn spend(root: &Path, on: &str, account: &str, amount: &str) {
    ops::create_transaction(
        root,
        date(on),
        "spend".into(),
        None,
        vec!["living".into()],
        None,
        vec![
            posting(account, amount),
            posting("Assets:Cash", &format!("-{amount}")),
        ],
        None,
        false,
    )
    .expect("create_transaction");
}

/// Food 1000 on 2026-05-03, Food:Lunch 500.5 on 2026-05-20, Rent 80000 on
/// 2026-06-01, all paid from Assets:Cash and tagged `living`.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    sapphire_ledger_core::init_workspace(root, "JPY").unwrap();
    for (name, ty) in [
        ("Expenses:Food", AccountType::Expense),
        ("Expenses:Food:Lunch", AccountType::Expense),
        ("Expenses:Rent", AccountType::Expense),
        ("Assets:Cash", AccountType::Asset),
    ] {
        ops::create_account(
            root,
            name.into(),
            ty,
            vec![],
            date("2026-01-01"),
            None,
            vec![],
        )
        .expect("create_account");
    }
    spend(root, "2026-05-03", "Expenses:Food", "1000");
    spend(root, "2026-05-20", "Expenses:Food:Lunch", "500.5");
    spend(root, "2026-06-01", "Expenses:Rent", "80000");
    let state = Arc::new(Mutex::new(LedgerState::open(root).unwrap()));
    let source = Arc::clone(&state);
    let router = http::routes(Arc::new(move || Ok(Arc::clone(&source))));
    Fixture { dir, state, router }
}

async fn get(router: &axum::Router, query: &str) -> (StatusCode, Option<String>, Value) {
    let resp = router
        .clone()
        .oneshot(
            Request::get(format!("/api/series?{query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let content_type = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string());
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, content_type, body)
}

const MAY_JUNE: &str = "interval=month&from=2026-05-01&to=2026-06-30";

fn error_of(body: &Value) -> &str {
    body["error"].as_str().expect("an error message")
}

#[tokio::test]
async fn flow_by_month_is_rows() {
    let f = fixture();
    let (status, _, body) = get(
        &f.router,
        &format!("measure=flow&{MAY_JUNE}&accounts=Expenses:Food"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!([
            {"date": "2026-05-01", "series": "Flow", "value": 1500.5},
            {"date": "2026-06-01", "series": "Flow", "value": 0.0},
        ])
    );
}

#[tokio::test]
async fn group_by_account_is_a_row_per_group_per_bucket() {
    let f = fixture();
    let (status, _, body) = get(
        &f.router,
        &format!("measure=flow&{MAY_JUNE}&account_types=Expense&group_by=account&depth=2"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!([
            {"date": "2026-05-01", "series": "Expenses:Food", "value": 1500.5},
            {"date": "2026-05-01", "series": "Expenses:Rent", "value": 0.0},
            {"date": "2026-06-01", "series": "Expenses:Food", "value": 0.0},
            {"date": "2026-06-01", "series": "Expenses:Rent", "value": 80000.0},
        ])
    );
}

#[tokio::test]
async fn repeated_accounts_are_or() {
    let f = fixture();
    let (_, _, body) = get(
        &f.router,
        &format!("measure=flow&{MAY_JUNE}&accounts=Expenses:Food:Lunch&accounts=Expenses:Rent"),
    )
    .await;
    assert_eq!(
        body,
        json!([
            {"date": "2026-05-01", "series": "Flow", "value": 500.5},
            {"date": "2026-06-01", "series": "Flow", "value": 80000.0},
        ])
    );
}

#[tokio::test]
async fn an_encoded_account_name_matches() {
    let f = fixture();
    let (_, _, plain) = get(
        &f.router,
        &format!("measure=flow&{MAY_JUNE}&accounts=Expenses:Food"),
    )
    .await;
    let (status, _, encoded) = get(
        &f.router,
        &format!("measure=flow&{MAY_JUNE}&accounts=Expenses%3AFood"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plain, encoded);
}

#[tokio::test]
async fn cumulative_and_negate() {
    let f = fixture();
    let (_, _, body) = get(
        &f.router,
        &format!("measure=flow&{MAY_JUNE}&accounts=Expenses:Food&cumulative=true&negate=true"),
    )
    .await;
    assert_eq!(
        body,
        json!([
            {"date": "2026-05-01", "series": "Flow", "value": -1500.5},
            {"date": "2026-06-01", "series": "Flow", "value": -1500.5},
        ])
    );
}

#[tokio::test]
async fn label_names_the_series() {
    let f = fixture();
    let (_, _, body) = get(
        &f.router,
        &format!("measure=flow&{MAY_JUNE}&accounts=Expenses:Food&label=%E9%A3%9F%E8%B2%BB"),
    )
    .await;
    let rows = body.as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r["series"] == "食費"), "got: {body}");
}

/// From the first of this month to the last day of the month after next.
fn around_today() -> (NaiveDate, NaiveDate, String) {
    let today = chrono::Local::now().date_naive();
    let from = today.with_day(1).unwrap();
    let to = from + Months::new(3) - chrono::Days::new(1);
    (today, from, format!("interval=month&from={from}&to={to}"))
}

#[tokio::test]
async fn no_rows_after_today_for_flow() {
    let f = fixture();
    let (today, _, range) = around_today();
    let (status, _, body) = get(&f.router, &format!("measure=flow&{range}")).await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        let date: NaiveDate = row["date"].as_str().unwrap().parse().unwrap();
        assert!(date <= today, "a row after today: {row}");
    }
}

#[tokio::test]
async fn budget_rows_continue_past_today() {
    let f = fixture();
    ops::create_budget(
        f.dir.path(),
        "生活費".into(),
        "living".into(),
        "JPY".into(),
        "30000".parse().unwrap(),
        vec![],
    )
    .expect("create_budget");
    f.state.lock().unwrap().reload().unwrap();
    let (_, _, range) = around_today();
    let (status, _, body) = get(
        &f.router,
        &format!("measure=budget&budget=%E7%94%9F%E6%B4%BB%E8%B2%BB&{range}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got: {body}");
    assert_eq!(body.as_array().unwrap().len(), 3, "got: {body}");
}

#[tokio::test]
async fn a_missing_measure_is_400() {
    let f = fixture();
    let (status, _, body) = get(&f.router, "interval=month&from=2026-05-01&to=2026-05-31").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error_of(&body).contains("measure"), "got: {body}");
}

#[tokio::test]
async fn an_unknown_measure_is_400_json() {
    let f = fixture();
    let (status, content_type, body) =
        get(&f.router, &format!("measure=spending&{MAY_JUNE}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    assert!(error_of(&body).contains("spending"), "got: {body}");
}

#[tokio::test]
async fn a_bad_date_is_400() {
    let f = fixture();
    let (status, _, body) = get(
        &f.router,
        "measure=flow&interval=month&from=2026-13-01&to=2026-12-31",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!error_of(&body).is_empty());
}

#[tokio::test]
async fn a_filter_that_does_not_fit_is_400() {
    let f = fixture();
    let (status, _, body) = get(
        &f.router,
        &format!("measure=budget&budget=X&accounts=Expenses:Food&{MAY_JUNE}"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!error_of(&body).is_empty());
}

#[tokio::test]
async fn a_range_past_the_bucket_limit_is_400() {
    let f = fixture();
    let (status, _, body) = get(
        &f.router,
        "measure=flow&interval=day&from=2020-01-01&to=2026-12-31",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!error_of(&body).is_empty());
}

#[tokio::test]
async fn an_unknown_parameter_is_400() {
    let f = fixture();
    let (status, _, body) = get(&f.router, &format!("measure=flow&{MAY_JUNE}&mesure=flow")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!error_of(&body).is_empty());
}

#[tokio::test]
async fn no_ledger_is_503() {
    let router = http::routes(Arc::new(|| {
        Err(std::io::Error::other(
            "no workspace selected; run `sapphire-ledger workspace select <dir>`",
        ))
    }));
    let (status, _, body) = get(&router, &format!("measure=flow&{MAY_JUNE}")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(error_of(&body).contains("workspace select"), "got: {body}");
}

/// An unset Grafana variable sends its parameter empty (`group_by=`): that is
/// the parameter left out, not a value to refuse.
#[tokio::test]
async fn empty_values_are_absent() {
    let f = fixture();
    let (_, _, absent) = get(&f.router, &format!("measure=flow&{MAY_JUNE}")).await;
    let (status, _, empty) = get(
        &f.router,
        &format!(
            "measure=flow&{MAY_JUNE}&currency=&accounts=&account_types=&tags=&line=\
             &group_by=&depth=&top=&label=&cumulative=&negate="
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "got: {empty}");
    assert_eq!(empty, absent);
}
