use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_sapphire-ledger")
}

/// `mcp` is the framework's now: it serves the running server's workspace,
/// so the old `--init` (create a ledger where the agent stands) is gone.
#[test]
fn mcp_is_the_frameworks_subcommand() {
    let out = Command::new(bin())
        .args(["mcp", "--help"])
        .output()
        .expect("run");
    assert!(out.status.success(), "mcp --help failed");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("--init"), "got: {text}");
}

#[test]
fn check_reports_a_clean_empty_ledger() {
    let dir = tempfile::tempdir().expect("tempdir");
    let init = Command::new(bin())
        .arg("init")
        .arg(dir.path())
        .output()
        .expect("run init");
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let out = Command::new(bin())
        .arg("--ledger-dir")
        .arg(dir.path())
        .arg("check")
        .output()
        .expect("run check");
    assert!(
        out.status.success(),
        "check failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("OK"));
}
