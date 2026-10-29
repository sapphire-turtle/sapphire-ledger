# sapphire-ledger

> Language: **English** | [日本語](README.ja.md)

A double-entry household ledger built on [sapphire-framework](https://github.com/fluo10/sapphire-framework) — file-based, local-first, made for human-agent collaboration.

> **Works with any MCP client.** AI integration is a standard MCP server, so sapphire-ledger is not tied to [sapphire-agent](https://github.com/sapphire-turtle/sapphire-agent) despite the shared name — any AI harness that can act as an MCP client (Claude Code, Claude Desktop, …) can use it.

## Concept

- **Plain text as source of truth** — accounts and transactions are Markdown files with YAML front matter (the description or comment is the body); assertions and prices are `.toml`. Read and edit them with any tool
- **No cache required** — `load_workspace` reads the files directly on every load, which is fast enough at household scale; no SQLite, no database
- **Double-entry bookkeeping** — every transaction is a set of balanced postings (debits = credits per currency)
- **Multi-currency from day one** — postings carry a currency; cross-currency transactions use inline exchange prices
- **One file per record** — each transaction, account, balance assertion, and price-log entry lives in its own file to keep git merges conflict-free under human + AI co-editing
- **Human–AI collaborative editing** — designed to work alongside AI agents (Claude, etc.) that can read, create, and edit entries in the same ledger via git or Syncthing sync

## Installation

Install **one** of the two editions. Both put the same command, `sapphire-ledger`, on
PATH: the desktop app's binary also runs every command of the headless one (`serve`,
`mcp`, `tx`, …), so it is a superset of it.

| Edition | For | How |
|---|---|---|
| Desktop app (`sapphire-ledger-desktop`) | Everyday use, with the GUI | Installer for Windows, macOS and Linux (in preparation) |
| Headless (`sapphire-ledger-headless`) | Hosts and servers without a GUI | Install script, cargo-binstall or cargo |

Installing both leaves two `sapphire-ledger` commands; `sapphire-ledger status` warns
when the one on PATH is not the one you ran.

### Headless: install script

```sh
# Linux, macOS
curl -fsSL https://raw.githubusercontent.com/sapphire-turtle/sapphire-framework/main/install/install.sh | sh -s -- ledger
```

```powershell
# Windows
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/sapphire-turtle/sapphire-framework/main/install/install.ps1))) ledger
```

This installs `sapphire-ledger` together with `sapphire-bridge`, which it syncs
through, into `~/.local/bin`. See the
[install script's README](https://github.com/sapphire-turtle/sapphire-framework/tree/main/install) for the options.

### Headless: cargo-binstall or cargo

```sh
cargo binstall sapphire-ledger-headless
# or build it
cargo install sapphire-ledger-headless
```

Either installs the `sapphire-ledger` binary. (0.1 to 0.2 were published as the
`sapphire-ledger` crate.)

### MCP clients

Either edition's `sapphire-ledger mcp` serves the ledger's tools over stdio, so an MCP
client's configuration is the same for both:

```json
{
  "mcpServers": {
    "sapphire-ledger": { "command": "sapphire-ledger", "args": ["mcp"] }
  }
}
```

## Project structure

```
sapphire-ledger/
├── headless/                # sapphire-ledger-headless: the `sapphire-ledger` binary without the GUI
├── desktop/                 # Desktop GUI (egui): the books and sync, a client of the server; runs the same commands
└── crates/
    ├── sapphire-ledger-cli/    # The command line both editions run
    ├── sapphire-ledger-server/ # The app server (`sapphire-ledger serve`: sync, MCP over stdio and /mcp)
    ├── sapphire-ledger-api/   # The server's methods, their parameters and results (what a client links)
    ├── sapphire-ledger-core/  # Data model, record I/O, migration, validation, write path
    └── sapphire-ledger-mcp/   # the ledger's MCP tools and server methods (library)
```

## Status

The data model, validation, write path and MCP server work. Either edition
puts one command on PATH, `sapphire-ledger`: the ledger's own verbs, and the
[sapphire-framework](https://github.com/sapphire-turtle/sapphire-framework)
app-server verbs beside them (`serve`, `status`, `service`, `workspace`,
`workgroup`, `device`, `external-device`, `mcp`, …). With the server
running (`sapphire-ledger serve`, or `sapphire-ledger service install`), point an MCP
client at `sapphire-ledger mcp` to read and record entries over stdio, or turn
on its `/mcp` in `config.toml` (`[http.mcp] enabled`) and reach it over HTTP with an external-device
token. Both serve the workspace the server has selected (see
[`docs/design.md`](docs/design.md#mcp-server) for the setup and the auth
model). The desktop app's window (`sapphire-ledger-desktop`) is a client of that
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

The desktop's **Charts** screen (on the left rail) lists saved *panels* and
draws the selected one: a panel is a chart saved as data in `panels/{id}.toml`
(its period and the queries of each plot), so a new chart is a new file, not a
new feature. A new budget makes its pace and balance panels, and **New panel**
offers templates (budget pace, budget balance, expenses by account, net worth, income and
expenses) to start an editor from. AI agents use the same panels as query
presets: `list_panels` shows them, `run_panel` runs one, and `query_series`
covers what none fits. On the command line, `sapphire-ledger panel list` and
`sapphire-ledger panel run <name>` print them as tables (`panel run` replaces
`budget report`).

A ledger made by an older build (every record TOML) has to be upgraded once
with `sapphire-ledger migrate` (`--dry-run` to preview); commit it first.

## License

This repository contains components under different licenses:

| Component | License |
|-----------|---------|
| `sapphire-ledger-api` | MIT OR Apache-2.0 |
| `sapphire-ledger-cli` | MIT OR Apache-2.0 |
| `sapphire-ledger-core` | MIT OR Apache-2.0 |
| `sapphire-ledger-headless` | MIT OR Apache-2.0 |
| `sapphire-ledger-mcp` | MIT OR Apache-2.0 |
| `sapphire-ledger-server` | MIT OR Apache-2.0 |
| `sapphire-ledger-desktop` | GPL-3.0-or-later, with [App Store marketplace exception](desktop/LICENSE.App-Store-Exception) |

The desktop app's binary includes the MIT/Apache-2.0 code of the command line and the
server, so the binary as a whole is distributed under GPL-3.0-or-later (with the same
exception).

See the `LICENSE` (or `LICENSE-MIT` / `LICENSE-APACHE`) file in each component's directory for the full license text.
