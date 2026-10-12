use sapphire_ledger_server::init;

#[test]
fn init_creates_the_marker_and_every_record_directory() {
    let parent = tempfile::tempdir().expect("tempdir");
    let root = parent.path().join("my-ledger");

    init::run(&root, "JPY").expect("init");

    assert!(
        root.join(".sapphire-ledger").is_dir(),
        "the marker directory is what makes this a workspace at all"
    );
    for dir in ["accounts", "transactions", "assertions", "prices"] {
        assert!(root.join(dir).is_dir(), "{dir}/ is missing");
    }
}

#[test]
fn init_writes_the_requested_base_currency() {
    let parent = tempfile::tempdir().expect("tempdir");
    let root = parent.path().join("my-ledger");

    init::run(&root, "USD").expect("init");

    let config =
        std::fs::read_to_string(root.join(".sapphire-ledger/config.toml")).expect("config");
    assert!(
        config.contains("USD"),
        "--base-currency must reach the config, got: {config}"
    );
}

#[test]
fn init_refuses_an_existing_workspace() {
    let parent = tempfile::tempdir().expect("tempdir");
    let root = parent.path().join("my-ledger");
    init::run(&root, "JPY").expect("first init");

    let err = init::run(&root, "JPY").expect_err("a second init must be refused");
    // Rendered as a chain, not `to_string()`: `run` wraps the core error in
    // context naming the path, which earns its place for the I/O failures core
    // reports without one — but that wrapper is what `to_string()` shows, so the
    // reason itself lives one link down. `main` returns `anyhow::Result`, which
    // prints the whole chain, so this is what an operator actually reads.
    let chain = format!("{err:#}");
    assert!(
        chain.contains("already contains"),
        "the error should say the directory is already a workspace, got: {chain}"
    );
}
