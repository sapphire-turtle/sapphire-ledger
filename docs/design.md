# Design

This document captures the locked-in design decisions for sapphire-ledger.
It is intended as a primer for contributors (human or AI) picking up the
project. The high-level concept lives in [`README.md`](../README.md); this
file goes deeper into the *why* and the on-disk shape.

## Goals

- **Local-first** household ledger using **double-entry bookkeeping**.
- **Plain-text source of truth** so a regular git workflow (or Syncthing,
  etc.) can carry the data between machines and between collaborators.
- **AI-collaborative**: a human and an AI agent (Claude, etc.) can edit the
  same ledger concurrently. Conflict surface must stay tiny.
- **Strict integrity**: a corrupt journal is worse than a slow one, so
  validation always errs on the side of rejecting bad data.

## Format: Markdown with YAML front matter, and TOML

Every record on disk is a single file in one of two formats:

| Record | Format | Why |
|---|---|---|
| Account, transaction | **Markdown with YAML front matter** | They carry free text — an account's `description`, a transaction's `comment` — that people and agents write as prose. The Markdown body is where prose belongs; every other field is front matter. |
| Assertion, price-log entry, workspace config | **TOML** | No free text, so nothing to gain from a body. |

The front matter is read and written by sapphire-framework's
`sapphire-framework-frontmatter` (YAML via `serde-saphyr`), shared with the
other `sapphire-*` apps.

The reasons TOML was first chosen over YAML still shape the format:

- **Decimal values** (amounts, exchange rates) are persisted as **strings**
  (`"1200"`, `"150.5"`) via `#[serde(with = "rust_decimal::serde::str")]`, so
  no float ever touches money. Hand-written YAML may leave an amount unquoted
  (`amount: 1200.50`); it is read as the same digits, not through a float.
- **Values YAML would misread** — a tag `yes`, a currency `NO`, an id of
  digits, a date — are quoted when the ledger writes them.
- **Broken YAML fails loudly**: `load_workspace` is fail-fast and names the
  file, so a malformed record written by an agent stops the load rather than
  being skipped.

Assertions and prices stay TOML for the reasons in the table below, which
was the original comparison:

| Format | Verdict |
|---|---|
| YAML (alone) | Indentation-sensitive; AI agents often produce broken YAML; `serde_yaml` is effectively frozen. |
| JSON / JSONL | No comments; JSONL doesn't match "one file per record". |
| Beancount | Excellent semantics but its tooling assumes many entries per file, which is the opposite of what we want for git-merge tolerance. |
| **TOML** | Line-oriented (small, local merge conflicts), Rust ecosystem is first-class, AI writes it reliably, supports comments, already used elsewhere in the `sapphire-*` workspace. |

### Schema versions and migration

`schema_version` in `.sapphire-ledger/config.toml` names the on-disk layout:

1. Every record is TOML.
2. Accounts and transactions are Markdown with YAML front matter.

A build reads exactly its own version: `load_workspace` and every
`core::ops` write refuse an older ledger (pointing at `sapphire-ledger
migrate`) and a newer one (pointing at an upgrade). `sapphire-ledger migrate`
([`migrate.rs`](../crates/sapphire-ledger-core/src/migrate.rs)) upgrades in
place:

- Nothing is written until every TOML record has parsed, read back unchanged
  from its Markdown, and been checked against any `.md` already at its
  destination. `--dry-run` stops there.
- `schema_version` is bumped last, so an interrupted run leaves an old-version
  ledger that a re-run finishes.
- TOML `#` comments have nowhere to go; files that look like they had some
  are listed.

A TOML account or transaction inside a version-2 ledger — what a client still
on version 1 syncs in after another machine migrated — fails the load, and
`migrate` sweeps it up. Every machine sharing a ledger should be upgraded
together.

## File granularity: one record, one file

The fundamental rule is **one record = one file** — a transaction, an
account, an assertion, or a price-log entry. Two people (or a person and an
AI) editing different records touch different files, so git merges become
trivial.

```
my-ledger/
├── .sapphire-ledger/
│   ├── config.toml          # workspace config (git-tracked)
│   └── .gitignore           # vestigial: still lists cache.sqlite
├── accounts/
│   └── {Type}/.../{Leaf}.md
├── transactions/
│   └── {year}/{MM}/{grain-id}.md
├── assertions/
│   └── {year}/{MM}/{grain-id}.toml
└── prices/
    └── {year}/{MM}/{grain-id}.toml
```

