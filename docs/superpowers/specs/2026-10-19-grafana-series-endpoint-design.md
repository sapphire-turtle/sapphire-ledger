# Series over HTTP: the series query for Grafana

- **Scope**: one HTTP route, `GET /api/series`, on the app server's listener, answering a single series query as rows Grafana's Infinity data source reads; the shared conversion it needs; docs with a Grafana setup
- **Builds on**: the series query and panels (`feat/panels`, [2026-10-17 spec](2026-10-17-series-query-and-panels-design.md)) — this branch is stacked on it
- **Changes no API**: `sapphire-ledger-api` stays as it is; the route's row shape lives in the CLI crate alone

## What this is

The Charts screen draws one panel at a time. Grafana can put many charts side
by side, overlay them and change the range with a picker, which suits
**finding out which views of the books are worth keeping** — the ledger is
new, and which charts matter is not known yet.

So this exposes the series query, as it is, over HTTP: every parameter of
`query_series` becomes a query-string parameter, and the answer is a table of
`date`, `series`, `value` rows. Each request recomputes from the books, so a
backdated or corrected transaction shows on the next refresh.

Grafana reaches it through the
[Infinity](https://grafana.com/grafana/plugins/yesoreyeram-infinity-datasource/)
data source, from Grafana's backend: Grafana runs on the same host as the
ledger server, so the ledger stays bound to loopback.

## What this is not

- **Not Prometheus.** A `/metrics` endpoint is scraped for the value *now*;
  a ledger's past changes when a transaction is backdated, and a scrape
  history would never show it. Answering PromQL (`/api/v1/query_range`) would
  mean parsing and evaluating PromQL. Neither is done.
- **No panels over HTTP.** Running a saved panel (`/api/panels/…`) waits until
  panels worth running from Grafana exist.
- **No series arithmetic, one currency** — as the series query itself.
- **No CSV, no wide format.** One response shape.
- **No new transport security.** Plain HTTP behind the external-device token,
  as `/mcp`; TLS stays out of scope.

## The route

`GET /api/series`, served by `AppServer::http` on the listener `/mcp` uses.
The framework serves application routes **whenever they are given**, whatever
`[mcp] http` says, and always behind an external-device bearer token. So on a
default `server.toml` the route answers on `127.0.0.1:3173`, and a request
without a valid token gets the framework's 401 (503 with the bridge down).

### Parameters

The range and one series, flat in the query string. Names and values are
`QuerySeriesParams`' and `SeriesQuery`'s, enums in their snake_case form.

| Parameter | Required | Meaning |
|---|---|---|
| `from`, `to` | yes | `YYYY-MM-DD`, both ends included. |
| `interval` | yes | `day` \| `week` \| `month` \| `year`. |
| `currency` | no | Default `base_currency`. |
| `measure` | yes | `flow` \| `balance` \| `budget`. |
| `accounts` | no, repeatable | `accounts=Expenses:Food&accounts=Expenses:Rent`. |
| `account_types` | no, repeatable | |
| `tags` | no, repeatable | |
| `budget` | for `budget` | A budget id or name. |
| `line` | no | `pace` \| `remaining`. |
| `group_by` | no | `none` \| `account` \| `account_type` \| `tag`. |
| `depth`, `top` | no | Integers ≥ 1. |
| `cumulative`, `negate` | no | `true` \| `false`. Default `false`. |
| `label` | no | The series' name when it is not grouped. |

**One series per request.** A Grafana panel holds several queries (A, B, …),
so several series are several requests; an array of series in a query string
would be unreadable. All the series query's own rules apply unchanged — a
filter that does not fit the measure is an error, at most 1000 buckets and
50 result series.

From Grafana the range comes from the time picker:
`from=${__from:date:YYYY-MM-DD}&to=${__to:date:YYYY-MM-DD}`.

Repeated parameters are read with `axum-extra`'s `Query` (serde_html_form);
axum's own `Query` (serde_urlencoded) cannot collect them into a `Vec`.

### Response

`200`, `application/json`: an array of rows, ordered by bucket, then by the
series' order in the result (so group order, `Other` last).

```json
[
  {"date": "2026-10-01", "series": "Expenses:Food", "value": 12000},
  {"date": "2026-10-01", "series": "Expenses:Rent", "value": 80000},
  {"date": "2026-11-01", "series": "Expenses:Food", "value": 9800.5}
]
```

- **`date`** is the bucket's name: its first date inside the range.
- **`series`** is the result series' name, as `query_series` gives it — the
  group key, or the label.
- **`value`** is a JSON number. The query's amounts are rounded to 2 places
  first; an `f64` holds a household amount exactly enough to draw.
- **A bucket with no value is no row.** `query_series` answers `null` for an
  actual series after today; leaving the row out stops Grafana's line at
  today.

The three columns never change, whatever the grouping, so an Infinity query is
set up once — `date` as time, `series` as string, `value` as number — and
Grafana's **Partition by values** transformation on `series` makes one line
per group.

### Errors

| Status | When | Body |
|---|---|---|
| 400 | A parameter is missing, unknown in value, or malformed; or `query_series` refuses the query (a filter that does not fit the measure, `from > to`, too many buckets or series, an unknown budget, another currency on a budget) | `{"error": "<message>"}` |
| 503 | The server has no workspace selected, or it is not a ledger | `{"error": "<message>"}` |
| 401 / 503 | No valid token / no bridge | the framework's |

The message is the one the IPC method gives for the same failure.

## The code

### `cli/src/http.rs` — new

The HTTP adapter, nothing else:

- `pub fn routes(ledger: Arc<CurrentLedger>) -> axum::Router` with the one
  route.
- `SeriesParams`, the flat query-string struct, and its conversion into
  `QuerySeriesParams` with one `SeriesQuery`.
- The handler: get the current ledger (503 on failure), lock it, convert and
  run the query through the shared function below, flatten the result into
  rows (400 on a query error).
- The row struct (`date`, `series`, `value`), private to this module.

### `sapphire-ledger-mcp` — one function made public

`LedgerMethods::query_series` builds a core `SeriesRequest` from
`QuerySeriesParams` and runs it. That body moves into a public function —
`run_query_series(ws: &Workspace, p: &QuerySeriesParams, today: NaiveDate) ->
anyhow::Result<api::SeriesResult>` — which the method and the route both call,
so the wire-to-core conversion stays in one place. The method's behaviour does
not change.

### `cli/src/serve.rs`

`build()` gives the same `CurrentLedger` to the methods and to the routes:
`.http(http::routes(ledger))`. `build_oneshot()` mounts nothing, as now.

### Dependencies

`cli` gains `axum` (0.8, the version the framework already pulls in) and
`axum-extra` with the `query` feature. The framework's `http` feature is
already on through `mcp`.

## Testing

- **Route tests** in `http.rs`, calling the router with
  `tower::ServiceExt::oneshot` (no token layer): rows for a flow series over
  a small fixture ledger; `group_by=account` giving a row per group per
  bucket; no rows after today; repeated `accounts`; `cumulative=true`; a
  missing `measure`, a bad date and a field that does not fit the measure
  each 400 with an `error`; no workspace 503.
- **One server test** through `build()` with `AppServer::http_auth` and a
  fixture token: no token is 401, the fixture token is 200 — the route is
  behind the framework's auth.
- **The shared function** keeps the existing `query_series` method tests
  passing unchanged.
- **By hand**: one Grafana panel against a real ledger, following the docs.

## Docs

`docs/design.md`, Server section: the route, its parameters and response, and
that it is always on behind the token. A short **Grafana** subsection:

- Install Infinity; add a data source with URL `http://127.0.0.1:3173`, an
  `Authorization: Bearer <token>` header (an external device allowed the
  ledger), and that URL in Infinity's own allowed hosts.
- A query: type JSON, parser backend, URL
  `/api/series?measure=flow&interval=month&group_by=account&from=${__from:date:YYYY-MM-DD}&to=${__to:date:YYYY-MM-DD}`,
  columns `date` (time), `series` (string), `value` (number), then
  Partition by values on `series`.
- **Grafana in Docker**: the container's `127.0.0.1` is not the host's. Use
  `host.docker.internal` (with `--add-host=host.docker.internal:host-gateway`
  on Linux) and bind the ledger to an address the container reaches. The
  `Host` allowlist is rmcp's and guards `/mcp` alone, so `/api/series` needs
  no `allowed_hosts` entry — but with `/mcp` also on, a non-loopback bind
  still needs one, or `serve` refuses to start.
