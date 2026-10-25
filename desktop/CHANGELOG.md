# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