A workspace is any directory that contains a `.sapphire-ledger/`
sub-directory, located by walking upward from the current directory — the
same convention `git` uses.

### Account hierarchy as directory hierarchy

Account names are colon-separated (`Assets:Cash:USD`). On disk this maps
directly to a directory tree under `accounts/`:

| Account name | File path |
|---|---|
| `Equity` | `accounts/Equity.md` |
| `Assets:Cash:JPY` | `accounts/Assets/Cash/JPY.md` |
| `Assets:Cash:USD` | `accounts/Assets/Cash/USD.md` |

Path conversion functions (`account_relative_path`,
`account_name_from_relative_path`) live in
[`workspace.rs`](../crates/sapphire-ledger-core/src/workspace.rs).

Account name validation rejects:

- Empty names or empty segments (`""`, `Assets::Cash`).
- `.` or `..` segments (path traversal).
- Segments containing `/` or `\`.

### Transaction, assertion and price-log paths

All three follow `{kind}/{year}/{MM}/{grain-id}.{ext}` — `.md` for
transactions, `.toml` for assertions and prices. The id is stored both
as the filename and as a field inside the file (`id: 0a1b2c3`) so a record
survives being moved or copied.

Ids come from [`grain-id`](https://crates.io/crates/grain-id). Records whose id
is their filename — transactions, assertions, prices — mint with
`GrainId::now_unix()` on the first attempt, whose decisecond resolution makes a
`{year}/{MM}/` listing time-ordered. Uniqueness is checked across the whole
record kind — every id already in use under `transactions/`, `assertions/` or
`prices/` — not merely against the destination path: two records minted in one
decisecond but dated in different months land in different `{year}/{MM}/`
directories, so a per-path check alone would let both keep the same id. A
collision therefore means another record of the same kind was minted in the
same decisecond; `now_unix()` is a pure function of that decisecond, so
re-minting it would return the same id forever, and `core::ops` mints
`GrainId::random()` on every retry instead. Uniqueness is the hard requirement
and ordering is the nicety, so ordering is what gives way — and inside a burst
it was unobtainable anyway. The `date` field inside each record remains the real
ordering key.

**Accounts use `GrainId::random()` from the start**: an account's id is not in
its path, so it does no ordering work, and a chart of accounts created in one
sitting would otherwise share a long leading prefix — the opposite of what CLI
completion wants. Because an account's id is not its filename, the
refuse-to-overwrite check cannot see an id collision, so `create_account`
checks the minted id against the accounts already on disk.

## Data model

All struct definitions live in [`crates/sapphire-ledger-core/src/`](../crates/sapphire-ledger-core/src/).
This section summarizes the shapes; the source is authoritative.

### Account references

`Account` carries an `id`. Postings and assertions reference an account by
`account_id`, with a denormalized `account_name` beside it:

```yaml
postings:
- account_id: 0a1b2c3           # authoritative; survives a rename
  account_name: Expenses:Food   # for whoever reads the raw file
```

The id is what links; the name is never used for matching when the id is
present. This is what makes renaming an account cheap: the account file
changes and nothing else has to.

The alternative — name-only references plus a rename operation that rewrites
every transaction — was rejected because that rewrite spans many files while
remote sync resolves conflicts per path, last-writer-wins. A client adding a
transaction under the old name mid-rename would leave a record pointing at
nothing.

Three rules follow:

1. **A stale `account_name` is not an error.** Older records keep the old name
   until rewritten for some other reason. Flagging it would reintroduce the
   whole-history rewrite this design avoids.
2. **Duplicate account ids *are* an error.** An account's path comes from its
   name, so a rename is a file move; if that races under sync, one id can land
   at two paths.
3. **Either field alone is accepted on read.** Both present means the id wins;
   name-only is resolved to an id when the record is written; neither is an
   error. A hand-written record must not require an id lookup first.

### Account ([`account.rs`](../crates/sapphire-ledger-core/src/account.rs))

`accounts/Assets/Cash/USD.md`:

```markdown
---
id: a3k9m2p
name: Assets:Cash:USD
type: Asset                  # Asset | Liability | Equity | Income | Expense
currencies:                  # empty/omitted = any currency allowed
- USD
opened_at: 2026-05-21
# closed_at: ...             # optional
---

