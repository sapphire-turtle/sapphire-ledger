# Series over HTTP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Serve one series query per request at `GET /api/series` on the app server's HTTP listener, as `date`/`series`/`value` rows Grafana's Infinity data source reads.

**Architecture:** `sapphire-ledger-mcp` gets a public `run_query_series` that both the existing IPC method and the new route call, so the wire-to-core conversion stays in one place. `cli/src/http.rs` is the HTTP adapter: flat query-string params → `QuerySeriesParams` → `run_query_series` → rows. `serve::build()` mounts it with `AppServer::http`, behind the framework's external-device token.

**Tech Stack:** Rust 2024, axum 0.8, axum-extra (`query`, serde_html_form), tower 0.5 (`ServiceExt::oneshot` in tests), chrono, serde_json.

**Spec:** `docs/superpowers/specs/2026-10-19-grafana-series-endpoint-design.md`

## Global Constraints

- `sapphire-ledger-api` does not change (version, `API_VERSION`, types).
- Route: `GET /api/series`. One series per request.
- Rows: exactly the keys `date` (`YYYY-MM-DD`, the bucket's first date in range), `series` (`SeriesData::label` as `query_series` gives it), `value` (JSON number). A bucket whose value is `null` is no row. Order: by bucket, then by result-series order.
- Errors: `400` / `503` with body `{"error": "<message>"}`; the message for a query failure is `format!("{e:#}")`, as the IPC method's `render_error`.
- 503 only when the ledger cannot be resolved (no workspace selected, not a ledger); every other failure of ours is 400.
- `cli` adds `axum = "0.8"` and `axum-extra = { version = "0.10", features = ["query"] }` (pick the axum-extra release that depends on axum 0.8 — check `cargo tree -i axum` shows one axum).
- Commit trailer: `Co-Authored-By: <the model writing the commit> <noreply@anthropic.com>`.
- Run `cargo fmt` before each commit; `cargo clippy --workspace --all-targets -- -D warnings` stays clean; `cargo test --workspace` passes.

## Review Focus

1. **Grafana's time picker gives `from` after `to`, or a 10-year range at `day`** — 400 with the query's own message, never a 500 or a panic (pinned in Task 2: `a_range_past_the_bucket_limit_is_400`).
2. **A query-string value that is not a known enum (`measure=spending`, `interval=Month`)** — 400 JSON naming the bad value, not axum's plain-text rejection (pinned in Task 2: `an_unknown_measure_is_400_json`).
3. **A label or group with non-ASCII text (`食費`, `Expenses:食費`) and an account name with `:` in the query string, unencoded or percent-encoded** — both parse to the same account (pinned in Task 2: `an_encoded_account_name_matches`).
4. **A range reaching past today** — the actual line stops at today's bucket; budget lines continue (pinned in Task 2: `no_rows_after_today_for_flow` and `budget_rows_continue_past_today`).
5. **A request with no token, before any workspace is selected** — 401 from the framework, not our 503 leaking that the server has no workspace (pinned in Task 3).

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/sapphire-ledger-mcp/src/methods.rs` | Modify: `run_query_series` (public), `LedgerMethods::query_series` calls it |
| `crates/sapphire-ledger-mcp/src/lib.rs` | Modify: re-export `run_query_series` |
| `cli/src/http.rs` | Create: `routes`, `SeriesParams`, the handler, the row type |
| `cli/src/lib.rs` | Modify: `pub mod http;` |
| `cli/src/serve.rs` | Modify: one `StateSource` shared by methods and routes; `.http(...)` |
| `cli/Cargo.toml` | Modify: axum, axum-extra; dev: tower, async-trait, framework `keys` |
| `cli/tests/http_series.rs` | Create: route tests over a fixture ledger |
| `cli/tests/http_auth.rs` | Create: one server test, the route behind the token |
| `docs/design.md` | Modify: Server section — the route and a Grafana subsection |

---

### Task 1: `run_query_series`, shared by the IPC method

**Files:**
- Modify: `crates/sapphire-ledger-mcp/src/methods.rs:753-772` (the `query_series` method), its `mod tests`
- Modify: `crates/sapphire-ledger-mcp/src/lib.rs:18`

**Interfaces:**
- Consumes: `wire::convert`, `wire::series_query`, `wire::series_result`, `parse_date`, `series::query_series` (all existing).
- Produces: `pub fn run_query_series(ws: &sapphire_ledger_core::Workspace, p: &sapphire_ledger_api::QuerySeriesParams, today: chrono::NaiveDate) -> anyhow::Result<sapphire_ledger_api::SeriesResult>`, re-exported as `sapphire_ledger_mcp::run_query_series`.

- [ ] **Step 1: Write the failing test** in `methods.rs`'s `mod tests`

```rust
#[test]
fn run_query_series_answers_what_the_method_answers() {
    let (_dir, server) = test_server();
    add_account(&server, "Expenses:Food", "Expense");
    add_account(&server, "Assets:Cash", "Asset");
    // one transaction on 2026-05-21: Expenses:Food 1200 / Assets:Cash -1200
    // (same shape as add_transaction_records_and_is_then_queryable)
    let mut q = api::SeriesQuery::new(api::Measure::Flow);
    q.accounts = vec!["Expenses:Food".into()];
    let p = QuerySeriesParams {
        from: "2026-05-01".into(), to: "2026-05-31".into(),
        interval: api::Interval::Month, currency: None,
        series: vec![q],
    };
    let today = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
    let result = run_query_series(server.lock_state().workspace(), &p, today).unwrap();
    assert_eq!(result.buckets, vec![NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()]);
    assert_eq!(result.series[0].label, "Flow");
    assert_eq!(result.series[0].values, vec![Some("1200".to_string())]);
}
```

- [ ] **Step 2: Run** `cargo test -p sapphire-ledger-mcp run_query_series` — Expected: FAIL, `run_query_series` not found.

- [ ] **Step 3: Implement** `run_query_series` in `methods.rs` with the body of today's method (building `SeriesRequest`, calling `series::query_series`, `wire::series_result`). The method becomes: lock, `run_query_series(guard.workspace(), &p, Local::now().date_naive())`, `serde_json::to_string_pretty`, `.map_err(render_error)`. Add it to the `pub use methods::{…}` line in `lib.rs`.

- [ ] **Step 4: Run** `cargo test -p sapphire-ledger-mcp` and `cargo test -p sapphire-ledger --test mcp_tools` — Expected: all PASS (method behaviour unchanged).

- [ ] **Step 5: Commit** — `refactor(mcp): run_query_series, shared by the method and other callers`

---

### Task 2: `GET /api/series` — the HTTP adapter

**Files:**
- Create: `cli/src/http.rs`
- Modify: `cli/src/lib.rs` (`pub mod http;`), `cli/Cargo.toml`
- Test: `cli/tests/http_series.rs`

**Interfaces:**
- Consumes: `sapphire_ledger_mcp::run_query_series` (Task 1), `sapphire_ledger_mcp::StateSource` (`Arc<dyn Fn() -> io::Result<Arc<Mutex<LedgerState>>> + Send + Sync>`).
- Produces: `pub fn routes(source: StateSource) -> axum::Router` in `sapphire_ledger::http`. (Takes a `StateSource`, as `register` does, rather than the spec's `Arc<CurrentLedger>`: same thing to `serve`, and the tests can hand it a fixture.)

`SeriesParams` (private, `#[derive(Deserialize)]`, `#[serde(deny_unknown_fields)]`): `from: String`, `to: String`, `interval: api::Interval`, `currency: Option<String>`, then the `SeriesQuery` fields with the same names and types — `measure`, `accounts`, `account_types`, `tags` (`#[serde(default)] Vec<_>`), `budget`, `line`, `group_by`, `depth`, `top`, `label` (`Option`), `cumulative`, `negate` (`#[serde(default)] bool`). Do not `#[serde(flatten)]` a `SeriesQuery` — serde_html_form cannot collect sequences through `flatten`. A `From<SeriesParams> for api::QuerySeriesParams` with one series.

Row (private, `Serialize`): `date: NaiveDate`, `series: String`, `value: f64` — from `SeriesData::values[i]` parsed with `str::parse::<f64>()`; `None` skipped.

Handler: `Result<axum_extra::extract::Query<SeriesParams>, QueryRejection>`; a rejection → 400 `{"error": rejection.body_text()}`. `source()` error → 503 `{"error": e.to_string()}`. Lock with the same poison recovery as `LedgerMethods::lock_state` (`unwrap_or_else(|e| e.into_inner())`). Query error → 400 `{"error": format!("{e:#}")}`. Today is `chrono::Local::now().date_naive()`.

Test fixture in `cli/tests/http_series.rs`: a `tempdir`, `init_workspace(dir, "JPY")`, `Arc<Mutex<LedgerState>>` opened on it; records added through `LedgerMethods::from_shared(Arc::clone(&state))` (`add_account`, `add_transaction`, `add_budget`); `routes(Arc::new(move || Ok(Arc::clone(&state))))`. Requests with `tower::ServiceExt::oneshot(Request::get(uri).body(Body::empty()))`; body read with `axum::body::to_bytes` and parsed as `serde_json::Value`. Fixture data: accounts `Expenses:Food`, `Expenses:Food:Lunch`, `Expenses:Rent`, `Assets:Cash`; transactions 2026-05-03 Food 1000, 2026-05-20 Food:Lunch 500.5, 2026-06-01 Rent 80000, all against Assets:Cash.

- [ ] **Step 1: Write the failing tests**

| Test | Request (`/api/series?…`) | Asserts |
|---|---|---|
| `flow_by_month_is_rows` | `measure=flow&interval=month&from=2026-05-01&to=2026-06-30&accounts=Expenses:Food` | 200; body `[{"date":"2026-05-01","series":"Flow","value":1500.5},{"date":"2026-06-01","series":"Flow","value":0.0}]` |
| `group_by_account_is_a_row_per_group_per_bucket` | `measure=flow&interval=month&from=2026-05-01&to=2026-06-30&account_types=Expense&group_by=account&depth=2` | rows for 2026-05-01 `Expenses:Food` 1500.5, `Expenses:Rent` 0; 2026-06-01 `Expenses:Food` 0, `Expenses:Rent` 80000 — in that order |
| `repeated_accounts_are_or` | `…&accounts=Expenses:Food:Lunch&accounts=Expenses:Rent` over May–June | May 500.5, June 80000 |
| `an_encoded_account_name_matches` | `accounts=Expenses%3AFood` vs `accounts=Expenses:Food` | same body |
| `cumulative_and_negate` | `…&accounts=Expenses:Food&cumulative=true&negate=true` May–June | May -1500.5, June -1500.5 |
| `label_names_the_series` | `…&label=%E9%A3%9F%E8%B2%BB` (食費) | every `series` is `"食費"` |
| `no_rows_after_today_for_flow` | `interval=month`, `from` = first of this month, `to` = last day of the month after next | every row's `date` ≤ today; at least one row |
| `budget_rows_continue_past_today` | a budget (monthly 30000 on `Expenses:Food`) added via `add_budget`; `measure=budget&budget=<its name>&interval=month`, same range | 3 rows |
| `a_missing_measure_is_400` | `interval=month&from=2026-05-01&to=2026-05-31` | 400; `error` contains `measure` |
| `an_unknown_measure_is_400_json` | `measure=spending&…` | 400; `content-type` is `application/json`; `error` contains `spending` |
| `a_bad_date_is_400` | `from=2026-13-01` | 400 with `error` |
| `a_filter_that_does_not_fit_is_400` | `measure=budget&budget=X&accounts=Expenses:Food` | 400; `error` is the query's message (non-empty) |
| `a_range_past_the_bucket_limit_is_400` | `interval=day&from=2020-01-01&to=2026-12-31` | 400 with `error` |
| `an_unknown_parameter_is_400` | `…&mesure=flow` | 400 |
| `no_ledger_is_503` | source returning `Err(io::Error::other("no workspace selected; run `sapphire-ledger workspace select <dir>`"))` | 503; `error` contains `workspace select` |

- [ ] **Step 2: Run** `cargo test -p sapphire-ledger --test http_series` — Expected: FAIL to compile, `sapphire_ledger::http` missing.

- [ ] **Step 3: Add dependencies** to `cli/Cargo.toml`: `axum`, `axum-extra` (Global Constraints); dev: `tower = { version = "0.5", features = ["util"] }`. Check `cargo tree -p sapphire-ledger -i axum` lists one axum version.

- [ ] **Step 4: Implement** `cli/src/http.rs` per the Interfaces above; `pub mod http;` in `lib.rs`. Module doc: what the route is for (Grafana via Infinity), that it is one series per request, and that the framework puts the token in front.

- [ ] **Step 5: Run** `cargo test -p sapphire-ledger --test http_series` — Expected: all PASS.

- [ ] **Step 6: Commit** — `feat(cli): GET /api/series, one series query as rows for Grafana`

---

### Task 3: Mount the route on the server, behind the token; docs

**Files:**
- Modify: `cli/src/serve.rs:52-57` (`build`), its module doc
- Modify: `cli/Cargo.toml` (dev-dependencies)
- Test: `cli/tests/http_auth.rs`
- Modify: `docs/design.md` (Server section, after the TLS paragraph)

**Interfaces:**
- Consumes: `http::routes` (Task 2); `AppServer::http`, `AppServer::config`, `AppServer::http_auth`; `sapphire_framework::keys::{AuthConfig, Verifier, Verdict, Authenticated}`.
- Produces: nothing new for other tasks.

- [ ] **Step 1: Write the failing test** `cli/tests/http_auth.rs` — one test in its own binary (`LEDGER_CTX` directories are first-writer-wins per process; set the three `SAPPHIRE_LEDGER_*_DIR` vars and `init_app_context(AppKind::Server)` as `mcp_tools.rs` does). A fixture `Verifier` accepts exactly the token `"good-token"` (`Verdict::Accepted(Authenticated { id: <any GrainId>, name: "grafana".into() })`), refuses others. Free port: bind `127.0.0.1:0`, take the address, drop the listener. Server: `serve::build().endpoint(endpoint).config(ServerConfig { http: HttpConfig { listen: Some(addr), ..Default::default() }, ..Default::default() }).http_auth(AuthConfig::new(Arc::new(fixture)))`, spawned. Requests with a raw `tokio::net::TcpStream` (`GET /api/series?measure=flow&interval=month&from=2026-05-01&to=2026-05-31 HTTP/1.1`, `Host: 127.0.0.1`, `Connection: close`), retried until the port accepts (10 s deadline). No workspace is selected.

```rust
assert_eq!(status(get(addr, None).await), 401);              // no token: the framework, not our 503
assert_eq!(status(get(addr, Some("bad")).await), 401);
let resp = get(addr, Some("good-token")).await;
assert_eq!(status(&resp), 503);                              // through auth, to the route
assert!(body(&resp).contains("workspace select"));
```

- [ ] **Step 2: Run** `cargo test -p sapphire-ledger --test http_auth` — Expected: FAIL (404 instead of 503: the route is not mounted). Add dev-dependencies first if it does not compile: `async-trait = "0.1"`, `sapphire-framework = { workspace = true, features = ["server", "ipc", "mcp", "keys"] }`, `tokio` with `net`, `io-util`.

- [ ] **Step 3: Implement** in `build()`: build one `StateSource` (`Arc::new(move || ledger.get())`) and pass a clone to `sapphire_ledger_mcp::register` and to `.http(http::routes(source))`. Add a bullet for the route to `serve.rs`'s module doc.

- [ ] **Step 4: Run** `cargo test --workspace` — Expected: all PASS, including `mcp_tools`.

- [ ] **Step 5: Docs** — in `docs/design.md`'s Server section, after the TLS paragraph: a **`/api/series`** paragraph (always served, behind the same token as `/mcp`, whatever `[mcp] http` says; one series per request; parameters are `query_series`'s, flat; rows `date`/`series`/`value`, no row for a `null`; 400/503 with `{"error"}`) and a **Grafana** subsection with the spec's three points: the Infinity data source (URL `http://127.0.0.1:3173`, `Authorization: Bearer <token>` header from `external-device add`, the URL in Infinity's allowed hosts); a query (JSON, backend parser, the example URL with `${__from:date:YYYY-MM-DD}` / `${__to:date:YYYY-MM-DD}`, columns `date` time / `series` string / `value` number, Partition by values on `series`); Grafana in Docker (`host.docker.internal` with `--add-host=host.docker.internal:host-gateway`, a bind the container reaches, `allowed_hosts` needed only for `/mcp`). Keep the section's tone — short bold-led paragraphs.

- [ ] **Step 6: Commit** — `feat(cli): serve /api/series on the server's listener; Grafana in the design`

- [ ] **Step 7: By hand** — on the homelab host: `external-device add` a `grafana` device allowed the ledger, add the Infinity data source, build one panel (monthly flow by account, depth 2) and check it matches `sapphire-ledger panel run` / the desktop for the same months. Report what was seen; fix docs that were wrong.
