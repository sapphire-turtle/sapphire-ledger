//! The ledger's MCP tools against a running ledger server, over IPC: the path
//! `sapphire-ledger mcp` takes, and the one `/mcp` takes in-process.
//!
//! What these pin is the ledger's half of the wiring — that `serve::build`
//! registers the ledger's methods, that the tools reach them, and that they
//! work on the workspace the server has selected. The transports, the token
//! check on `/mcp` and the framework's own tools are the framework's to test.
//!
//! One test: `LEDGER_CTX`'s directories are first-writer-wins per process.

use std::time::Duration;

use sapphire_framework::backend::protocol as proto;
use sapphire_framework::ipc::{self, ClientInfo, Endpoint};
use sapphire_framework::mcp::ToolContext;
use sapphire_framework::mcp::rmcp::handler::server::wrapper::Parameters;
use sapphire_ledger::serve;
use sapphire_ledger_api::{
    AddAccountParams, AddBudgetParams, AddTransactionParams, BudgetReportParams, EmptyParams,
    PostingParam, QueryPostingsParams,
};
use sapphire_ledger_core::{AppKind, LEDGER_CTX};
use sapphire_ledger_mcp::LedgerTools;

async fn wait_until_listening(endpoint: &Endpoint) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !ipc::probe(endpoint).await.unwrap() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the server never started listening"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn client(endpoint: &Endpoint) -> ipc::Client {
    let info = ClientInfo {
        kind: "test".into(),
        version: "0.0.0".into(),
        api: proto::API_VERSION,
        app_api: None,
        pid: std::process::id(),
    };
    ipc::connect_or_absent(endpoint, LEDGER_CTX.app_name, info)
        .await
        .unwrap()
        .expect("the server is listening")
        .0
}

fn posting(account: &str, amount: &str) -> PostingParam {
    PostingParam {
        account: account.into(),
        amount: amount.into(),
        currency: "JPY".into(),
        price_value: None,
        price_currency: None,
        memo: None,
    }
}

fn account(name: &str, ty: &str) -> Parameters<AddAccountParams> {
    Parameters(AddAccountParams {
        default_tags: vec![],
        name: name.into(),
        account_type: ty.into(),
        currencies: vec![],
        opened_at: "2026-01-01".into(),
        description: None,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tools_work_on_the_servers_workspace_over_ipc() {
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
    let server = serve::build().endpoint(endpoint.clone());
    let task = tokio::spawn(async move { server.run().await.unwrap() });
    wait_until_listening(&endpoint).await;

    let tools = LedgerTools::new(ToolContext::dial(
        LEDGER_CTX.app_name,
        serve::VERSION,
        Some(sapphire_ledger_api::API_VERSION),
        endpoint.clone(),
    ));

    // No workspace yet: the ledger's own message, not the IPC layer's.
    let err = tools
        .list_accounts(Parameters(EmptyParams {}))
        .await
        .expect_err("nothing is selected");
    assert!(err.contains("workspace select"), "got: {err}");

    let ledger = tmp.path().join("ledger");
    std::fs::create_dir_all(&ledger).unwrap();
    let c = client(&endpoint).await;
    let _: proto::WorkspaceInitResult = c
        .call(
            proto::WORKSPACE_INIT,
            proto::WorkspaceInitParams {
                dir: ledger.clone(),
            },
        )
        .await
        .unwrap();
    sapphire_ledger_core::init_workspace(&ledger, "JPY").unwrap();

    tools
        .add_account(account("Expenses:Food", "Expense"))
        .await
        .expect("add_account");
    tools
        .add_account(account("Assets:Cash", "Asset"))
        .await
        .expect("add_account");
    let created = tools
        .add_transaction(Parameters(AddTransactionParams {
            apply_default_tags: true,
            date: "2026-05-21".into(),
            narration: "groceries".into(),
            payee: None,
            tags: vec![],
            status: None,
            postings: vec![
                posting("Expenses:Food", "1200"),
                posting("Assets:Cash", "-1200"),
            ],
            comment: None,
        }))
        .await
        .expect("add_transaction");
    assert!(created.contains("created transaction"), "got: {created}");
    assert!(
        std::fs::read_dir(ledger.join("transactions"))
            .unwrap()
            .next()
            .is_some(),
        "the record is in the selected workspace"
    );

    let hits = tools
        .query_postings(Parameters(QueryPostingsParams {
            account: Some("Expenses:Food".into()),
            currency: None,
            date_from: None,
            date_to: None,
        }))
        .await
        .expect("query_postings");
    assert!(hits.contains("groceries"), "got: {hits}");

    tools
        .add_budget(Parameters(AddBudgetParams {
            name: "生活費".into(),
            tag: "living".into(),
            currency: "JPY".into(),
            monthly_amount: "100000".into(),
            balance_accounts: vec!["Assets:Cash".into()],
        }))
        .await
        .expect("add_budget");
    let report = tools
        .budget_report(Parameters(BudgetReportParams {
            budget: "生活費".into(),
            month: "2026-05".into(),
        }))
        .await
        .expect("budget_report");
    assert!(
        report.contains("\"daily_budget\": \"3225.81\""),
        "got: {report}"
    );

    // A refused write comes back as the ledger's reason.
    let err = tools
        .add_transaction(Parameters(AddTransactionParams {
            apply_default_tags: true,
            date: "2026-05-21".into(),
            narration: "wrong".into(),
            payee: None,
            tags: vec![],
            status: None,
            postings: vec![
                posting("Expenses:Food", "1200"),
                posting("Assets:Cash", "-999"),
            ],
            comment: None,
        }))
        .await
        .expect_err("unbalanced");
    assert!(err.contains("does not balance"), "got: {err}");
    assert!(
        !err.contains("remote error"),
        "the IPC framing leaked: {err}"
    );

    let _: serde_json::Value = c
        .call(ipc::SHUTDOWN_METHOD, serde_json::json!({}))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("the server stopped")
        .unwrap();
}