ドル現金。旅行用の封筒に入れてある。
```

The body is the optional `description`, in Markdown. A `description:` key in
the front matter is also accepted while the body is empty, so whoever writes
the file by hand need not know which field lives where; both at once is an
error.

`Account::allows_currency(&str)` returns `true` if `currencies` is empty
or contains the requested currency. The validator uses this to flag
postings that violate an account's currency constraint.

### Transaction ([`transaction.rs`](../crates/sapphire-ledger-core/src/transaction.rs))

`transactions/2026/05/0a1b2c3.md`:

```markdown
---
id: 0a1b2c3
date: 2026-05-21
narration: イオン買い物
payee: イオン                 # optional
tags:                         # optional
- grocery
status: cleared               # optional: cleared | pending
created_at: 2026-05-21T18:30:00+09:00
updated_at: 2026-05-21T18:30:00+09:00
postings:
- account_id: 0f2e9a1         # authoritative; account_name is denormalized
  account_name: Expenses:Food
  amount: "1200"              # string-encoded Decimal
  currency: JPY
  # memo: ...                 # optional, per-posting
  # price: { value: "150", currency: JPY }   # optional, see Multi-currency
- account_id: 7c4d8b0
  account_name: Liabilities:CreditCard:Rakuten
  amount: "-1200"
  currency: JPY
---

週末のまとめ買い。ポイント 2 倍デー。
```

`narration` is the one-line title; the body is the optional `comment`, in
Markdown, with the same front-matter fallback as an account's description.

`Transaction::validate()` enforces:

1. At least 2 postings.
2. Per-currency net contribution is zero, with each posting's
   contribution computed via `Posting::balance_contribution()` (which
   applies the inline `price` conversion when present — see below).

### Multi-currency

A cross-currency transaction uses Beancount-style inline prices:

```yaml
# Move 15,000 JPY into a USD cash account at a rate of 150 JPY/USD.
postings:
- account_id: 9k3n7x2
  account_name: Assets:Cash:USD
  amount: "100"
  currency: USD
  price:                      # 1 USD = 150 JPY
    value: "150"
    currency: JPY
- account_id: 2m5p8q1
  account_name: Assets:Cash:JPY
  amount: "-15000"
  currency: JPY
```

`Posting::balance_contribution()` returns `(currency, signed_amount)`:

- No price set → `(self.currency, self.amount)` as-is.
- Price set → `(price.currency, self.amount * price.value)`.

The transaction balances if every resulting currency totals zero. In the
example above, both sides contribute to JPY and net to zero.

**Out of scope for now**: lot tracking, unrealized FX P&L, and converting a
stored rate back into a balance. The price log itself is stored — see
[Price log](#price-log-pricesrs) below — but nothing yet reads it back to
convert. See the open follow-ups.

### Assertion ([`assertion.rs`](../crates/sapphire-ledger-core/src/assertion.rs))

A balance assertion declares the expected balance of an account at the end
of a given date (after all transactions on that date — hledger semantics,
not Beancount's before-the-date semantics).

```toml
id = "b7f3n0q"
account_id   = "4h6j2k9"       # authoritative; account_name is denormalized
account_name = "Assets:Brokerage"
date = "2026-05-31"
created_at = "2026-05-31T23:59:00+09:00"
updated_at = "2026-05-31T23:59:00+09:00"

[[balances]]
amount = "100"
currency = "USD"

