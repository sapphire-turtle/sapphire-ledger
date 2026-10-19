//! The ledger's own HTTP routes, on the app server's listener.
//!
//! `GET /api/series` answers one series query as `date`/`series`/`value`
//! rows: the shape Grafana's Infinity data source reads from its backend, a
//! panel's several queries being several requests. The parameters are
//! `query_series`'s, flat in the query string. The framework puts the
//! external-device token in front of every route given to `AppServer::http`;
//! nothing here checks one.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum_extra::extract::{Query, QueryRejection};
use chrono::NaiveDate;
use sapphire_ledger_api::{
    self as api, AccountType, BudgetLine, GroupBy, Interval, Measure, QuerySeriesParams,
    SeriesQuery,
};
use sapphire_ledger_mcp::StateSource;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The ledger's HTTP routes, each resolving the ledger through `source` per
/// request.
pub fn routes(source: StateSource) -> Router {
    Router::new()
        .route("/api/series", get(series))
        .with_state(source)
}

/// `query_series`' parameters with its one series, flat. Not a flattened
/// [`SeriesQuery`]: serde_html_form cannot collect repeated keys through
/// `flatten`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeriesParams {
    from: String,
    to: String,
    interval: Interval,
    currency: Option<String>,
    measure: Measure,
    #[serde(default)]
    accounts: Vec<String>,
    #[serde(default)]
    account_types: Vec<AccountType>,
    #[serde(default)]
    tags: Vec<String>,
    budget: Option<String>,
    line: Option<BudgetLine>,
    group_by: Option<GroupBy>,
    depth: Option<u32>,
    top: Option<u32>,
    #[serde(default)]
    cumulative: bool,
    #[serde(default)]
    negate: bool,
    label: Option<String>,
}

impl From<SeriesParams> for QuerySeriesParams {
    fn from(p: SeriesParams) -> QuerySeriesParams {
        QuerySeriesParams {
            from: p.from,
            to: p.to,
            interval: p.interval,
            currency: p.currency,
            series: vec![SeriesQuery {
                measure: p.measure,
                accounts: p.accounts,
                account_types: p.account_types,
                tags: p.tags,
                budget: p.budget,
                line: p.line,
                group_by: p.group_by,
                depth: p.depth,
                top: p.top,
                cumulative: p.cumulative,
                negate: p.negate,
                label: p.label,
            }],
        }
    }
}

/// One value of one result series.
#[derive(Serialize)]
struct Row {
    date: NaiveDate,
    series: String,
    value: f64,
}

fn error(status: StatusCode, message: String) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

async fn series(
    State(source): State<StateSource>,
    query: Result<Query<SeriesParams>, QueryRejection>,
) -> Response {
    let Query(params) = match query {
        Ok(q) => q,
        Err(rejection) => return error(StatusCode::BAD_REQUEST, rejection.body_text()),
    };
    let state = match source() {
        Ok(state) => state,
        Err(e) => return error(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
    };
    let guard = state.lock().unwrap_or_else(|e| e.into_inner());
    let today = chrono::Local::now().date_naive();
    match sapphire_ledger_mcp::run_query_series(guard.workspace(), &params.into(), today) {
        Ok(result) => Json(rows(&result)).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

/// By bucket, then in the result's series order; a `null` is no row.
fn rows(result: &api::SeriesResult) -> Vec<Row> {
    let mut rows = Vec::new();
    for (i, date) in result.buckets.iter().enumerate() {
        for s in &result.series {
            if let Some(value) = s.values.get(i).cloned().flatten() {
                rows.push(Row {
                    date: *date,
                    series: s.label.clone(),
                    value: value.parse().unwrap_or(f64::NAN),
                });
            }
        }
    }
    rows
}
