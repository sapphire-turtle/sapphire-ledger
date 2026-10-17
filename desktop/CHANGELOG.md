# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/desktop-v0.1.0...desktop-v0.2.0) - 2026-10-17

### Added

- *(desktop)* Charts screen comparing a budget with spending and balance
- *(desktop)* default tags on accounts and in the transaction form
- *(api)* [**breaking**] default tags and budgets in the ledger's API, version 2
- *(desktop)* lay the window out with the framework's Shell

### Fixed

- final review — balance plot scale, aligned axes, month overflow, ops order
- *(desktop)* ignore loads overtaken by a budget or month change
- *(desktop)* merge default tags only when the account actually changes
- *(desktop)* point to Settings and the status bar, not the Sync tab

### Other

- *(desktop)* start through sapphire-framework's gui::app::run

## [0.1.0](https://github.com/sapphire-turtle/sapphire-ledger/releases/tag/desktop-v0.1.0) - 2026-10-15

### Added

- [**breaking**] accounts and transactions as Markdown with YAML front matter
- *(desktop)* the books and sync, as a client of the app server

### Other

- split the IPC API into sapphire-ledger-api
- *(license)* desktop under GPL-3.0-or-later; no license at the root
- adopt repo-layout and language conventions; add CONTRIBUTING and README.ja
