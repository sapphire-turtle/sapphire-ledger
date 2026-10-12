use std::net::SocketAddr;

use sapphire_ledger::http;

fn addr(s: &str) -> SocketAddr {
    s.parse().expect("addr")
}

#[test]
fn loopback_needs_no_allowed_host() {
    http::check_exposure(addr("127.0.0.1:3173"), &[])
        .expect("a loopback bind is complete on its own");
}

#[test]
fn a_wide_bind_without_an_allowed_host_is_refused() {
    let err = http::check_exposure(addr("0.0.0.0:3173"), &[])
        .expect_err("a wide bind with no allowlist serves 403 to everyone");
    let msg = err.to_string();
    assert!(
        msg.contains("allowed_hosts"),
        "the error must name the flag that fixes it, got: {msg}"
    );
}

#[test]
fn a_wide_bind_with_an_allowed_host_is_accepted() {
    http::check_exposure(addr("0.0.0.0:3173"), &["ledger.example.net".to_string()])
        .expect("a named host makes a wide bind usable");
}

#[test]
fn a_blank_allowed_host_does_not_count() {
    http::check_exposure(addr("0.0.0.0:3173"), &["  ".to_string()])
        .expect_err("whitespace is not a hostname");
}

#[test]
fn a_missing_config_file_is_the_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = http::ServerConfig::load(&dir.path().join("server.toml")).expect("defaults");
    assert!(config.http.enabled);
    assert_eq!(config.http.addr, addr(http::DEFAULT_ADDR));
    assert!(config.http.allowed_hosts.is_empty());
}

#[test]
fn the_config_file_sets_the_bind_and_the_hosts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("server.toml");
    std::fs::write(
        &path,
        "[http]\naddr = \"0.0.0.0:4000\"\nallowed_hosts = [\"ledger.example.net\"]\n",
    )
    .expect("write");
    let config = http::ServerConfig::load(&path).expect("parses");
    assert_eq!(config.http.addr, addr("0.0.0.0:4000"));
    assert_eq!(config.http.allowed_hosts, ["ledger.example.net"]);
    http::check_exposure(config.http.addr, &config.http.allowed_hosts).expect("usable");
}

#[test]
fn a_misspelt_key_is_an_error_not_a_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("server.toml");
    std::fs::write(&path, "[http]\nallowed_host = [\"x\"]\n").expect("write");
    http::ServerConfig::load(&path).expect_err("a typo must not silently fall back to loopback");
}
