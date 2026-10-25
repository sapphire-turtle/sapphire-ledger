# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/sapphire-ledger-core-v0.1.0...sapphire-ledger-core-v0.2.0) - 2026-10-25

### Added

- *(mcp)* add_transactions, many transactions in one call, all or none
- *(cli)* [**breaking**] panel list and panel run in place of budget report
- *(core)* panels — saved charts of series queries, with their ops and checks
- *(core)* a generic series query — flow, balance and budget lines
- *(core)* a budget's month as a day-by-day report
- *(core)* budget records, their writes and their validation
- *(core)* update an account's default tags
- *(core)* copy accounts' default tags into new transactions

### Fixed

- budget edits carry template panels along; budget opening counts sub-accounts
- final review — balance plot scale, aligned axes, month overflow, ops order

## [0.1.0](https://github.com/sapphire-turtle/sapphire-ledger/releases/tag/sapphire-ledger-core-v0.1.0) - 2026-10-15

### Added

- [**breaking**] accounts and transactions as Markdown with YAML front matter
- *(desktop)* the books and sync, as a client of the app server
- fold the CLI and the server into one sapphire-ledger binary ([#19](https://github.com/sapphire-turtle/sapphire-ledger/pull/19))

### Other

- *(core)* give init_completes_a_bare_marker a tempfile directory
- Merge pull request #31 from sapphire-turtle/chore/framework-0.20
- *(core)* make each test's directory with tempfile
- *(deps)* sapphire-framework 0.20.0; init completes a bare marker
- *(deps)* move to sapphire-framework 0.16.0
- adopt repo-layout and language conventions; add CONTRIBUTING and README.ja
