# sapphire-ledger-server

The app server of [sapphire-ledger](https://github.com/sapphire-turtle/sapphire-ledger):
the one process that serves the ledger's workspace, syncs it through the host's
`sapphire-bridge`, answers the ledger's methods and MCP tools, and watches for an id
claimed by more than one file.

It is a library, run as `sapphire-ledger serve` by either edition
(`sapphire-ledger-headless` or the desktop app).

## License

MIT OR Apache-2.0.
