# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/sapphire-ledger-mcp-v0.1.0...sapphire-ledger-mcp-v0.2.0) - 2026-10-25

### Added

- *(mcp)* query_series and panels as query presets, in place of budget_report
- *(api)* series query and panel types, and the panel templates
- *(mcp)* budget tools and update_account for agents
- *(mcp)* budget and account-update methods on the app server
- *(api)* [**breaking**] default tags and budgets in the ledger's API, version 2
- *(core)* copy accounts' default tags into new transactions

### Fixed

- budget edits carry template panels along; budget opening counts sub-accounts

### Other

- say update ops rewrite from known fields

## [0.1.0](https://github.com/sapphire-turtle/sapphire-ledger/releases/tag/sapphire-ledger-mcp-v0.1.0) - 2026-10-15

### Added

- [**breaking**] accounts and transactions as Markdown with YAML front matter
- *(desktop)* the books and sync, as a client of the app server
- *(mcp)* move onto sapphire-framework's MCP (sapphire-framework#233)
- fold the CLI and the server into one sapphire-ledger binary ([#19](https://github.com/sapphire-turtle/sapphire-ledger/pull/19))

### Other

- split the IPC API into sapphire-ledger-api
- *(deps)* sapphire-framework 0.18.0, rmcp 3.5
- adopt repo-layout and language conventions; add CONTRIBUTING and README.ja
