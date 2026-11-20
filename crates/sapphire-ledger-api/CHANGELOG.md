# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [4.0.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/sapphire-ledger-api-v3.0.0...sapphire-ledger-api-v4.0.0) - 2026-11-20

### Other

- [**breaking**] dates and timestamps are jiff's, not chrono's (RUSTSEC-2026-0543)

### Breaking

- `created_at` and `updated_at` are `jiff::Timestamp`, not chrono's `DateTime<FixedOffset>`, and dates are `jiff::civil::Date`: chrono is unmaintained (RUSTSEC-2026-0543). The JSON is unchanged: RFC 3339 in the server's offset, and `YYYY-MM-DD`. `time` has the helpers.
- `sapphire-framework-server-api` 7 (re-exported as `server`), with sapphire-framework 0.29: the IPC handshake's API version 7.

## [3.0.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/sapphire-ledger-api-v2.1.0...sapphire-ledger-api-v3.0.0) - 2026-11-03

### Changed

- **Breaking:** `server` re-exports sapphire-framework-server-api 6 (was 5), so the major and `API_VERSION` are 3: a client and server of different majors refuse the handshake.

## [2.1.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/sapphire-ledger-api-v2.0.0...sapphire-ledger-api-v2.1.0) - 2026-10-25

### Added

- *(mcp)* add_transactions, many transactions in one call, all or none

### Other

- Merge pull request #41 from sapphire-turtle/release-plz/2026-10-16T04-45-46Z

## [2.0.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/sapphire-ledger-api-v1.0.0...sapphire-ledger-api-v2.0.0) - 2026-10-25

### Added

- *(desktop)* [**breaking**] the Charts screen draws panels; budgets get their panels on creation
- *(api)* series query and panel types, and the panel templates
- *(api)* [**breaking**] default tags and budgets in the ledger's API, version 2

## [1.0.0](https://github.com/sapphire-turtle/sapphire-ledger/releases/tag/sapphire-ledger-api-v1.0.0) - 2026-10-15

### Other

- release with release-plz
- split the IPC API into sapphire-ledger-api
