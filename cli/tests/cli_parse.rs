//! The one binary's command line: the ledger's verbs and the framework's,
//! flat at the top level (#19).

use clap::Parser as _;
use sapphire_ledger::cli::{Cli, Command, FrameworkCommand};

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("sapphire-ledger").chain(args.iter().copied()))
        .unwrap_or_else(|e| panic!("{args:?} should parse: {e}"))
}

#[test]
fn a_bare_invocation_is_the_server() {
    // `None` is what `main` runs as `serve`; the service manager starts the
    // binary with `serve`, which must land in the same place.
    assert!(parse(&[]).command.is_none());
    assert!(matches!(
        parse(&["serve"]).command,
        Some(Command::Framework(FrameworkCommand::Serve))
    ));
}

#[test]
fn the_framework_verbs_sit_at_the_top_level() {
    for args in [
        &["status"][..],
        &["service", "status"],
        &["workspace", "show"],
        &["workspace", "select", "/tmp/ledger"],
        &["workgroup", "list"],
        &["device", "list"],
        &["external-device", "list"],
        &["mcp"],
    ] {
        assert!(
            matches!(parse(args).command, Some(Command::Framework(_))),
            "{args:?} should be a framework verb"
        );
    }
}

#[test]
fn the_ledger_verbs_still_parse_beside_them() {
    assert!(matches!(parse(&["check"]).command, Some(Command::Check)));
    assert!(matches!(
        parse(&["--ledger-dir", "/tmp/l", "tx", "list"]).command,
        Some(Command::Tx(_))
    ));
}
