# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/v0.1.0...v0.2.0) - 2026-10-20

### Added

- let cargo binstall install the release binaries
- *(cli)* [**breaking**] panel list and panel run in place of budget report
- *(mcp)* query_series and panels as query presets, in place of budget_report
- *(api)* series query and panel types, and the panel templates
- *(cli)* budget report
- *(mcp)* budget tools and update_account for agents
- *(api)* [**breaking**] default tags and budgets in the ledger's API, version 2
- *(core)* copy accounts' default tags into new transactions

### Fixed

- budget edits carry template panels along; budget opening counts sub-accounts

### Other

- note that any MCP client works, not only sapphire-agent
- Merge pull request #46 from sapphire-turtle/feat/binstall
- the series query, panels and the Charts screen

## [0.1.0](https://github.com/sapphire-turtle/sapphire-ledger/releases/tag/v0.1.0) - 2026-10-15

### Added

- *(cli)* [**breaking**] print the help on a bare invocation instead of starting the server
- [**breaking**] accounts and transactions as Markdown with YAML front matter
- *(desktop)* the books and sync, as a client of the app server
- *(mcp)* move onto sapphire-framework's MCP (sapphire-framework#233)
- fold the CLI and the server into one sapphire-ledger binary ([#19](https://github.com/sapphire-turtle/sapphire-ledger/pull/19))
- *(cli)* add the write commands, so a person can record without an MCP client ([#15](https://github.com/sapphire-turtle/sapphire-ledger/pull/15))

### Other

- release with release-plz
- split the IPC API into sapphire-ledger-api
- *(license)* desktop under GPL-3.0-or-later; no license at the root
- *(deps)* sapphire-framework 0.19.0; reconnect to the bridge on its own
- Merge pull request #21 from sapphire-turtle/chore/framework-0.16
- unify the tagline — built on sapphire-framework, human-agent collaboration
- adopt repo-layout and language conventions; add CONTRIBUTING and README.ja
- /mcp over HTTP, authenticated per device ([#14](https://github.com/sapphire-turtle/sapphire-ledger/pull/14))
- Phase 1: record ids, the core write path, and an MCP server ([#9](https://github.com/sapphire-turtle/sapphire-ledger/pull/9))
- *(cli)* rename binary from `sale` to `sapphire-ledger`
- scaffold Cargo workspace with core/mcp/cli/desktop crates