[[balances]]
amount = "5000"
currency = "JPY"
```

One assertion file declares one account's expected balances at one date,
optionally across multiple currencies. Failure to match is a hard error
(no "warn and continue" mode). There is no Beancount-style `pad`
auto-balancing — mismatches must be fixed manually.

Subtree assertions ("`Assets` and all descendants total X") are not
supported in MVP. Leaf accounts only.

### Price log ([`prices.rs`](../crates/sapphire-ledger-core/src/prices.rs))

A standalone record of one observed exchange rate, independent of any
transaction: `1 base = rate quote` on `date`. Stored under
`prices/{year}/{MM}/{grain-id}.toml`, the same layout as transactions and
assertions.

```toml
id = "0a1b2c3"
date = "2026-05-21"
base = "USD"
quote = "JPY"
rate = "150.5"
source = "manual"              # optional
created_at = "2026-05-21T18:30:00+09:00"
updated_at = "2026-05-21T18:30:00+09:00"
```

This is storage only. `sapphire-ledger` can record a rate, but nothing yet
reads the price log back to convert a balance into `base_currency` or build a
net-worth report — that's the conversion half deferred out of Multi-currency
above.

### Opening balances

When a new account begins with a non-zero balance, that's expressed as a
regular transaction posting against `Equity:OpeningBalances`. Assertions
are reserved for verification, not for declaring initial state. This
matches Beancount and hledger conventions.

### Workspace config ([`config.rs`](../crates/sapphire-ledger-core/src/config.rs))

```toml
schema_version = 1
base_currency = "JPY"     # used for FX-converted reporting (planned)

[cache]
scan_interval = 60         # reserved for a future mtime-rescan consumer; unread today
```

`base_currency` will drive net-worth / FX-converted views once price
*conversion* is built on top of the price log (see [Price
log](#price-log-pricesrs)). Without it, only per-currency views are possible.

## Validation pipeline

Validation runs at two layers.

**Per-record**, on parse:

- `Transaction::validate()` — balance + posting count.
- Account-name shape (`account_name_segments`).

**Cross-record**, against the whole workspace
([`validate.rs`](../crates/sapphire-ledger-core/src/validate.rs)):

`Workspace::validate()` returns `Vec<ValidationIssue>` and never
short-circuits — the user gets every issue in one pass. Issues currently
detected:

- Two accounts share an `id` (a rename that raced under sync).
- Two accounts share a `name`. A name-only posting would otherwise resolve
  to whichever of them won the lookup table.
- Two transactions, two assertions or two price entries share an `id`. Ids
  are unique per record kind, not per `{year}/{MM}/` directory; a record
  whose date was corrected moves between directories, and that move is a
  delete-plus-add that can race under sync.
- Transaction fails per-record validation (balance, posting count).
- Posting references an account that doesn't exist in `accounts/`.
- Posting or assertion carries no account reference at all (neither
  `account_id` nor `account_name`).
- Posting currency violates the account's `currencies` constraint.
- Assertion references an undefined account.
- Assertion balance currency violates the account's `currencies`
  constraint.

`ValidationIssue` carries optional `transaction_id`, `assertion_id`, and
`account` fields so MCP tools (`validate_workspace`, and `sapphire-ledger
check` on the CLI) can return structured reports.

**Not yet implemented** (see issues):

- Balance assertions actually compared against historical posting sums.
- Date ordering / future-date sanity.
- Account `opened_at` / `closed_at` enforcement against posting dates.

## Cache strategy

There is deliberately **no ledger-specific cache**. `load_workspace` walks the
record files on every load, which is milliseconds at household scale.

The framework supplies mtime tracking (`sapphire-framework-track`) and search
(`sapphire-framework-retrieve`) when a consumer needs them; neither is wired
into any crate here yet — the `[cache]` section of the workspace config
(`scan_interval`) is reserved for whichever of them ends up consuming it. A
ledger-specific index — postings by account, running balances — is deferred
until walking is measurably slow. See issue #1.

`rusqlite` is not a dependency of this workspace, and no crate here adds one.
`grain-id` ships an optional `rusqlite` feature — off by default, and not
enabled by anything in this workspace — that only adds `ToSql`/`FromSql` impls
for `GrainId`; it is not a caching mechanism and does not pull rusqlite into
the build unless a crate here explicitly turns the feature on, so `Cargo.lock`
currently has no rusqlite entry at all. A SQLite-backed cache is a deliberate
non-goal here, not a stopgap for a slow walk that hasn't been measured yet.

## Crate structure

```
sapphire-ledger/
├── cli/                     # `sapphire-ledger`, the one binary: CLI, app server, `mcp`
├── desktop/                 # egui GUI: a client of the app server
└── crates/
    ├── sapphire-ledger-core/  # data model, record I/O, migration, validation, write path
    └── sapphire-ledger-mcp/   # the ledger's MCP tools and server methods — LIBRARY only
