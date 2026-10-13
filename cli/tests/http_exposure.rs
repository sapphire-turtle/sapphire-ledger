use std::net::SocketAddr;

use sapphire_ledger::serve;

fn addr(s: &str) -> SocketAddr {
    s.parse().expect("addr")
}

#[test]
fn loopback_needs_no_allowed_host() {
    serve::check_exposure(addr("127.0.0.1:3173"), &[])
        .expect("a loopback bind is complete on its own");
}

#[test]
fn a_wide_bind_without_an_allowed_host_is_refused() {
    let err = serve::check_exposure(addr("0.0.0.0:3173"), &[])
        .expect_err("a wide bind with no allowlist serves 403 to everyone");
    let msg = err.to_string();
    assert!(
        msg.contains("allowed_hosts"),
        "the error must name the flag that fixes it, got: {msg}"
    );
}

#[test]
fn a_wide_bind_with_an_allowed_host_is_accepted() {
    serve::check_exposure(addr("0.0.0.0:3173"), &["ledger.example.net".to_string()])
        .expect("a named host makes a wide bind usable");
}

#[test]
fn a_blank_allowed_host_does_not_count() {
    serve::check_exposure(addr("0.0.0.0:3173"), &["  ".to_string()])
        .expect_err("whitespace is not a hostname");
}
