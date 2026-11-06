# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/v0.3.1...v0.4.0) - 2026-11-06

### Added

- On macOS, the app bundle put in `~/Applications` when Sapphire Ledger is added to the app menu has the mark on the brand's tile, as the package's icns, rather than the bare mark on the system's plate (corundum-framework 0.11).

### Changed

- **Breaking:** sapphire-framework 0.28 and corundum-framework 0.11. `update` installs only a release file signed with sapphire-turtle's key: it fetches the file's `<file>.sig` from the same release and refuses a file without one, or whose signature does not match (sapphire-turtle/sapphire-framework#402).
- Each release file — the bare binaries for servers, the install script and `cargo binstall` — comes with its `<file>.sig`, made with sapphire-turtle's minisign key, which `update` checks (sapphire-turtle/sapphire-framework#402).

### Fixed

- On Windows, `sapphire-ledger-desktop.exe` carries the app icon, in the brand colour, so Explorer and the taskbar show Sapphire Ledger's icon rather than a generic one.

## [0.3.1](https://github.com/sapphire-turtle/sapphire-ledger/compare/v0.3.0...v0.3.1) - 2026-11-04

### Fixed

- On Windows, keeping the app server running (the first start's offer, or `service install`) no longer fails with "access is denied": the service is now a logon task for the installing user, which needs no administrator (sapphire-framework 0.27, corundum 0.9, sapphire-turtle/corundum-framework#105).
- On Windows, applying an update from the app's window installs it and starts the app again; before, the app only closed and was not updated (sapphire-framework 0.27.1, corundum 0.9.1, sapphire-turtle/corundum-framework#109). An app at v0.3.0 or earlier still hits this when it applies this update: install v0.3.1 by hand once, from its `_x64-setup.exe` ([#74](https://github.com/sapphire-turtle/sapphire-ledger/pull/74))

## [0.3.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/v0.2.1...v0.3.0) - 2026-11-03

### Added

- **Breaking:** Sapphire Ledger is distributed as an installer per OS, made with cargo-packager ([#73](https://github.com/sapphire-turtle/sapphire-ledger/pull/73)): `sapphire-ledger-desktop_<version>_x64-setup.exe`, `_aarch64.dmg` and `_x86_64.AppImage`. The installed app updates itself from the latest release, checking each update's signature first.
- **Breaking:** one desktop binary that runs the CLI too; headless, cli and server crates
- the Ledger's picked transaction or account, and its forms, in the side pane
- the app icon

### Changed

- **Breaking:** sapphire-framework 0.24. HTTP clients (`/mcp`) authenticate with API keys kept in the workspace (`<root>/.sapphire-ledger/api_keys/`), managed with `sapphire-ledger api-key list|add|retire|restore|rotate` (`external-device` is a hidden alias for this release). Every key has a profile: the workgroup's existing external-device tokens keep working but become **read-only**, so they may call only the reading tools (`list_accounts`, `query_postings`, `run_panel`, …); an agent that records entries needs a new key, `sapphire-ledger api-key add <name> --profile read-write`. Update `sapphire-bridge` together with the ledger.
- **Breaking:** the `workgroup`, `device` and `embedding` subcommands are gone: use `sapphire-bridge workgroup|device|embedding`.
- Settings link to the Sapphire Bridge window for the workgroup, devices and embedding (sapphire-framework 0.24), and gain an API keys page.
- **Breaking:** sapphire-framework 0.26: `workspace init` and `select` without a directory use the app's default workspace, `<data dir>/workspaces/default`, not the current directory; pass `.` for the old behaviour.

### Fixed

- name each edition's releases, so update installs them

## [0.2.1](https://github.com/sapphire-turtle/sapphire-ledger/compare/desktop-v0.2.0...desktop-v0.2.1) - 2026-10-25

### Other

- updated the following local packages: sapphire-ledger-api, sapphire-ledger-core

## [0.2.0](https://github.com/sapphire-turtle/sapphire-ledger/compare/desktop-v0.1.0...desktop-v0.2.0) - 2026-10-25

### Added

- let cargo binstall install the release binaries
- *(desktop)* edit, preview and delete panels from the Charts screen
- *(desktop)* [**breaking**] the Charts screen draws panels; budgets get their panels on creation
- *(mcp)* query_series and panels as query presets, in place of budget_report
- *(api)* series query and panel types, and the panel templates
- *(desktop)* Charts screen comparing a budget with spending and balance
- *(desktop)* default tags on accounts and in the transaction form
- *(api)* [**breaking**] default tags and budgets in the ledger's API, version 2
- *(desktop)* lay the window out with the framework's Shell

### Fixed

- *(desktop)* Charts editor shows id account filters, closes only its own form, remove glyph
- *(desktop)* preview with no panels, delete ends a preview, preview clears loading
- final review — balance plot scale, aligned axes, month overflow, ops order
- *(desktop)* ignore loads overtaken by a budget or month change
- *(desktop)* merge default tags only when the account actually changes
- *(desktop)* point to Settings and the status bar, not the Sync tab

### Other

- update to sapphire-framework 0.22
- Merge pull request #46 from sapphire-turtle/feat/binstall
- *(desktop)* start through sapphire-framework's gui::app::run

### Changed

- [**breaking**] sapphire-framework 0.22 (egui 0.37): talks only to the ledger's app server, which must be 0.22 too; the theme and language are saved and synced by the framework.

## [0.1.0](https://github.com/sapphire-turtle/sapphire-ledger/releases/tag/desktop-v0.1.0) - 2026-10-15

### Added

- [**breaking**] accounts and transactions as Markdown with YAML front matter
- *(desktop)* the books and sync, as a client of the app server

### Other

- split the IPC API into sapphire-ledger-api
- *(license)* desktop under GPL-3.0-or-later; no license at the root
- adopt repo-layout and language conventions; add CONTRIBUTING and README.ja