```

The MCP crate is intentionally **a library, not a binary**. `sapphire-ledger`
registers its methods on the app server and hands its tools to the
framework, which serves them on stdio (`sapphire-ledger mcp`) and at `/mcp`.
See the [MCP server](#mcp-server) section for the details.

Mobile builds (potentially with Dioxus) and a VS Code extension are
deferred — see open follow-ups.

## MCP server

Built on sapphire-framework's MCP (`sapphire_framework::mcp`, feature `mcp`).
The ledger's tools do not open the ledger themselves: each one calls a
`sapphire-ledger.<tool>` method on the running app server over IPC, the way
the framework's own tools call `workspace.*`. So one set of tools serves both
transports the framework provides, and both act on the workspace the server
has selected:

- **stdio** — `sapphire-ledger mcp`, the framework's subcommand, which an
  agent launches as a child process. It dials the running server and never
  starts one; a missing server is reported per call, and the next call tries
  again.
- **HTTP** — `/mcp` on the server's HTTP listener, behind the workgroup's
  external-device tokens, when `server.toml` turns it on. See [Server:
  `sapphire-ledger serve`](#server-sapphire-ledger-serve) below.

The two halves live in `sapphire-ledger-mcp`:
[`methods.rs`](../crates/sapphire-ledger-mcp/src/methods.rs) is the server
side (`register` adds the methods to the app server's router; each runs
against the current workspace's `LedgerState`), and
[`tools.rs`](../crates/sapphire-ledger-mcp/src/tools.rs) is the rmcp handler
(`LedgerTools`) the framework serves. The methods carry their own API version
(`APP_API`), checked in the IPC handshake.

### Tools

Nine tools of the ledger's own, declared in
[`tools.rs`](../crates/sapphire-ledger-mcp/src/tools.rs): five read, four
write. Beside them the framework's read-only tools (`status`,
`workspace_show`, `workspace_search`, `workspace_read_file`,
`workspace_list_dir`, `workgroup_show`, `device_list`). The framework's raw
file writes are deliberately left out: a record written without the ledger's
checks can leave the books unbalanced. Descriptions below are the tools' own
`#[tool(description = ...)]` text, condensed to one line each.

| Tool | Does |
|---|---|
| `list_accounts` | List every account with its id, name, type, allowed currencies and open date. The id is the stable handle — it survives a rename. |
| `get_transaction` | Show one transaction, with all of its postings, by id. |
| `query_postings` | Find postings, optionally filtered by account (id or name), currency and date range; each result carries its transaction's id, date and narration. |
| `query_prices` | Find price-log entries, optionally filtered by base, quote and date range. |
| `validate_workspace` | Re-read the ledger from disk and report every validation issue. Empty array means consistent; a stale `account_name` alone is NOT an issue. |
| `add_account` | Create an account. Fails if one with that name already exists. Every account a posting names must be created first. Returns the new account's id. |
| `add_transaction` | Record a transaction. Postings must sum to zero per currency, and every account named must already exist. Nothing is written if validation fails. |
| `add_assertion` | Record a balance assertion: what an account should hold at the END of a date. A mismatch is a hard error when the books are checked. |
| `add_price` | Record an observed exchange rate in the price log: one unit of `base` costs `rate` units of `quote` on `date`. |

### Crate is library-only

`sapphire-ledger-mcp` exposes the tools and the methods as reusable code. It
does **not** produce a binary. There is no `sapphire-ledger-mcp` CLI to
install separately.

### stdio: `sapphire-ledger mcp`

`sapphire-ledger mcp` is the framework's `mcp` subcommand: it speaks MCP over
stdio so it can be wired directly into Claude Desktop / Claude Code as an
`mcp__sapphire-ledger__*` server. It serves the running server's selected
workspace, so `--ledger-dir` does not apply, and the old `--init` (create a
ledger where the agent stands) is gone: create one with `sapphire-ledger
init` and `workspace select` it.

### Server: `sapphire-ledger serve`

