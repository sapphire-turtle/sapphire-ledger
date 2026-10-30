use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

pub use sapphire_framework::server::FrameworkCommand;

/// The command line of `sapphire-ledger`, the one command both editions put
/// on PATH.
///
/// The ledger's own verbs and the framework's sit side by side at the top
/// level. A bare invocation prints the help; the service manager starts the
/// binary with `serve`. The desktop app parses the same [`GlobalArgs`] and
/// [`Command`], with the command optional: none opens its window.
#[derive(Parser)]
#[command(
    name = "sapphire-ledger",
    arg_required_else_help = true,
    about = "Local-first double-entry household ledger",
    version
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

/// The options every command takes.
#[derive(Args)]
pub struct GlobalArgs {
    /// Path to the ledger root (the directory containing `.sapphire-ledger/`).
    /// Overrides the automatic upward search from the current directory.
    /// Can also be set via the SAPPHIRE_LEDGER_DIR environment variable.
    ///
    /// Read by the ledger's own verbs only: `serve` serves the workspace the
    /// server has selected (`workspace select`), and `mcp` works on that one.
    #[arg(long, env = "SAPPHIRE_LEDGER_DIR", global = true, value_name = "DIR")]
    pub ledger_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Initialize a new ledger in the given directory (defaults to current directory)
    Init {
        /// Directory to initialize (created if it does not exist)
        path: Option<PathBuf>,
        /// Base currency used for reporting (default: JPY)
        #[arg(long, default_value = "JPY")]
        base_currency: String,
    },
    /// Load every record and report any validation issues
    Check,
    /// Upgrade the ledger's files to the layout this build reads.
    ///
    /// Moves accounts and transactions from TOML to Markdown with YAML front
    /// matter, their description or comment becoming the body. Safe to re-run;
    /// commit (or back up) the ledger first, since TOML `#` comments are not
    /// carried over.
    Migrate {
        /// Check every record and list what would change, writing nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Manage the chart of accounts.
    #[command(subcommand)]
    Account(AccountCommand),
    /// Record and read transactions.
    #[command(subcommand)]
    Tx(TxCommand),
    /// Record balance assertions.
    #[command(subcommand)]
    Assertion(AssertionCommand),
    /// Record observed exchange rates.
    #[command(subcommand)]
    Price(PriceCommand),
    /// Run saved panels: a chart's queries, as a table.
    #[command(subcommand)]
    Panel(PanelCommand),
    /// The framework's verbs: serve, status, service, update, workspace,
    /// workgroup, device, embedding, external-device, config, mcp.
    #[command(flatten)]
    Framework(FrameworkCommand),
}

#[derive(Subcommand)]
pub enum AccountCommand {
    /// Create an account. Every account a posting names must exist first.
    Add {
        /// Colon-separated name, e.g. `Assets:Cash:JPY`.
        name: String,
        /// One of: Asset, Liability, Equity, Income, Expense.
        #[arg(long = "type", value_name = "TYPE")]
        account_type: String,
        /// Currencies this account may hold. Repeatable. Omit to allow any.
        #[arg(long = "currency", value_name = "CCY")]
        currencies: Vec<String>,
        /// `YYYY-MM-DD` the account opened. Defaults to today.
        #[arg(long, value_name = "DATE")]
        opened_at: Option<String>,
        /// Free-form notes, kept as the Markdown body of the account's file.
        #[arg(long)]
        description: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum TxCommand {
    /// Record a transaction.
    ///
    /// Postings must sum to zero per currency, and every account named must
    /// already exist. Nothing is written if validation fails.
    Add {
        /// `YYYY-MM-DD`. Defaults to today.
        #[arg(long, value_name = "DATE")]
        date: Option<String>,
        /// What the transaction was.
        #[arg(long)]
        narration: String,
        #[arg(long)]
        payee: Option<String>,
        /// Repeatable.
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
        /// `cleared` or `pending`.
        #[arg(long)]
        status: Option<String>,
        /// `<account> <amount> <currency>`, with an optional
        /// `@ <value> <currency>` for a cross-currency inline price.
        /// Repeatable; at least two are required.
        ///
        /// The account may be given by name or by id.
        #[arg(
            long = "posting",
            value_name = "SPEC",
            required = true,
            allow_hyphen_values = true
        )]
        postings: Vec<String>,
        /// Free-form notes, kept as the Markdown body of the transaction's file.
        #[arg(long)]
        comment: Option<String>,
    },
    /// List postings, newest last.
    List {
        /// Account name or id.
        #[arg(long)]
        account: Option<String>,
        #[arg(long, value_name = "CCY")]
        currency: Option<String>,
        /// Inclusive lower bound, `YYYY-MM-DD`.
        #[arg(long, value_name = "DATE")]
        date_from: Option<String>,
        /// Inclusive upper bound, `YYYY-MM-DD`.
        #[arg(long, value_name = "DATE")]
        date_to: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum AssertionCommand {
    /// Record what an account should hold at the END of a date.
    Add {
        /// Account name or id.
        account: String,
        /// `YYYY-MM-DD`.
        #[arg(long, value_name = "DATE")]
        date: String,
        /// `<amount> <currency>`. Repeatable; at least one is required.
        #[arg(
            long = "balance",
            value_name = "SPEC",
            required = true,
            allow_hyphen_values = true
        )]
        balances: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum PriceCommand {
    /// Record that one unit of `base` cost `rate` units of `quote` on `date`.
    Add {
        /// `YYYY-MM-DD`.
        #[arg(long, value_name = "DATE")]
        date: String,
        #[arg(long, value_name = "CCY")]
        base: String,
        #[arg(long, value_name = "CCY")]
        quote: String,
        #[arg(long)]
        rate: String,
        /// Where the rate came from, e.g. `manual`.
        #[arg(long)]
        source: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PanelCommand {
    /// Every panel: id, name and period.
    List,
    /// One panel's queries, a table per plot: a row per bucket, a column per series.
    Run {
        /// Panel name or id.
        panel: String,
        /// `YYYY-MM-DD` the period is around. Defaults to today.
        #[arg(long, value_name = "DATE")]
        anchor: Option<String>,
        /// Periods to move by: -1 is the previous month (or year).
        #[arg(long, allow_hyphen_values = true, value_name = "N")]
        offset: Option<i32>,
    },
}
