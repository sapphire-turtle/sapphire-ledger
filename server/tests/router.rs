//! The composition this server exists for: `/mcp`, and only `/mcp`, behind
//! the framework's token check.
//!
//! `protect`'s own semantics, and the bridge's answers, are the framework's to
//! test. What these pin is the wiring: that `build_router` applies `protect`
//! at all, that an accepted token reaches the MCP service, and that nothing
//! but `/mcp` is mounted. Each is a one-line mistake that leaves every other
//! test passing.
//!
//! A test verifier stands in for the bridge, so these drive
//! [`serve::build_router`] with `tower::ServiceExt::oneshot`: no socket, no
//! bridge, no port.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt as _;
use sapphire_framework::keys::{AuthConfig, Authenticated, GrainId, Verdict, Verifier};
use tower::ServiceExt as _;

use sapphire_ledger_server::serve;

/// The one token the test verifier accepts.
const TOKEN: &str = "sapphire-ed-test";

/// Accepts [`TOKEN`] and refuses everything else, the way the bridge answers
/// for an external device allowed to use this app.
struct OneToken;

#[async_trait::async_trait]
impl Verifier for OneToken {
    async fn verify(&self, token: &str) -> Verdict {
        if token == TOKEN {
            Verdict::Accepted(Authenticated {
                id: GrainId::from_u64_lossy(1),
                name: "laptop".to_owned(),
            })
        } else {
            Verdict::Refused
        }
    }
}

/// A bridge that cannot be asked.
struct Down;

#[async_trait::async_trait]
impl Verifier for Down {
    async fn verify(&self, _token: &str) -> Verdict {
        Verdict::Unavailable
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    ledger: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let ledger = dir.path().join("ledger");
        sapphire_ledger_core::init_workspace(&ledger, "JPY").expect("init");
        Self { _dir: dir, ledger }
    }

    fn router_with(&self, auth: AuthConfig) -> axum::Router {
        serve::build_router(
            auth,
            &self.ledger,
            &[],
            tokio_util::sync::CancellationToken::new(),
        )
        .expect("router")
    }

    fn router(&self) -> axum::Router {
        self.router_with(AuthConfig::new(Arc::new(OneToken)))
    }
}

/// Send one request and report its status. `Host: localhost` because rmcp's
/// allowlist is loopback-only here, and a 403 from that guard would be
/// indistinguishable from the auth answers these tests are about.
async fn status(router: axum::Router, uri: &str, bearer: Option<&str>) -> StatusCode {
    let mut builder = Request::builder().uri(uri).header("host", "localhost");
    if let Some(token) = bearer {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let request = builder.body(Body::empty()).expect("request");
    let response = router.oneshot(request).await.expect("response");
    let status = response.status();
    // Drain the body so a handler that streams cannot leave the response
    // half-built, and so a failure has something to print.
    let _ = response.into_body().collect().await;
    status
}

#[tokio::test]
async fn mcp_without_a_bearer_is_refused() {
    let f = Fixture::new();
    assert_eq!(
        status(f.router(), "/mcp", None).await,
        StatusCode::UNAUTHORIZED,
        "an unauthenticated /mcp request must not reach the ledger -- this is \
         the assertion that catches `protect` being dropped from `build_router`"
    );
}

#[tokio::test]
async fn mcp_with_a_bogus_bearer_is_refused() {
    let f = Fixture::new();
    assert_eq!(
        status(f.router(), "/mcp", Some("sapphire-ed-not-a-real-token")).await,
        StatusCode::UNAUTHORIZED,
        "a token the verifier refuses must be refused"
    );
}

#[tokio::test]
async fn mcp_with_an_accepted_bearer_gets_past_the_check() {
    let f = Fixture::new();
    let status = status(f.router(), "/mcp", Some(TOKEN)).await;
    // What rmcp answers a bare GET is rmcp's business -- it may be 405, 406
    // or a stream. The claim here is only that the check let it through,
    // which is what makes the 401s above evidence of authentication rather
    // than of a router that refuses everything.
    assert_ne!(status, StatusCode::UNAUTHORIZED);
    assert_ne!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn only_mcp_is_mounted() {
    let f = Fixture::new();
    // Authenticated on purpose: `protect` layers the whole router, so an
    // unauthenticated request would 401 before routing and tell us nothing
    // about whether the route exists.
    assert_eq!(
        status(f.router(), "/rpc", Some(TOKEN)).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_bridge_that_cannot_be_asked_is_503() {
    let f = Fixture::new();
    assert_eq!(
        status(
            f.router_with(AuthConfig::new(Arc::new(Down))),
            "/mcp",
            Some(TOKEN)
        )
        .await,
        StatusCode::SERVICE_UNAVAILABLE,
        "with the bridge down the token may be fine: 503, never 401 or through"
    );
}

#[tokio::test]
async fn no_verifier_closes_the_router() {
    let f = Fixture::new();
    assert_eq!(
        status(
            f.router_with(AuthConfig::unconfigured()),
            "/mcp",
            Some(TOKEN)
        )
        .await,
        StatusCode::SERVICE_UNAVAILABLE,
        "authentication that is not wired must close the router, never open it"
    );
}