**Built.** Since #19 there is one binary. `sapphire-ledger serve` — or a bare
`sapphire-ledger`, which is the same thing and what an installed service
runs — is the ledger's
[sapphire-framework](https://github.com/sapphire-turtle/sapphire-framework)
app server: the framework's `AppServer` (IPC socket, the current workspace,
signals, `<data dir>/logs/app.log`) plus what is the ledger's own — sync
through the host's `sapphire-bridge`, the `sapphire-ledger.*` methods its
MCP tools call, and the duplicate-id watch. The framework's other verbs
(`status`, `service install|uninstall|status`,
`workspace init|select|show|move|remove|map`, `workgroup …`, `device …`,
`embedding …`, `external-device …`, `mcp`) are flattened into the same command line
beside the ledger's own. The original reasoning for `/mcp` lives in
[`docs/superpowers/specs/2026-09-02-ledger-server-design.md`](superpowers/specs/2026-09-02-ledger-server-design.md);
its key-file and user/device sections were superseded by sapphire-framework
0.15's external devices, described below.

**The server serves one workspace: the one selected.** `--ledger-dir` is for
the ledger's own verbs; the server serves whatever `workspace select` (or
`workspace init`, `workspace map`) last chose, remembered in
`<config dir>/workspace.toml` across restarts. The MCP tools and the watch
follow a switch without a restart.

**A missing bridge is not fatal.** With no `sapphire-bridge` running the
server starts without sync and says so in its log; sync starts with the next
server start after the bridge is up. Any other bridge failure — another
version, say — stops the start.

**The ledger's own verbs still work on the files directly.** `init`, `check`
and the write commands do not go through the server: the ledger keeps
no cache a second process could collide on, so they need nothing from it. A
running server notices their writes the way it notices a human's editor.
Routing them over IPC instead is a later step.

**HTTP settings live in `<config dir>/server.toml`**, not in flags, because
the service manager starts the binary as plain `serve`. The file is the
framework's (`ServerConfig`), shared by every sapphire app:

```toml
[http]
listen = "127.0.0.1:3173"      # the ledger's default when omitted
allowed_hosts = ["ledger.example.net"]

[mcp]
http = true                    # serve /mcp; off by default
```

A missing file is all defaults — `/mcp` **off**; an unknown key is an error,
so a typo cannot quietly fall back to loopback. The ledger's earlier format
(`[http] enabled` / `addr`) is one such error: rename `addr` to `listen` and
replace `enabled = true` with `[mcp] http = true`. A listener that cannot
bind is logged and does not stop the server.

**The `Host` allowlist is never empty.** rmcp refuses any request whose
`Host` header isn't on an allowlist, and its own default is loopback-only.
The framework always adds loopback on top of `allowed_hosts`, so an empty
list is never handed to rmcp, which would read that as "allow every host"
instead of "loopback only". `listen` is what widens the bind, and
`allowed_hosts` is what widens this list. With `/mcp` on and bound beyond
loopback with no `allowed_hosts` at all, `sapphire-ledger serve` **refuses
to start**: a wide bind with an empty allowlist would 403 every request,
which is useless rather than dangerous, and would present as a client bug
rather than a configuration mistake.

**TLS is out of scope.** `/mcp` is plain HTTP, and has
no plans to speak anything else, so any bind past loopback puts every
device's bearer token on the wire in cleartext. Put it behind a reverse
proxy that terminates TLS, or reach it over a network that is already
encrypted (a WireGuard or Tailscale interface, say), rather than exposing
`listen` directly.

**Clients are the workgroup's external devices.** A bearer token belongs to
an external device in the workgroup's ledger (`external_devices/<id>.toml`,
synced by the bridge), which lists the applications it may use. The ledger
keeps only the token's SHA-256; the token itself is shown once, by
`external-device add` or `rotate`. `/mcp` asks the bridge about every
presented token through the framework's `BridgeVerifier`, which caches a
success for 30 seconds — so a `retire` or `rotate` takes effect within that
long, with no restart. A refused token is 401; with the bridge not running
every request is 503, because the token may well be fine. This server holds
no key file and no user registry of its own.

**Duplicate ids are reported, never resolved.** A background watch re-walks
the workspace once at startup and then every five minutes, and logs any id
(or, for accounts, any name) claimed by more than one file at `WARN`, naming
the id and every path carrying it. It does not merge, delete, or re-id
anything. This matters because two of ledger's own path conventions embed
mutable data — a transaction's or assertion's path derives from its *date*,
an account's from its *name* — so a client offline across a rename or a date
correction can resurrect a stale path under sync and leave one id at two
files. A duplicated transaction is a wrong balance, so the server surfaces
the problem loudly rather than guessing at a fix.

