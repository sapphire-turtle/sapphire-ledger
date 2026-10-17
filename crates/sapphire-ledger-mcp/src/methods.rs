//! The ledger's methods on its app server: what the MCP tools call.
//!
//! Every tool reaches the ledger the way the framework's own tools reach the
//! workspace — over the app server's IPC, as `sapphire-ledger.<tool>` — so one
//! implementation serves the `mcp` subcommand (stdio, dialling the running
//! server) and `/mcp` on the server's HTTP listener alike. [`register`] adds
//! the methods to the server's router; [`crate::tools`] is the other end.
//!
//! [`LedgerMethods`] is the ledger side of one call: an
//! `Arc<Mutex<LedgerState>>`, and a function per method returning the text the
//! agent reads.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use sapphire_framework::ipc::{RequestCtx, Router, RpcError};
use sapphire_ledger_api::{
    self as api, AddAccountParams, AddAssertionParams, AddBudgetParams, AddPriceParams,
    AddTransactionParams, BudgetReportParams, EmptyParams, GetTransactionParams, PostingHit,
    PostingParam, QueryPostingsParams, QueryPricesParams, UpdateAccountParams, UpdateBudgetParams,
};
use sapphire_ledger_core::{LedgerState, ops};
use serde::de::DeserializeOwned;

use crate::wire;

/// Called after a method writes, with every path the write touched.
///
/// One call is one batch: a write producing several files reports them
/// together, so a syncing receiver never sees half of a change.
pub type WriteObserver = Arc<dyn Fn(&[PathBuf]) + Send + Sync>;

/// Where a call finds its ledger: asked once per call.
///
/// The app server's ledger is whatever workspace it has selected, which
/// `workspace select` can change underneath it; an error refuses the call.
pub type StateSource = Arc<dyn Fn() -> std::io::Result<Arc<Mutex<LedgerState>>> + Send + Sync>;

/// Add the ledger's methods to the app server's `router`, each resolving the
/// ledger through `source` when it is called.
pub fn register(router: Router, source: StateSource) -> Router {
    let router = route(
        router,
        api::LIST_ACCOUNTS,
        &source,
        LedgerMethods::list_accounts,
    );
    let router = route(
        router,
        api::GET_TRANSACTION,
        &source,
        LedgerMethods::get_transaction,
    );
    let router = route(
        router,
        api::QUERY_POSTINGS,
        &source,
        LedgerMethods::query_postings,
    );
    let router = route(
        router,
        api::QUERY_PRICES,
        &source,
        LedgerMethods::query_prices,
    );
    let router = route(
        router,
        api::VALIDATE_WORKSPACE,
        &source,
        LedgerMethods::validate_workspace,
    );
    let router = route(
        router,
        api::ADD_ACCOUNT,
        &source,
        LedgerMethods::add_account,
    );
    let router = route(
        router,
        api::ADD_TRANSACTION,
        &source,
        LedgerMethods::add_transaction,
    );
    let router = route(
        router,
        api::ADD_ASSERTION,
        &source,
        LedgerMethods::add_assertion,
    );
    let router = route(
        router,
        api::UPDATE_ACCOUNT,
        &source,
        LedgerMethods::update_account,
    );
    let router = route(
        router,
        api::LIST_BUDGETS,
        &source,
        LedgerMethods::list_budgets,
    );
    let router = route(router, api::ADD_BUDGET, &source, LedgerMethods::add_budget);
    let router = route(
        router,
        api::UPDATE_BUDGET,
        &source,
        LedgerMethods::update_budget,
    );
    let router = route(
        router,
        api::BUDGET_REPORT,
        &source,
        LedgerMethods::budget_report,
    );
    route(router, api::ADD_PRICE, &source, LedgerMethods::add_price)
}

/// Register one method: parse its parameters, then run `method` against the
/// current ledger on the blocking pool — the ledger is files and a std mutex.
///
/// The method's text is the result; its error text is the RPC error's message,
/// which the tool hands to the agent as it is.
fn route<P>(
    router: Router,
    name: &str,
    source: &StateSource,
    method: fn(&LedgerMethods, P) -> Result<String, String>,
) -> Router
where
    P: DeserializeOwned + Send + 'static,
{
    let source = Arc::clone(source);
    router.method(name, move |ctx: RequestCtx| {
        let source = Arc::clone(&source);
        async move {
            let params: P = serde_json::from_value(ctx.params)
                .map_err(|e| RpcError::invalid_params(e.to_string()))?;
            let text = tokio::task::spawn_blocking(move || {
                let state = source().map_err(|e| e.to_string())?;
                method(&LedgerMethods::from_shared(state), params)
            })
            .await
            .map_err(|e| RpcError::internal(format!("the ledger method panicked: {e}")))?
            .map_err(RpcError::internal)?;
            Ok(serde_json::Value::String(text))
        }
    })
}

