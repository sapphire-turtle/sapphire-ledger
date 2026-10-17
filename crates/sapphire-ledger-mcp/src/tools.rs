//! The ledger's MCP tools: an `rmcp` handler whose tools call the app
//! server's `sapphire-ledger.*` methods ([`crate::methods`]), beside the
//! framework's own tools.
//!
//! The handler holds the framework's [`ToolContext`], so the same tools serve
//! the `mcp` subcommand (which dials the running server) and `/mcp` on the
//! server's HTTP listener (an in-process connection). Hand it to the server
//! with [`service`].

use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::ServerConfig,
    tool, tool_handler, tool_router,
};
use sapphire_framework::ipc;
use sapphire_framework::mcp::{self, McpService, ToolContext};
use serde::Serialize;

use sapphire_ledger_api::{
    self as api, AddAccountParams, AddAssertionParams, AddBudgetParams, AddPriceParams,
    AddTransactionParams, BudgetReportParams, EmptyParams, GetTransactionParams,
    QueryPostingsParams, QueryPricesParams, UpdateAccountParams, UpdateBudgetParams,
};

/// How the ledger answers MCP: [`LedgerTools`], one per session.
pub fn service() -> McpService {
    McpService::new(LedgerTools::new)
}

#[derive(Clone, Debug)]
pub struct LedgerTools {
    ctx: ToolContext,
    tool_router: ToolRouter<Self>,
}

impl AsRef<ToolContext> for LedgerTools {
    fn as_ref(&self) -> &ToolContext {
        &self.ctx
    }
}

impl LedgerTools {
    /// The ledger's tools and the framework's read-only ones. The
    /// framework's raw file writes are left out: a ledger record written
    /// without the ledger's checks can leave the books unbalanced.
    pub fn new(ctx: ToolContext) -> Self {
        Self {
            ctx,
            tool_router: Self::tool_router() + mcp::framework_tools(),
        }
    }

    /// Call one of the ledger's methods and hand its text back as the tool's.
    ///
    /// An error the method answered with is the ledger's message to the
    /// agent, so it goes through without the IPC layer's framing.
    async fn call(&self, method: &str, params: impl Serialize) -> Result<String, String> {
        self.ctx.call(method, params).await.map_err(|e| match e {
            mcp::Error::Ipc(ipc::Error::Rpc(rpc)) => rpc.message,
            other => other.to_string(),
        })
    }
}

