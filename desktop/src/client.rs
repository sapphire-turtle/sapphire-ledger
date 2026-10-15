//! The desktop's link to the ledger's methods on the running app server.
//!
//! The framework's `FrameworkClient` covers the framework's half (status,
//! workspaces, sync); the ledger's own `sapphire-ledger.*` methods go over a
//! connection of this app's own. It is dialled on the first call and again
//! after it drops — the server may start after the window opens, or restart
//! while it is open — and a call is never retried, so a write is never
//! applied twice.

use std::sync::Arc;

use sapphire_framework::backend::protocol::API_VERSION;
use sapphire_framework::ipc::{self, Client, ClientInfo, Endpoint};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// What this client reports in the IPC handshake.
const CLIENT_KIND: &str = "desktop";

pub struct LedgerClient {
    app: &'static str,
    version: &'static str,
    endpoint: Endpoint,
    held: tokio::sync::Mutex<Option<Arc<Client>>>,
}

impl LedgerClient {
    pub fn new(app: &'static str, version: &'static str, endpoint: Endpoint) -> LedgerClient {
        LedgerClient {
            app,
            version,
            endpoint,
            held: tokio::sync::Mutex::new(None),
        }
    }

    /// Call `method` and return its text: a confirmation for a write, JSON
    /// for a read. The error is a message to show as it is.
    pub async fn call(&self, method: &str, params: impl Serialize) -> Result<String, String> {
        let client = self.client().await?;
        match client.call::<_, String>(method, params).await {
            Ok(text) => Ok(text),
            Err(ipc::Error::Rpc(rpc)) => Err(rpc.message),
            Err(ipc::Error::Closed | ipc::Error::Io(_)) => {
                self.forget(&client).await;
                Err(format!(
                    "the connection to the {} server was lost; try again",
                    self.app
                ))
            }
            Err(err) => Err(err.to_string()),
        }
    }

    /// Call a read method and parse the JSON it answers with.
    pub async fn read<R: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<R, String> {
        let text = self.call(method, params).await?;
        serde_json::from_str(&text).map_err(|e| format!("unexpected answer to {method}: {e}"))
    }

    async fn client(&self) -> Result<Arc<Client>, String> {
        let mut held = self.held.lock().await;
        if let Some(client) = held.as_ref().filter(|c| !c.is_closed()) {
            return Ok(Arc::clone(client));
        }
        let info = ClientInfo {
            kind: CLIENT_KIND.to_owned(),
            version: self.version.to_owned(),
            api: API_VERSION,
            app_api: Some(sapphire_ledger_api::API_VERSION),
            pid: std::process::id(),
        };
        let (client, _) = ipc::connect_or_absent(&self.endpoint, self.app, info)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "no {} server is running; start it from the Sync tab, or with `{} serve`",
                    self.app, self.app
                )
            })?;
        let client = Arc::new(client);
        *held = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn forget(&self, lost: &Arc<Client>) {
        let mut held = self.held.lock().await;
        if held.as_ref().is_some_and(|h| Arc::ptr_eq(h, lost)) {
            *held = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use sapphire_framework::backend::protocol as proto;
    use sapphire_ledger_api::{
        self as api, AddAccountParams, EmptyParams, PostingHit, QueryPostingsParams,
        ValidationIssue,
    };
    use sapphire_ledger_core::{AppKind, LEDGER_CTX};

    use super::*;
    use crate::form::{PostingRow, TxForm};

    /// The desktop's reads and writes against a running ledger server: that
    /// what the methods answer parses into the types the view shows, and
    /// that a form's parameters are what the server accepts.
    ///
    /// One test: `LEDGER_CTX`'s directories are first-writer-wins per process.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_desktop_reads_and_writes_the_servers_books() {
        let tmp = tempfile::tempdir().unwrap();
        for cat in ["CACHE", "DATA", "CONFIG"] {
            // SAFETY: the only test in this binary that touches the
            // environment, before the server starts.
            unsafe {
                std::env::set_var(
                    format!("SAPPHIRE_LEDGER_{cat}_DIR"),
                    tmp.path().join(cat.to_lowercase()),
                )
            };
        }
        sapphire_ledger_core::init_app_context(AppKind::Server);
        std::fs::create_dir_all(tmp.path().join("run")).unwrap();
        let endpoint = Endpoint::in_dir(LEDGER_CTX.app_name, tmp.path().join("run"));

        let client = LedgerClient::new(LEDGER_CTX.app_name, "0.0.0", endpoint.clone());
        let err = client
            .call(api::LIST_ACCOUNTS, EmptyParams {})
            .await
            .unwrap_err();
        assert!(
            err.contains("no sapphire-ledger server is running"),
            "got: {err}"
        );

        let server = sapphire_ledger::serve::build().endpoint(endpoint.clone());
        let task = tokio::spawn(async move { server.run().await.unwrap() });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !ipc::probe(&endpoint).await.unwrap() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the server never listened"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let ledger = tmp.path().join("ledger");
        std::fs::create_dir_all(&ledger).unwrap();
        let _: proto::WorkspaceInitResult = client
            .client()
            .await
            .unwrap()
            .call(
                proto::WORKSPACE_INIT,
                proto::WorkspaceInitParams {
                    dir: ledger.clone(),
                },
            )
            .await
            .unwrap();
        sapphire_ledger_core::init_workspace(&ledger, "JPY").unwrap();

        for (name, ty) in [("Expenses:Food", "Expense"), ("Assets:Cash", "Asset")] {
            client
                .call(
                    api::ADD_ACCOUNT,
                    AddAccountParams {
                        name: name.into(),
                        account_type: ty.into(),
                        currencies: vec![],
                        opened_at: "2026-01-01".into(),
                        description: None,
                    },
                )
                .await
                .expect("add_account");
        }
        let mut form = TxForm::new("2026-05-21".into(), "JPY");
        form.narration = "groceries".into();
        form.postings = vec![
            PostingRow {
                account: "Expenses:Food".into(),
                amount: "+1,200".into(),
                currency: "JPY".into(),
                memo: String::new(),
            },
            PostingRow {
                account: "Assets:Cash".into(),
                amount: "-1200".into(),
                currency: "JPY".into(),
                memo: String::new(),
            },
        ];
        let created = client
            .call(api::ADD_TRANSACTION, form.to_params().unwrap())
            .await
            .expect("the form's parameters are what the server takes");
        assert!(created.contains("created transaction"), "got: {created}");

        let accounts: Vec<api::Account> = client
            .read(api::LIST_ACCOUNTS, EmptyParams {})
            .await
            .expect("accounts parse");
        assert_eq!(accounts.len(), 2);
        let postings: Vec<PostingHit> = client
            .read(
                api::QUERY_POSTINGS,
                QueryPostingsParams {
                    account: None,
                    currency: None,
                    date_from: None,
                    date_to: None,
                },
            )
            .await
            .expect("postings parse");
        assert_eq!(postings.len(), 2);
        let issues: Vec<ValidationIssue> = client
            .read(api::VALIDATE_WORKSPACE, EmptyParams {})
            .await
            .expect("issues parse");
        assert!(issues.is_empty(), "{issues:?}");

        let _: serde_json::Value = client
            .client()
            .await
            .unwrap()
            .call(ipc::SHUTDOWN_METHOD, serde_json::json!({}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("the server stopped")
            .unwrap();
    }
}
