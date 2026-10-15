//! The one binary's command line: the ledger's verbs and the framework's,
//! flat at the top level (#19).

use clap::Parser as _;
use sapphire_ledger::cli::{Cli, Command, FrameworkCommand};

fn parse(args: &[&str]) -> Cli {
    Cli::try_parse_from(std::iter::once("sapphire-ledger").chain(args.iter().copied()))
        .unwrap_or_else(|e| panic!("{args:?} should parse: {e}"))
}

#[test]
fn a_bare_invocation_prints_the_help() {
    // The framework's recommendation: nothing starts on a bare invocation;
    // the service manager starts the binary with `serve`.
    let err = Cli::try_parse_from(["sapphire-ledger"])
        .err()
        .expect("a bare invocation does not parse");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    );
    assert!(matches!(
        parse(&["serve"]).command,
        Command::Framework(FrameworkCommand::Serve)
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
            matches!(parse(args).command, Command::Framework(_)),
            "{args:?} should be a framework verb"
        );
    }
}

#[test]
fn the_ledger_verbs_still_parse_beside_them() {
    assert!(matches!(parse(&["check"]).command, Command::Check));
    assert!(matches!(
        parse(&["--ledger-dir", "/tmp/l", "tx", "list"]).command,
        Command::Tx(_)
    ));
}
