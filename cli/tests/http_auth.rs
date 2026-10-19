//! `/api/series` on a running ledger server, behind the external-device
//! token: that `serve::build` mounts the route on the HTTP listener, and that
//! the framework's token check comes before it — a request without a token is
//! refused before the route can say that no workspace is selected.
//!
//! One test: `LEDGER_CTX`'s directories are first-writer-wins per process.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use sapphire_framework::ipc::Endpoint;
use sapphire_framework::keys::{AuthConfig, Authenticated, GrainId, Verdict, Verifier};
use sapphire_framework::server::{HttpConfig, ServerConfig};
use sapphire_ledger::serve;
use sapphire_ledger_core::{AppKind, LEDGER_CTX};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

/// Accepts the one token `good-token`.
struct OneToken;

#[async_trait::async_trait]
impl Verifier for OneToken {
    async fn verify(&self, token: &str) -> Verdict {
        if token == "good-token" {
            Verdict::Accepted(Authenticated {
                id: GrainId::NIL,
                name: "grafana".into(),
            })
        } else {
            Verdict::Refused
        }
    }
}

fn free_port() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

/// The raw response to `GET /api/series…`, retried until the listener is up.
async fn get(addr: SocketAddr, token: Option<&str>) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut stream = loop {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(stream) => break stream,
            Err(e) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the HTTP listener never came up: {e}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    };
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET /api/series?measure=flow&interval=month&from=2026-05-01&to=2026-05-31 HTTP/1.1\r\n\
         Host: 127.0.0.1\r\n{auth}Connection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

fn status(response: &str) -> u16 {
    response
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("not an HTTP response: {response}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn the_route_is_mounted_behind_the_token() {
    let tmp = tempfile::tempdir().unwrap();
    for cat in ["CACHE", "DATA", "CONFIG"] {
        // SAFETY: the only test in this binary, before any thread reads them.
        unsafe {
            std::env::set_var(
                format!("SAPPHIRE_LEDGER_{cat}_DIR"),
                tmp.path().join(cat.to_lowercase()),
            )
        };
    }
    sapphire_ledger_core::init_app_context(AppKind::Server);

    let endpoint = Endpoint::in_dir(LEDGER_CTX.app_name, tmp.path().join("run"));
    std::fs::create_dir_all(tmp.path().join("run")).unwrap();
    let addr = free_port();
    let server = serve::build()
        .endpoint(endpoint)
        .config(ServerConfig {
            http: HttpConfig {
                listen: Some(addr),
                ..Default::default()
            },
            ..Default::default()
        })
        .http_auth(AuthConfig::new(Arc::new(OneToken)));
    tokio::spawn(async move { server.run().await.unwrap() });

    assert_eq!(status(&get(addr, None).await), 401);
    assert_eq!(status(&get(addr, Some("bad")).await), 401);

    // Through the token to the route, which has no workspace to answer from.
    let response = get(addr, Some("good-token")).await;
    assert_eq!(status(&response), 503, "got: {response}");
    assert!(response.contains("workspace select"), "got: {response}");
}
