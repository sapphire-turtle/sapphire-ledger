# sapphire-ledger

> Language: **English** | [日本語](README.ja.md)

A double-entry household ledger built on [sapphire-framework](https://github.com/fluo10/sapphire-framework) — file-based, local-first, made for human-agent collaboration.

## Concept

- **Plain text as source of truth** — accounts and transactions are Markdown files with YAML front matter (the description or comment is the body); assertions and prices are `.toml`. Read and edit them with any tool
- **No cache required** — `load_workspace` reads the files directly on every load, which is fast enough at household scale; no SQLite, no database
- **Double-entry bookkeeping** — every transaction is a set of balanced postings (debits = credits per currency)
- **Multi-currency from day one** — postings carry a currency; cross-currency transactions use inline exchange prices
- **One file per record** — each transaction, account, balance assertion, and price-log entry lives in its own file to keep git merges conflict-free under human + AI co-editing
- **Human–AI collaborative editing** — designed to work alongside AI agents (Claude, etc.) that can read, create, and edit entries in the same ledger via git or Syncthing sync

## Project structure

```
sapphire-ledger/
├── cli/                     # the one binary, sapphire-ledger: CLI, and the app server (sync, MCP over stdio and /mcp)
├── desktop/                 # Desktop GUI (egui): the books and sync, a client of the server
└── crates/
    ├── sapphire-ledger-core/  # Data model, record I/O, migration, validation, write path
    └── sapphire-ledger-mcp/   # the ledger's MCP tools and server methods (library)
```

## Status

The data model, validation, write path and MCP server work. Everything ships
as one binary, `sapphire-ledger`: the ledger's own verbs, and the
[sapphire-framework](https://github.com/sapphire-turtle/sapphire-framework)
app-server verbs beside them (`serve`, `status`, `service`, `workspace`,
`workgroup`, `device`, `external-device`, `mcp`, …). With the server
running (`sapphire-ledger serve`, or a bare `sapphire-ledger`), point an MCP
client at `sapphire-ledger mcp` to read and record entries over stdio, or turn
on its `/mcp` in `server.toml` and reach it over HTTP with an external-device
token. Both serve the workspace the server has selected (see
[`docs/design.md`](docs/design.md#mcp-server) for the setup and the auth
model). The desktop app (`sapphire-ledger-desktop`) is a client of that
server: it shows the accounts with their balances and each account's
postings, records transactions and accounts, and carries the framework's
sync panel. The CLI writes too:

```
sapphire-ledger account add Expenses:Food --type Expense
sapphire-ledger tx add --narration "groceries" \
  --posting "Expenses:Food 1200 JPY" --posting "Assets:Cash -1200 JPY"
sapphire-ledger tx list --account Expenses:Food
```

plus `assertion add` and `price add` — all thin wrappers over the same
`core::ops` the MCP tools use, so the rules are identical whoever is typing.

A ledger made by an older build (every record TOML) has to be upgraded once
with `sapphire-ledger migrate` (`--dry-run` to preview); commit it first.

## License

This repository contains components under different licenses:

| Component | License |
|-----------|---------|
| `sapphire-ledger-core` | MIT OR Apache-2.0 |
| `sapphire-ledger-mcp` | MIT OR Apache-2.0 |
| `sapphire-ledger` (`cli/`) | MIT OR Apache-2.0 |
| `sapphire-ledger-desktop` | GPL-3.0-or-later, with [App Store marketplace exception](desktop/LICENSE.App-Store-Exception) |

See the `LICENSE` (or `LICENSE-MIT` / `LICENSE-APACHE`) file in each component's directory for the full license text.