#[tool_router]
impl LedgerTools {
    #[tool(
        description = "List every account with its id, name, type, allowed currencies \
        and open date. The id is the stable handle: it survives a rename, and other \
        tools accept it wherever they accept a name.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_accounts(
        &self,
        Parameters(p): Parameters<EmptyParams>,
    ) -> Result<String, String> {
        self.call(api::LIST_ACCOUNTS, p).await
    }

    #[tool(
        description = "Show one transaction, with all of its postings, by id.",
        annotations(read_only_hint = true)
    )]
    pub async fn get_transaction(
        &self,
        Parameters(p): Parameters<GetTransactionParams>,
    ) -> Result<String, String> {
        self.call(api::GET_TRANSACTION, p).await
    }

    #[tool(
        description = "Find postings, optionally filtered by account, currency and date \
        range. `account` matches either an account id or an account name. Each result \
        carries its transaction's id, date and narration.",
        annotations(read_only_hint = true)
    )]
    pub async fn query_postings(
        &self,
        Parameters(p): Parameters<QueryPostingsParams>,
    ) -> Result<String, String> {
        self.call(api::QUERY_POSTINGS, p).await
    }

    #[tool(
        description = "Find price-log entries, optionally filtered by base, quote and \
        date range.",
        annotations(read_only_hint = true)
    )]
    pub async fn query_prices(
        &self,
        Parameters(p): Parameters<QueryPricesParams>,
    ) -> Result<String, String> {
        self.call(api::QUERY_PRICES, p).await
    }

    #[tool(
        description = "Re-read the ledger from disk and report every validation issue. \
        Returns an empty array when the ledger is consistent. A posting whose stored \
        account_name is out of date is NOT an issue -- the account_id is what counts.",
        annotations(read_only_hint = true)
    )]
    pub async fn validate_workspace(
        &self,
        Parameters(p): Parameters<EmptyParams>,
    ) -> Result<String, String> {
        self.call(api::VALIDATE_WORKSPACE, p).await
    }

    #[tool(
        description = "Create an account. Fails if one with that name already exists. \
        Every account a posting names must be created first. Returns the new account's id. \
        `default_tags` are copied into the transactions later recorded against it."
    )]
    pub async fn add_account(
        &self,
        Parameters(p): Parameters<AddAccountParams>,
    ) -> Result<String, String> {
        self.call(api::ADD_ACCOUNT, p).await
    }

    #[tool(
        description = "Record a transaction. Postings must sum to zero per currency, and \
        every account named must already exist. The posting accounts' default tags are \
        added to `tags` unless `apply_default_tags` is false. Nothing is written if \
        validation fails."
    )]
    pub async fn add_transaction(
        &self,
        Parameters(p): Parameters<AddTransactionParams>,
    ) -> Result<String, String> {
        self.call(api::ADD_TRANSACTION, p).await
    }

    #[tool(
        description = "Record a balance assertion: what an account should hold at the END \
        of a date. A mismatch is a hard error when the books are checked."
    )]
    pub async fn add_assertion(
        &self,
        Parameters(p): Parameters<AddAssertionParams>,
    ) -> Result<String, String> {
        self.call(api::ADD_ASSERTION, p).await
    }

    #[tool(
        description = "Record an observed exchange rate in the price log: one unit of \
        `base` costs `rate` units of `quote` on `date`."
    )]
    pub async fn add_price(
        &self,
        Parameters(p): Parameters<AddPriceParams>,
    ) -> Result<String, String> {
        self.call(api::ADD_PRICE, p).await
    }

    #[tool(
        description = "List every budget: its id, name, the tag that selects its \
        transactions, currency, monthly amount and the accounts it is paid from.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_budgets(
        &self,
        Parameters(p): Parameters<EmptyParams>,
    ) -> Result<String, String> {
        self.call(api::LIST_BUDGETS, p).await
    }

    #[tool(
        description = "One budget's month, day by day: spending of the transactions \
        carrying the budget's tag (to Expense accounts, in its currency), cumulated, \
        against the monthly amount spread evenly over the month; and the paid-from \
        accounts' balance against the month-start balance minus that daily budget. \
        `budget` is its id or name, `month` is YYYY-MM. Days after today are marked \
        `elapsed: false`.",
        annotations(read_only_hint = true)
    )]
    pub async fn budget_report(
        &self,
        Parameters(p): Parameters<BudgetReportParams>,
    ) -> Result<String, String> {
        self.call(api::BUDGET_REPORT, p).await
    }

    #[tool(
        description = "Create a monthly budget. Transactions carrying `tag` count \
        against it; `balance_accounts` (ids or names) are what it is paid from. \
        Fails if a budget with that name exists."
    )]
    pub async fn add_budget(
        &self,
        Parameters(p): Parameters<AddBudgetParams>,
    ) -> Result<String, String> {
        self.call(api::ADD_BUDGET, p).await
    }

    #[tool(
        description = "Change a budget, by id or name. Only the fields given change; \
        a new amount also redraws past months. The file is rewritten from its known \
        fields: unknown keys and comments in it are not kept."
    )]
    pub async fn update_budget(
        &self,
        Parameters(p): Parameters<UpdateBudgetParams>,
    ) -> Result<String, String> {
        self.call(api::UPDATE_BUDGET, p).await
    }

    #[tool(
        description = "Change an account's default tags, by id or name. Default tags \
        are copied into each transaction recorded afterwards that posts to the \
        account; past transactions keep the tags they have. The file is rewritten from its \
        known fields: unknown keys and comments in it are not kept."
    )]
    pub async fn update_account(
        &self,
        Parameters(p): Parameters<UpdateAccountParams>,
    ) -> Result<String, String> {
        self.call(api::UPDATE_ACCOUNT, p).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LedgerTools {
    fn get_info(&self) -> ServerConfig {
        let info = mcp::server_info(&self.ctx);
        let framework = info.instructions.clone().unwrap_or_default();
        info.with_instructions(format!(
            "Sapphire Ledger is a plain-text double-entry household ledger. Use \
             list_accounts to see the chart of accounts, query_postings to find what \
             happened to an account, add_transaction to record spending, and \
             validate_workspace to check the books. Every transaction must balance: its \
             postings sum to zero per currency. An account must exist before anything \
             can be posted to it. list_budgets and budget_report show how a month is going \
             against a budget.\n\n{framework}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<rmcp::model::Tool> {
        let endpoint = ipc::Endpoint::in_dir("sapphire-ledger", std::env::temp_dir());
        let ctx = ToolContext::dial("sapphire-ledger", "0.0.0", None, endpoint);
        LedgerTools::new(ctx).tool_router.list_all()
    }

    /// Zero-argument tools must advertise a top-level `type: object`, or
    /// Anthropic rejects the tool with
    /// `tools.<N>.custom.input_schema.type: Field required`.
    #[test]
    fn every_tool_input_schema_declares_object_type() {
        for tool in tools() {
            let schema = serde_json::to_value(&tool.input_schema).expect("schema to json");
            assert_eq!(
                schema.get("type").and_then(|t| t.as_str()),
                Some("object"),
                "tool {} has a non-object input schema: {schema}",
                tool.name
            );
        }
    }

    /// The ledger's fourteen, the framework's read-only seven, and none of the
    /// framework's raw file writes.
    #[test]
    fn the_ledger_serves_its_tools_and_the_frameworks_read_only_ones() {
        let names: Vec<String> = tools().iter().map(|t| t.name.to_string()).collect();
        for expected in [
            "list_budgets",
            "budget_report",
            "add_budget",
            "update_budget",
            "update_account",
            "list_accounts",
            "add_transaction",
            "validate_workspace",
            "status",
            "workspace_show",
            "workspace_read_file",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "missing {expected}: {names:?}"
            );
        }
        assert!(
            !names.iter().any(|n| n.starts_with("workspace_write")
                || n == "workspace_append_file"
                || n == "workspace_delete_file"),
            "raw writes would bypass the ledger's checks: {names:?}"
        );
        assert_eq!(names.len(), 14 + 7, "{names:?}");
    }
}