A first-time setup:

```
sapphire-ledger init ~/ledger                         # the ledger's files
sapphire-ledger service install                       # or run `sapphire-ledger serve` in a terminal
sapphire-ledger workspace select ~/ledger             # what the server serves
sapphire-ledger external-device add laptop            # token to stdout, once; needs the bridge
sapphire-ledger status
```

`init` and `workspace init` can come in either order: `init` completes a
marker the framework made, keeping its `[workspace.*]` tables.

`last_updated_by` is not implemented. `Authenticated` carries the external
device's id and name — that is the hook the field will use once it exists,
but no record schema has the field yet.

### Library API shape

- `sapphire_ledger_mcp::register(router, source)` adds the ledger's methods
  to the app server's IPC router; `source` resolves the current workspace's
  `LedgerState` per call (`serve::CurrentLedger`), opened once per root.
- `LedgerMethods` is one call's work over an `Arc<Mutex<LedgerState>>`; its
  unit tests pin the ledger's behaviour without a server.
- `sapphire_ledger_mcp::service()` is the `McpService` handed to
  `AppServer::mcp`, building a `LedgerTools` per session.
- `serve::build()` is the whole server minus the bridge, which the
  integration test (`cli/tests/mcp_tools.rs`) runs and drives over IPC.

## Licensing

| Crate | License |
|---|---|
| `sapphire-ledger-core` | MIT OR Apache-2.0 |
| `sapphire-ledger-mcp`  | MIT OR Apache-2.0 |
| `sapphire-ledger` (`cli/`) | MIT OR Apache-2.0 |
| `sapphire-ledger-desktop` | MIT OR Apache-2.0 (initial) |

Permissive everywhere for MVP. If a desktop or mobile build is eventually
distributed through an official store (Mac App Store, Microsoft Store,
iOS/Android), the corresponding crate may switch to GPL-3.0-or-later for
that distribution channel. The reverse direction (GPL → permissive) is
hard to undo without contributor consent, so the default stays
permissive until there is a reason to tighten it.

## Status

- ✅ Workspace scaffold, `cargo build` clean.
- ✅ Data model (accounts, transactions, postings, prices, assertions, config).
- ✅ TOML round-trip with serde.
- ✅ Accounts and transactions as Markdown with YAML front matter (schema 2),
  with `sapphire-ledger migrate` from schema 1.
- ✅ Path conventions, workspace discovery, `init_workspace`.
- ✅ Repository I/O and cross-record validation + `sapphire-ledger check`.
- ✅ Record ids everywhere, with id-linked account references.
- ✅ Price-log records (storage; conversion and reporting deferred).
- ✅ `core::ops` write path.
- ✅ Framework dependency (`AppContext`) + `LedgerState` session object.
- ✅ One binary (#19): `sapphire-ledger serve` is the framework app server,
  with a duplicate-id watch; the framework's verbs sit beside the ledger's.
- ✅ MCP on the framework's MCP (sapphire-framework#233): the ledger's tools
  call the server over IPC, served on stdio (`sapphire-ledger mcp`) and at
  `/mcp` behind external-device tokens.
- 🚧 Sync through the bridge is wired but not yet exercised end to end; the
  ledger's own verbs still write the files directly rather than over IPC.
- ✅ CLI write commands (`account add`, `tx add`, `assertion add`, `price add`, `tx list`).
- ✅ Desktop GUI (`sapphire-ledger-desktop`): a pure client of the running
  server, like sapphire-sync's. The Ledger tab reads and writes through the
  `sapphire-ledger.*` methods (accounts with balances, postings, validation
  issues, new transaction / account forms); the Sync tab is the framework's
  `SyncPanel`. CJK text uses a font the OS already has
  (`sapphire_framework::gui::fonts`); no font is bundled.
- 🚧 Price conversion / base-currency reporting.
- 🚧 Phase 2: see the issue tracker.

See [GitHub issues](https://github.com/fluo10/sapphire-ledger/issues) for
follow-up work.