#[derive(Clone)]
pub struct LedgerMethods {
    state: Arc<Mutex<LedgerState>>,
    write_observer: Option<WriteObserver>,
}

impl std::fmt::Debug for LedgerMethods {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LedgerMethods").finish_non_exhaustive()
    }
}

impl LedgerMethods {
    pub fn new(state: LedgerState) -> Self {
        Self::from_shared(Arc::new(Mutex::new(state)))
    }

    /// Build over an existing state handle: every call on the server shares
    /// one ledger.
    pub fn from_shared(state: Arc<Mutex<LedgerState>>) -> Self {
        Self {
            state,
            write_observer: None,
        }
    }

    pub fn shared_state(&self) -> Arc<Mutex<LedgerState>> {
        Arc::clone(&self.state)
    }

    /// Set where post-write notifications go.
    pub fn with_write_observer(mut self, observer: WriteObserver) -> Self {
        self.write_observer = Some(observer);
        self
    }
    fn notify_write(&self, paths: &[PathBuf]) {
        if let Some(observer) = &self.write_observer {
            observer(paths);
        }
    }

    /// Take the state lock, recovering it if an earlier panic poisoned it.
    ///
    /// `.lock().unwrap()` would turn one panic under this mutex into a
    /// permanently dead server. `LedgerState`'s invariants do not depend on
    /// the previous holder finishing: it holds a snapshot that `reload()`
    /// rebuilds from scratch. So warn once and carry on.
    fn lock_state(&self) -> std::sync::MutexGuard<'_, LedgerState> {
        if self.state.is_poisoned() {
            tracing::warn!(
                "ledger state mutex was poisoned by an earlier panic; recovering it rather \
                 than failing every tool call for the rest of the process"
            );
        }
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn parse_date(raw: &str) -> anyhow::Result<chrono::NaiveDate> {
    raw.parse::<chrono::NaiveDate>()
        .with_context(|| format!("not a YYYY-MM-DD date: {raw}"))
}

fn parse_decimal(raw: &str) -> anyhow::Result<rust_decimal::Decimal> {
    raw.parse::<rust_decimal::Decimal>()
        .with_context(|| format!("not a decimal: {raw}"))
}

fn parse_account_type(raw: &str) -> anyhow::Result<sapphire_ledger_core::AccountType> {
    use sapphire_ledger_core::AccountType::*;
    Ok(match raw {
        "Asset" => Asset,
        "Liability" => Liability,
        "Equity" => Equity,
        "Income" => Income,
        "Expense" => Expense,
        other => anyhow::bail!(
            "unknown account type {other:?}; expected one of \
             Asset, Liability, Equity, Income, Expense"
        ),
    })
}

fn parse_status(raw: &str) -> anyhow::Result<sapphire_ledger_core::TransactionStatus> {
    use sapphire_ledger_core::TransactionStatus::*;
    Ok(match raw {
        "cleared" => Cleared,
        "pending" => Pending,
        other => anyhow::bail!("unknown status {other:?}; expected `cleared` or `pending`"),
    })
}

/// Classify a caller-supplied account reference into exactly one slot of the
/// id/name pair `ops` expects: `account_id` if `needle` matches a known
/// account's id, otherwise `account_name`. `ops::resolve_account` then
/// resolves whichever slot was filled -- an id that does not match is a hard
/// error, it is never retried as a name.
fn split_account_ref(
    needle: &str,
    accounts: &[sapphire_ledger_core::Account],
) -> (Option<String>, Option<String>) {
    if accounts.iter().any(|a| a.id == needle) {
        (Some(needle.to_string()), None)
    } else {
        (None, Some(needle.to_string()))
    }
}

/// Render an error for the agent, chain and all.
///
/// A tool's error text is the only thing the operator -- an AI assistant --
/// can act on, and the context naming *which file* is broken lives in the
/// source chain rather than in the outermost message. `{:#}` on an
/// `anyhow::Error` walks that chain; plain `to_string()` prints the context
/// and drops the reason.
fn render_error(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// Build a write tool's success message, appending a warning if the
/// post-write `reload()` failed. The record is on disk and the observer has
/// already been told either way -- this only affects what the caller reads
/// back, never whether the write is reported as having happened.
fn write_result(
    verb: &str,
    kind: &str,
    id: &str,
    dest: &Path,
    reload_err: Option<sapphire_ledger_core::Error>,
) -> String {
    let mut msg = format!("{verb} {kind} {id}: {}", dest.display());
    if let Some(e) = reload_err {
        // Same reason as `render_error`: the path naming the broken file is
        // in the chain, not in the outermost message.
        let e = render_error(e.into());
        msg.push_str(&format!(
            "\nWARNING: the record was written, but the ledger snapshot could not be \
             reloaded afterwards: {e}"
        ));
    }
    msg
}

fn find_budget<'a>(
    budgets: &'a [sapphire_ledger_core::Budget],
    needle: &str,
) -> anyhow::Result<&'a sapphire_ledger_core::Budget> {
    budgets
        .iter()
        .find(|b| b.id == needle)
        .or_else(|| budgets.iter().find(|b| b.name == needle))
        .with_context(|| format!("no budget {needle}"))
}

fn account_refs(
    needles: Vec<String>,
    accounts: &[sapphire_ledger_core::Account],
) -> Vec<sapphire_ledger_core::AccountRef> {
    needles
        .iter()
        .map(|n| {
            let (account_id, account_name) = split_account_ref(n, accounts);
            sapphire_ledger_core::AccountRef {
                account_id,
                account_name,
            }
        })
        .collect()
}

fn build_posting(
    p: PostingParam,
    accounts: &[sapphire_ledger_core::Account],
) -> anyhow::Result<sapphire_ledger_core::Posting> {
    let price = match (p.price_value, p.price_currency) {
        (Some(value), Some(currency)) => Some(sapphire_ledger_core::Price {
            value: parse_decimal(&value)?,
            currency,
        }),
        (None, None) => None,
        _ => anyhow::bail!("price_value and price_currency must be given together"),
    };
    let (account_id, account_name) = split_account_ref(&p.account, accounts);
    Ok(sapphire_ledger_core::Posting {
        account_id,
        account_name,
        amount: parse_decimal(&p.amount)?,
        currency: p.currency,
        price,
        memo: p.memo,
    })
}

// ── methods ─────────────────────────────────────────────────────────────────────

impl LedgerMethods {
    fn list_accounts(&self, _: EmptyParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let guard = self.lock_state();
            let accounts: Vec<api::Account> = guard
                .workspace()
                .accounts
                .iter()
                .map(wire::account)
                .collect();
            Ok(serde_json::to_string_pretty(&accounts)?)
        })()
        .map_err(render_error)
    }

    fn get_transaction(&self, p: GetTransactionParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let guard = self.lock_state();
            let found = guard
                .workspace()
                .transactions
                .iter()
                .find(|t| t.id == p.id)
                .with_context(|| format!("no transaction with id {}", p.id))?;
            Ok(serde_json::to_string_pretty(&wire::transaction(found))?)
        })()
        .map_err(render_error)
    }

    fn query_postings(&self, p: QueryPostingsParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let from = p.date_from.as_deref().map(parse_date).transpose()?;
            let to = p.date_to.as_deref().map(parse_date).transpose()?;
            let guard = self.lock_state();
            let ws = guard.workspace();

            // Resolve the filter once, so that filtering by a renamed
            // account's *current* name still finds postings whose stored
            // name is stale, and so that a posting carrying only a name
            // still matches.
            let wanted: Option<(Option<String>, String)> = p.account.as_ref().map(|needle| {
                match ws
                    .accounts
                    .iter()
                    .find(|a| &a.id == needle || &a.name == needle)
                {
                    Some(a) => (Some(a.id.clone()), a.name.clone()),
                    // The needle names no known account. Keep it literally, so a
                    // hand-edited reference to an account that does not exist is
                    // still findable -- that is worth surfacing, not hiding.
                    None => (None, needle.clone()),
                }
            });

            let mut hits: Vec<PostingHit> = Vec::new();
            for tx in &ws.transactions {
                if from.is_some_and(|f| tx.date < f) || to.is_some_and(|t| tx.date > t) {
                    continue;
                }
                for posting in &tx.postings {
                    if let Some((wanted_id, wanted_needle)) = &wanted {
                        let matches = match (&posting.account_id, wanted_id) {
                            // Both sides resolved: the id is authoritative and decides alone.
                            (Some(pid), Some(wid)) => pid == wid,
                            // The posting carries no id, so its name is its only reference.
                            (None, _) => {
                                posting.account_name.as_deref() == Some(wanted_needle.as_str())
                            }
                            // The needle named no known account; compare it literally so a
                            // dangling id reference is still findable.
                            (Some(pid), None) => pid == wanted_needle,
                        };
                        if !matches {
                            continue;
                        }
                    }
                    if p.currency.as_ref().is_some_and(|c| c != &posting.currency) {
                        continue;
                    }
                    hits.push(PostingHit {
                        transaction_id: tx.id.clone(),
                        date: tx.date,
                        narration: tx.narration.clone(),
                        account_id: posting.account_id.clone(),
                        account_name: posting.account_name.clone(),
                        amount: posting.amount.to_string(),
                        currency: posting.currency.clone(),
                    });
                }
            }
            hits.sort_by(|a, b| {
                a.date
                    .cmp(&b.date)
                    .then_with(|| a.transaction_id.cmp(&b.transaction_id))
            });
            Ok(serde_json::to_string_pretty(&hits)?)
        })()
        .map_err(render_error)
    }

    fn query_prices(&self, p: QueryPricesParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let from = p.date_from.as_deref().map(parse_date).transpose()?;
            let to = p.date_to.as_deref().map(parse_date).transpose()?;
            let guard = self.lock_state();

            let mut hits: Vec<_> = guard
                .workspace()
                .prices
                .iter()
                .filter(|e| !from.is_some_and(|f| e.date < f))
                .filter(|e| !to.is_some_and(|t| e.date > t))
                .filter(|e| !p.base.as_ref().is_some_and(|b| b != &e.base))
                .filter(|e| !p.quote.as_ref().is_some_and(|q| q != &e.quote))
                .collect();
            hits.sort_by_key(|e| e.date);
            let hits: Vec<api::PriceEntry> = hits.into_iter().map(wire::price_entry).collect();
            Ok(serde_json::to_string_pretty(&hits)?)
        })()
        .map_err(render_error)
    }

    fn validate_workspace(&self, _: EmptyParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let mut guard = self.lock_state();
            guard.reload()?;
            let issues: Vec<api::ValidationIssue> = guard
                .workspace()
                .validate()
                .into_iter()
                .map(wire::validation_issue)
                .collect();
            Ok(serde_json::to_string_pretty(&issues)?)
        })()
        .map_err(render_error)
    }

    fn add_account(&self, p: AddAccountParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let account_type = parse_account_type(&p.account_type)?;
            let opened_at = parse_date(&p.opened_at)?;

            let mut guard = self.lock_state();
            let (id, dest) = ops::create_account(
                guard.root(),
                p.name,
                account_type,
                p.currencies,
                opened_at,
                p.description,
                p.default_tags,
            )?;
            let reload_err = guard.reload().err();
            drop(guard);
            // The file exists regardless of whether we could re-read the
            // workspace, so the observer must always hear about it.
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("created", "account", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn add_transaction(&self, p: AddTransactionParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let date = parse_date(&p.date)?;
            let status = p.status.as_deref().map(parse_status).transpose()?;

            let mut guard = self.lock_state();
            // Nothing has been written yet, so a failure here should refuse
            // the write rather than resolve accounts against a stale
            // snapshot -- `ops` reads the accounts directory fresh, and this
            // keeps the reference the MCP layer classifies in sync with it.
            guard.reload()?;
            let postings = {
                let accounts = &guard.workspace().accounts;
                p.postings
                    .into_iter()
                    .map(|param| build_posting(param, accounts))
                    .collect::<anyhow::Result<Vec<_>>>()?
            };
            let (id, dest) = ops::create_transaction(
                guard.root(),
                date,
                p.narration,
                p.payee,
                p.tags,
                status,
                postings,
                p.comment,
                p.apply_default_tags,
            )?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result(
                "created",
                "transaction",
                &id,
                &dest,
                reload_err,
            ))
        })()
        .map_err(render_error)
    }

    fn add_assertion(&self, p: AddAssertionParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let date = parse_date(&p.date)?;
            let balances = p
                .balances
                .into_iter()
                .map(|b| {
                    Ok(sapphire_ledger_core::Balance {
                        amount: parse_decimal(&b.amount)?,
                        currency: b.currency,
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            let mut guard = self.lock_state();
            // See add_transaction: refuse the write rather than classify the
            // account reference against a stale snapshot.
            guard.reload()?;
            let (account_id, account_name) =
                split_account_ref(&p.account, &guard.workspace().accounts);
            let (id, dest) =
                ops::create_assertion(guard.root(), account_id, account_name, date, balances)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("created", "assertion", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn add_price(&self, p: AddPriceParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let date = parse_date(&p.date)?;
            let rate = parse_decimal(&p.rate)?;

            let mut guard = self.lock_state();
            let (id, dest) =
                ops::create_price(guard.root(), date, p.base, p.quote, rate, p.source)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("created", "price", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn update_account(&self, p: UpdateAccountParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let tags = p
                .default_tags
                .context("nothing to change: give default_tags")?;
            let mut guard = self.lock_state();
            let (id, dest) = ops::update_account(guard.root(), &p.account, tags)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("updated", "account", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn list_budgets(&self, _: EmptyParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let guard = self.lock_state();
            let budgets: Vec<api::Budget> =
                guard.workspace().budgets.iter().map(wire::budget).collect();
            Ok(serde_json::to_string_pretty(&budgets)?)
        })()
        .map_err(render_error)
    }

    fn add_budget(&self, p: AddBudgetParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let amount = parse_decimal(&p.monthly_amount)?;
            let mut guard = self.lock_state();
            // See add_transaction: classify references against fresh accounts.
            guard.reload()?;
            let refs = account_refs(p.balance_accounts, &guard.workspace().accounts);
            let (id, dest) =
                ops::create_budget(guard.root(), p.name, p.tag, p.currency, amount, refs)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("created", "budget", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn update_budget(&self, p: UpdateBudgetParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let monthly_amount = p.monthly_amount.as_deref().map(parse_decimal).transpose()?;
            let mut guard = self.lock_state();
            guard.reload()?;
            let balance_accounts = p
                .balance_accounts
                .map(|n| account_refs(n, &guard.workspace().accounts));
            let changes = ops::BudgetChanges {
                name: p.name,
                tag: p.tag,
                currency: p.currency,
                monthly_amount,
                balance_accounts,
            };
            let (id, dest) = ops::update_budget(guard.root(), &p.budget, changes)?;
            let reload_err = guard.reload().err();
            drop(guard);
            self.notify_write(std::slice::from_ref(&dest));
            Ok(write_result("updated", "budget", &id, &dest, reload_err))
        })()
        .map_err(render_error)
    }

    fn budget_report(&self, p: BudgetReportParams) -> Result<String, String> {
        (|| -> anyhow::Result<String> {
            let (year, month) = sapphire_ledger_core::report::parse_month(&p.month)?;
            let guard = self.lock_state();
            let ws = guard.workspace();
            let budget = find_budget(&ws.budgets, &p.budget)?;
            let today = chrono::Local::now().date_naive();
            let report =
                sapphire_ledger_core::report::budget_report(ws, budget, year, month, today)?;
            Ok(serde_json::to_string_pretty(&wire::budget_report(
                budget, year, month, &report,
            ))?)
        })()
        .map_err(render_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sapphire_ledger_core::AccountType;

    /// A fresh ledger at `dir`, opened.
    fn prepare_state(dir: &Path) -> LedgerState {
        sapphire_ledger_core::init_workspace(dir, "JPY").expect("init test ledger");
        LedgerState::open(dir).expect("open test ledger")
    }

    /// Build a throwaway server over a freshly initialized ledger. Hold the
    /// `TempDir` for the test's duration — dropping it removes the directory
    /// the open state points at.
    fn test_server() -> (tempfile::TempDir, LedgerMethods) {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = prepare_state(dir.path());
        (dir, LedgerMethods::new(state))
    }

    #[test]
    fn list_accounts_is_empty_on_a_fresh_ledger() {
        let (_dir, server) = test_server();
        let json = server.list_accounts(EmptyParams {}).expect("ok");
        assert_eq!(json.trim(), "[]");
    }

    #[test]
    fn validate_workspace_reports_an_undefined_account() {
        let (dir, server) = test_server();
        // Written straight to disk: ops::create_transaction would refuse it,
        // which is the point -- this is the hand-edited-file case that
        // validate_workspace exists to catch.
        let path = dir.path().join("transactions/2026/05/tx00001.md");
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &path,
            r#"---
id: tx00001
date: 2026-05-21
narration: orphan
created_at: 2026-05-21T18:30:00+09:00
updated_at: 2026-05-21T18:30:00+09:00
postings:
- account_name: Expenses:Nowhere
  amount: 10
  currency: JPY
- account_name: Assets:Nowhere
  amount: -10
  currency: JPY
---
"#,
        )
        .expect("write");

        let json = server.validate_workspace(EmptyParams {}).expect("ok");
        assert!(json.contains("undefined account"), "got: {json}");
    }

    /// A posting can carry only `account_name` (no `account_id`) -- that is
    /// a valid posting, not an error. `query_postings` must still find it
    /// when filtering by that account's name; matching the id-only arm and
    /// dropping the name-only posting silently would make the tool look
    /// empty for exactly the postings it exists to surface.
    #[test]
    fn query_postings_matches_a_name_only_posting_by_account_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        prepare_state(dir.path());

        ops::create_account(
            dir.path(),
            "Expenses:Food".to_string(),
            AccountType::Expense,
            vec!["JPY".to_string()],
            "2026-01-01".parse().expect("date"),
            None,
            vec![],
        )
        .expect("create account");
        ops::create_account(
            dir.path(),
            "Assets:Cash".to_string(),
            AccountType::Asset,
            vec!["JPY".to_string()],
            "2026-01-01".parse().expect("date"),
            None,
            vec![],
        )
        .expect("create account");

        // Written straight to disk, like the validate_workspace fixture
        // above: this is the only way to produce a posting with a name but
        // no id, since ops::create_transaction always resolves and fills
        // account_id.
        let path = dir.path().join("transactions/2026/05/tx00002.md");
        std::fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &path,
            r#"---
id: tx00002
date: 2026-05-21
narration: groceries
created_at: 2026-05-21T18:30:00+09:00
updated_at: 2026-05-21T18:30:00+09:00
postings:
- account_name: Expenses:Food
  amount: 10
  currency: JPY
- account_name: Assets:Cash
  amount: -10
  currency: JPY
---
"#,
        )
        .expect("write");

        let state = LedgerState::open(dir.path()).expect("reopen ledger");
        let server = LedgerMethods::new(state);

        let json = server
            .query_postings(QueryPostingsParams {
                account: Some("Expenses:Food".to_string()),
                currency: None,
                date_from: None,
                date_to: None,
            })
            .expect("ok");
        assert!(json.contains("\"tx00002\""), "got: {json}");
    }

    fn posting_param(account: &str, amount: &str) -> PostingParam {
        PostingParam {
            account: account.to_string(),
            amount: amount.to_string(),
            currency: "JPY".to_string(),
            price_value: None,
            price_currency: None,
            memo: None,
        }
    }

    fn add_account(server: &LedgerMethods, name: &str, ty: &str) -> String {
        server
            .add_account(AddAccountParams {
                default_tags: vec![],
                name: name.into(),
                account_type: ty.into(),
                currencies: vec![],
                opened_at: "2026-01-01".into(),
                description: None,
            })
            .expect("add_account")
    }

    #[test]
    fn add_transaction_records_and_is_then_queryable() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");

        let created = server
            .add_transaction(AddTransactionParams {
                apply_default_tags: true,
                date: "2026-05-21".into(),
                narration: "イオン買い物".into(),
                payee: Some("イオン".into()),
                tags: vec!["grocery".into()],
                status: None,
                postings: vec![
                    posting_param("Expenses:Food", "1200"),
                    posting_param("Assets:Cash:JPY", "-1200"),
                ],
                comment: None,
            })
            .expect("add_transaction");
        assert!(created.contains("created"), "got: {created}");

        let hits = server
            .query_postings(QueryPostingsParams {
                account: Some("Expenses:Food".into()),
                currency: None,
                date_from: None,
                date_to: None,
            })
            .expect("query");
        assert!(hits.contains("イオン買い物"), "got: {hits}");

        let issues = server.validate_workspace(EmptyParams {}).expect("validate");
        assert_eq!(issues.trim(), "[]", "ledger should be clean, got: {issues}");
    }

    #[test]
    fn a_posting_can_name_an_account_by_its_id() {
        let (_dir, server) = test_server();
        let food_msg = add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");

        // add_account reports "created account <id>: <path>"; pull the id out.
        let food_id = food_msg
            .split_whitespace()
            .nth(2)
            .expect("id in message")
            .trim_end_matches(':')
            .to_string();

        server
            .add_transaction(AddTransactionParams {
                apply_default_tags: true,
                date: "2026-05-21".into(),
                narration: "by id".into(),
                payee: None,
                tags: vec![],
                status: None,
                postings: vec![
                    posting_param(&food_id, "500"),
                    posting_param("Assets:Cash:JPY", "-500"),
                ],
                comment: None,
            })
            .expect("add_transaction by id");

        let issues = server.validate_workspace(EmptyParams {}).expect("validate");
        assert_eq!(issues.trim(), "[]", "got: {issues}");
    }

    #[test]
    fn add_transaction_rejects_an_unbalanced_entry() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");

        let err = server
            .add_transaction(AddTransactionParams {
                apply_default_tags: true,
                date: "2026-05-21".into(),
                narration: "wrong".into(),
                payee: None,
                tags: vec![],
                status: None,
                postings: vec![
                    posting_param("Expenses:Food", "1200"),
                    posting_param("Assets:Cash:JPY", "-999"),
                ],
                comment: None,
            })
            .expect_err("must reject");
        assert!(err.contains("does not balance"), "got: {err}");
    }

    #[test]
    fn add_account_rejects_an_unknown_type() {
        let (_dir, server) = test_server();
        let err = server
            .add_account(AddAccountParams {
                default_tags: vec![],
                name: "Assets:Cash:JPY".into(),
                account_type: "Bogus".into(),
                currencies: vec![],
                opened_at: "2026-01-01".into(),
                description: None,
            })
            .expect_err("must reject");
        assert!(
            err.contains("Bogus"),
            "the error should name the bad input, got: {err}"
        );
    }

    /// Build a throwaway server that records every path it is asked to
    /// notify about, so a test can assert the observer actually fired.
    fn test_server_with_observer() -> (tempfile::TempDir, LedgerMethods, Arc<Mutex<Vec<PathBuf>>>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = prepare_state(dir.path());
        let seen: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_for_observer = Arc::clone(&seen);
        let server =
            LedgerMethods::new(state).with_write_observer(Arc::new(move |paths: &[PathBuf]| {
                seen_for_observer.lock().unwrap().extend_from_slice(paths);
            }));
        (dir, server, seen)
    }

    #[test]
    fn a_successful_write_notifies_the_observer() {
        let (_dir, server, seen) = test_server_with_observer();

        server
            .add_price(AddPriceParams {
                date: "2026-05-21".into(),
                base: "USD".into(),
                quote: "JPY".into(),
                rate: "150".into(),
                source: None,
            })
            .expect("add_price");

        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "observer should fire exactly once for one write"
        );
    }

    /// `load_workspace` is fail-fast over every file in the workspace, so a
    /// single malformed record anywhere -- a human edit, a half-finished
    /// sync, a git conflict marker -- makes the post-write `reload()` fail.
    /// That must not be reported as a failed write: the file is genuinely on
    /// disk, the observer must still be told, and an AI caller must not be
    /// tempted to retry (which would mint a second, duplicate record).
    #[test]
    fn a_broken_record_does_not_turn_a_successful_write_into_a_reported_failure() {
        let (dir, server, seen) = test_server_with_observer();

        let broken = dir.path().join("assertions/2026/05/as-broken.toml");
        std::fs::create_dir_all(broken.parent().unwrap()).expect("mkdir");
        std::fs::write(&broken, "this is not valid toml >>>>>>> HEAD\n")
            .expect("write broken toml");

        let result = server
            .add_price(AddPriceParams {
                date: "2026-05-21".into(),
                base: "USD".into(),
                quote: "JPY".into(),
                rate: "150".into(),
                source: None,
            })
            .expect("the write itself succeeded; only the post-write reload should fail");

        assert!(result.contains("created price"), "got: {result}");
        assert!(
            result.contains("WARNING"),
            "the caller should be told the snapshot reload failed, got: {result}"
        );
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "the observer must still fire even though reload() failed"
        );
    }

    /// The operator is an AI that can only act on what the error text says.
    /// `validate_workspace` is the tool it reaches for to diagnose a broken
    /// ledger, and a parse error naming a line number in an unnamed file is
    /// not actionable.
    #[test]
    fn validate_workspace_names_the_file_it_could_not_parse() {
        let (dir, server) = test_server();

        let broken = dir.path().join("assertions/2026/05/as-broken.toml");
        std::fs::create_dir_all(broken.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &broken,
            "this is not valid toml >>>>>>> HEAD
",
        )
        .expect("write");

        let err = server
            .validate_workspace(EmptyParams {})
            .expect_err("a malformed record must fail the reload");
        assert!(
            err.contains("as-broken.toml"),
            "the agent cannot fix a file it is not told about, got: {err}"
        );
        assert!(
            err.contains("TOML parse error"),
            "the reason must survive alongside the path, got: {err}"
        );
    }

    /// A write refused because some *other* record is malformed has to name
    /// that record too -- otherwise the agent is told its write was blocked
    /// and has nothing to go on.
    #[test]
    fn a_blocked_write_names_the_file_that_blocked_it() {
        let (dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash", "Asset");

        let broken = dir.path().join("assertions/2026/05/as-broken.toml");
        std::fs::create_dir_all(broken.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &broken,
            "this is not valid toml >>>>>>> HEAD
",
        )
        .expect("write");

        let err = server
            .add_transaction(AddTransactionParams {
                apply_default_tags: true,
                date: "2026-05-21".into(),
                narration: "blocked".into(),
                payee: None,
                tags: vec![],
                status: None,
                postings: vec![
                    posting_param("Expenses:Food", "1200"),
                    posting_param("Assets:Cash", "-1200"),
                ],
                comment: None,
            })
            .expect_err("the pre-write reload must refuse the write");
        assert!(
            err.contains("as-broken.toml"),
            "the agent must be told which file blocked the write, got: {err}"
        );
    }

    /// The post-write WARNING embeds the same error, and is subject to the
    /// same requirement.
    #[test]
    fn the_post_write_warning_names_the_broken_file() {
        let (dir, server, _seen) = test_server_with_observer();

        let broken = dir.path().join("assertions/2026/05/as-broken.toml");
        std::fs::create_dir_all(broken.parent().unwrap()).expect("mkdir");
        std::fs::write(
            &broken,
            "this is not valid toml >>>>>>> HEAD
",
        )
        .expect("write");

        let result = server
            .add_price(AddPriceParams {
                date: "2026-05-21".into(),
                base: "USD".into(),
                quote: "JPY".into(),
                rate: "150".into(),
                source: None,
            })
            .expect("the write itself succeeded");
        assert!(
            result.contains("as-broken.toml"),
            "the WARNING must name the file that broke the reload, got: {result}"
        );
    }

    fn add_budget(server: &LedgerMethods) -> String {
        server
            .add_budget(api::AddBudgetParams {
                name: "生活費".into(),
                tag: "living".into(),
                currency: "JPY".into(),
                monthly_amount: "100000".into(),
                balance_accounts: vec!["Assets:Cash:JPY".into()],
                create_panels: false,
            })
            .expect("add_budget")
    }

    fn spend(server: &LedgerMethods, date: &str, tags: Vec<String>, amount: &str, apply: bool) {
        server
            .add_transaction(AddTransactionParams {
                date: date.into(),
                narration: "spend".into(),
                payee: None,
                tags,
                status: None,
                postings: vec![
                    posting_param("Expenses:Food", amount),
                    posting_param("Assets:Cash:JPY", &format!("-{amount}")),
                ],
                comment: None,
                apply_default_tags: apply,
            })
            .expect("add_transaction");
    }

    #[test]
    fn a_budget_is_listed_and_reported_by_day() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");
        assert!(add_budget(&server).contains("created budget"));
        spend(&server, "2026-05-21", vec!["living".into()], "1200", true);

        let budgets: Vec<api::Budget> =
            serde_json::from_str(&server.list_budgets(EmptyParams {}).unwrap()).unwrap();
        assert_eq!(budgets[0].name, "生活費");
        assert_eq!(
            budgets[0].balance_accounts[0].account_name.as_deref(),
            Some("Assets:Cash:JPY")
        );

        let report: api::BudgetReport = serde_json::from_str(
            &server
                .budget_report(api::BudgetReportParams {
                    budget: "生活費".into(),
                    month: "2026-05".into(),
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(report.month, "2026-05");
        assert_eq!(report.daily_budget, "3225.81");
        assert_eq!(report.days.len(), 31);
        assert_eq!(report.days[20].spent_cumulative, "1200");
        assert_eq!(report.days[30].budget_cumulative, "100000");
        assert_eq!(report.days[20].actual_balance.as_deref(), Some("-1200"));
    }

    #[test]
    fn update_budget_changes_the_amount_and_a_bad_month_is_refused() {
        let (_dir, server) = test_server();
        add_account(&server, "Assets:Cash:JPY", "Asset");
        add_budget(&server);
        let msg = server
            .update_budget(api::UpdateBudgetParams {
                budget: "生活費".into(),
                name: None,
                tag: None,
                currency: None,
                monthly_amount: Some("120000".into()),
                balance_accounts: None,
            })
            .unwrap();
        assert!(msg.contains("updated budget"), "got: {msg}");
        let budgets: Vec<api::Budget> =
            serde_json::from_str(&server.list_budgets(EmptyParams {}).unwrap()).unwrap();
        assert_eq!(budgets[0].monthly_amount, "120000");

        let err = server
            .budget_report(api::BudgetReportParams {
                budget: "生活費".into(),
                month: "May".into(),
            })
            .unwrap_err();
        assert!(err.contains("YYYY-MM"), "got: {err}");
        let err = server
            .budget_report(api::BudgetReportParams {
                budget: "nope".into(),
                month: "2026-05".into(),
            })
            .unwrap_err();
        assert!(err.contains("no budget nope"), "got: {err}");
    }

    #[test]
    fn default_tags_set_by_update_account_reach_new_transactions() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        add_account(&server, "Assets:Cash:JPY", "Asset");
        let msg = server
            .update_account(api::UpdateAccountParams {
                account: "Expenses:Food".into(),
                default_tags: Some(vec!["living".into()]),
            })
            .unwrap();
        assert!(msg.contains("updated account"), "got: {msg}");
        let accounts: Vec<api::Account> =
            serde_json::from_str(&server.list_accounts(EmptyParams {}).unwrap()).unwrap();
        let food = accounts.iter().find(|a| a.name == "Expenses:Food").unwrap();
        assert_eq!(food.default_tags, ["living"]);

        spend(&server, "2026-05-21", vec![], "100", true);
        spend(&server, "2026-05-22", vec![], "100", false);
        let state = server.shared_state();
        let guard = state.lock().unwrap();
        let mut tags: Vec<Vec<String>> = guard
            .workspace()
            .transactions
            .iter()
            .map(|t| t.tags.clone())
            .collect();
        tags.sort();
        assert_eq!(tags, vec![vec![], vec!["living".to_string()]]);
    }

    #[test]
    fn update_account_with_nothing_to_change_is_refused() {
        let (_dir, server) = test_server();
        add_account(&server, "Expenses:Food", "Expense");
        let err = server
            .update_account(api::UpdateAccountParams {
                account: "Expenses:Food".into(),
                default_tags: None,
            })
            .unwrap_err();
        assert!(err.contains("nothing to change"), "got: {err}");
    }
}
