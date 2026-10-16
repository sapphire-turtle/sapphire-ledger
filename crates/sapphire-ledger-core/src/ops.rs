//! The single write path for every record kind.
//!
//! CLI, MCP, GUI and importers all funnel through here so that id
//! generation, path resolution, validation and the refuse-to-overwrite rule
//! exist in exactly one place.

use grain_id::GrainId;

/// Mint a time-ordered record id, for records whose id is their filename.
///
/// `GrainId::now_unix()` has decisecond resolution, so records made in the
/// same tenth of a second collide; the create functions re-mint rather than
/// overwrite. Time ordering is what makes a `{year}/{MM}/` listing readable.
pub fn new_id() -> String {
    GrainId::now_unix().to_string()
}

/// Mint a random record id, for records whose id is *not* their filename.
///
/// Accounts use this. Their id does no ordering work — `opened_at` carries
/// the meaningful date — and a chart of accounts is typically created in one
/// sitting, where time-ordered ids would all share a long leading prefix
/// exactly when there are the most to tell apart at a CLI prompt.
pub fn new_random_id() -> String {
    GrainId::random().to_string()
}

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset, Local, NaiveDate};
use rust_decimal::Decimal;

use crate::account::{Account, AccountType, resolve_account};
use crate::assertion::{Assertion, Balance};
use crate::budget::{AccountRef, Budget};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::prices::PriceEntry;
use crate::repository::{
    check_schema_version, load_markdown, load_toml, save_markdown, save_toml, walk_files,
    walk_markdown_files,
};
use crate::transaction::{Posting, Transaction, TransactionStatus};
use crate::workspace::{
    ACCOUNTS_DIR, ASSERTIONS_DIR, BUDGETS_DIR, CONFIG_FILE, MARKDOWN_EXTENSION, PRICES_DIR,
    TOML_EXTENSION, TRANSACTIONS_DIR, WORKSPACE_DIR, account_relative_path,
    assertion_relative_path, budget_relative_path, price_relative_path, transaction_relative_path,
};

/// How many times to re-mint an id when the destination is already taken.
///
/// `new_id()` is deterministic within a decisecond, so re-calling it on a
/// collision would mint the same id forever; [`mint_free_id`] falls back to
/// `new_random_id()` after the first attempt to actually break the tie. An
/// account's random id colliding with an existing one is astronomically
/// unlikely, but the retry must still terminate.
const ID_ATTEMPTS: usize = 32;

fn now() -> DateTime<FixedOffset> {
    Local::now().into()
}

/// Refuse to write into a ledger this build does not read: a record in the
/// current layout would land in a ledger still in the old one.
fn refuse_other_schema(root: &Path) -> Result<()> {
    let config_path = root.join(WORKSPACE_DIR).join(CONFIG_FILE);
    let config: Config = load_toml(&config_path)?;
    check_schema_version(&config_path, config.schema_version)
}

fn refuse_existing(path: &Path) -> Result<()> {
    if path.exists() {
        return Err(Error::Validation(format!(
            "{} already exists; refusing to overwrite",
            path.display()
        )));
    }
    Ok(())
}

/// Load just the accounts. Creates need them to resolve references and to
/// check id uniqueness, and at household scale re-walking is free.
fn load_accounts(root: &Path) -> Result<Vec<Account>> {
    walk_markdown_files(&root.join(ACCOUNTS_DIR))?
        .iter()
        .map(|p| load_markdown::<Account>(p))
        .collect()
}

/// Mint an id that no record of this kind is already using.
///
/// Uniqueness is scoped to the **record kind**, not to one `{year}/{MM}/`
/// directory. The destination path embeds the record's own date, so checking
/// only whether `dest` exists would let two records minted inside the same
/// decisecond with dates in different months take the same id — their paths
/// differ, so neither collision check would fire. `kind_dir` is walked to
/// collect every id already taken across the kind; `extension` is the kind's
/// file format.
///
/// Time-ordering is an ergonomic nicety on top of uniqueness, so the first
/// attempt uses the time-ordered generator, which is what a record written on
/// its own gets. `new_id()` has decisecond resolution and no random component,
/// so it is deterministic within that window: a collision means another record
/// of this kind was minted in this same decisecond, and re-calling it would
/// mint the identical id forever. Only a different generator can break that
/// tie, so every attempt after the first is random.
fn mint_free_id<F>(
    root: &Path,
    kind_dir: &str,
    extension: &str,
    relative: F,
) -> Result<(String, PathBuf)>
where
    F: Fn(&str) -> PathBuf,
{
    let taken: HashSet<String> = walk_files(&root.join(kind_dir), extension)?
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();

    for attempt in 0..ID_ATTEMPTS {
        let id = if attempt == 0 {
            new_id()
        } else {
            new_random_id()
        };
        if taken.contains(&id) {
            continue;
        }
        let dest = root.join(relative(&id));
        // The walk above is a snapshot; still refuse an existing destination.
        if !dest.exists() {
            return Ok((id, dest));
        }
    }
    Err(Error::Validation(format!(
        "could not mint a free record id after {ID_ATTEMPTS} attempts"
    )))
}

/// Rewrite a set of postings so each one carries the resolved `account_id`
/// and the account's current `account_name`.
///
/// A record must never reach disk carrying only a name: the name is the part
/// allowed to go stale.
fn resolve_postings(postings: Vec<Posting>, accounts: &[Account]) -> Result<Vec<Posting>> {
    let by_id: HashMap<&str, &Account> = accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> = accounts.iter().map(|a| (a.name.as_str(), a)).collect();

    postings
        .into_iter()
        .map(|p| {
            let account = resolve_account(
                p.account_id.as_deref(),
                p.account_name.as_deref(),
                &by_id,
                &by_name,
            )?;
            if !account.allows_currency(&p.currency) {
                return Err(Error::Validation(format!(
                    "posts {} to {}, but that account only allows {}",
                    p.currency,
                    account.name,
                    account.currencies.join(", "),
                )));
            }
            Ok(Posting {
                account_id: Some(account.id.clone()),
                account_name: Some(account.name.clone()),
                ..p
            })
        })
        .collect()
}

/// Trim each tag, drop empty ones and repeats, keeping first-seen order.
pub fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = tag.trim();
        if !tag.is_empty() && !out.iter().any(|t| t == tag) {
            out.push(tag.to_owned());
        }
    }
    out
}

/// `tags`, then every default tag of every posting's account not already
/// there, in posting order. The postings must already be resolved.
fn with_default_tags(
    mut tags: Vec<String>,
    postings: &[Posting],
    accounts: &[Account],
) -> Vec<String> {
    for posting in postings {
        let Some(account) = accounts
            .iter()
            .find(|a| posting.account_id.as_deref() == Some(a.id.as_str()))
        else {
            continue;
        };
        for tag in &account.default_tags {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

/// Write a new account.
///
/// A duplicate name is a hard error. The path comes from the name, so the
/// refuse-to-overwrite check catches that on its own — but only while every
/// account file sits at the path its name implies, and nothing enforces that:
/// a hand edit, a half-applied rename or a sync artifact can leave an account
/// named `Assets:Foo` in `accounts/Assets/Bar.md`. So the name is also
/// checked against the accounts already on disk.
///
/// The id is random and is *not* in the path, so the refuse-to-overwrite
/// check cannot see an id collision either — that is checked here too.
pub fn create_account(
    root: &Path,
    name: String,
    account_type: AccountType,
    currencies: Vec<String>,
    opened_at: NaiveDate,
    description: Option<String>,
    default_tags: Vec<String>,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let dest = root.join(account_relative_path(&name)?);
    refuse_existing(&dest)?;

    let existing = load_accounts(root)?;
    if existing.iter().any(|a| a.name == name) {
        return Err(Error::Validation(format!(
            "an account named {name} already exists"
        )));
    }
    let taken: std::collections::HashSet<&str> = existing.iter().map(|a| a.id.as_str()).collect();

    let id = (0..ID_ATTEMPTS)
        .map(|_| new_random_id())
        .find(|candidate| !taken.contains(candidate.as_str()))
        .ok_or_else(|| {
            Error::Validation(format!(
                "could not mint a free account id after {ID_ATTEMPTS} attempts"
            ))
        })?;

    let account = Account {
        id: id.clone(),
        name,
        account_type,
        currencies,
        opened_at,
        closed_at: None,
        default_tags: normalize_tags(default_tags),
        description,
    };
    save_markdown(&dest, &account)?;
    Ok((id, dest))
}

/// Change an existing account's default tags. `account` is its id or its
/// name; an id wins over a name, as everywhere else.
///
/// The file is rewritten where it is: the path comes from the name, and the
/// name does not change here. It is rewritten from the known fields, so
/// unknown keys and comments in it are not kept.
pub fn update_account(
    root: &Path,
    account: &str,
    default_tags: Vec<String>,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let loaded: Vec<(Account, PathBuf)> = walk_markdown_files(&root.join(ACCOUNTS_DIR))?
        .into_iter()
        .map(|p| Ok((load_markdown::<Account>(&p)?, p)))
        .collect::<Result<_>>()?;
    let (mut found, path) = loaded
        .iter()
        .find(|(a, _)| a.id == account)
        .or_else(|| loaded.iter().find(|(a, _)| a.name == account))
        .cloned()
        .ok_or_else(|| Error::Validation(format!("no account {account}")))?;
    found.default_tags = normalize_tags(default_tags);
    save_markdown(&path, &found)?;
    Ok((found.id, path))
}

/// Write a new transaction, resolving and validating before touching disk.
#[allow(clippy::too_many_arguments)]
pub fn create_transaction(
    root: &Path,
    date: NaiveDate,
    narration: String,
    payee: Option<String>,
    tags: Vec<String>,
    status: Option<TransactionStatus>,
    postings: Vec<Posting>,
    comment: Option<String>,
    apply_default_tags: bool,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let accounts = load_accounts(root)?;
    let postings = resolve_postings(postings, &accounts)?;
    let tags = if apply_default_tags {
        with_default_tags(tags, &postings, &accounts)
    } else {
        tags
    };

    let (id, dest) = mint_free_id(root, TRANSACTIONS_DIR, MARKDOWN_EXTENSION, |id| {
        transaction_relative_path(date, id)
    })?;
    let timestamp = now();
    let transaction = Transaction {
        id: id.clone(),
        date,
        narration,
        payee,
        tags,
        status,
        created_at: timestamp,
        updated_at: timestamp,
        postings,
        comment,
    };
    transaction.validate()?;
    save_markdown(&dest, &transaction)?;
    Ok((id, dest))
}

/// Write a new balance assertion.
pub fn create_assertion(
    root: &Path,
    account_id: Option<String>,
    account_name: Option<String>,
    date: NaiveDate,
    balances: Vec<Balance>,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    if balances.is_empty() {
        return Err(Error::Validation(
            "assertion must declare at least one balance".into(),
        ));
    }
    let accounts = load_accounts(root)?;
    let by_id: HashMap<&str, &Account> = accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> = accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    let account = resolve_account(
        account_id.as_deref(),
        account_name.as_deref(),
        &by_id,
        &by_name,
    )?;
    for balance in &balances {
        if !account.allows_currency(&balance.currency) {
            return Err(Error::Validation(format!(
                "asserts {} balance for {}, but that account only allows {}",
                balance.currency,
                account.name,
                account.currencies.join(", "),
            )));
        }
    }

    let (id, dest) = mint_free_id(root, ASSERTIONS_DIR, TOML_EXTENSION, |id| {
        assertion_relative_path(date, id)
    })?;
    let timestamp = now();
    let assertion = Assertion {
        id: id.clone(),
        account_id: Some(account.id.clone()),
        account_name: Some(account.name.clone()),
        date,
        balances,
        created_at: timestamp,
        updated_at: timestamp,
    };
    save_toml(&dest, &assertion)?;
    Ok((id, dest))
}

/// Write a new price-log entry.
pub fn create_price(
    root: &Path,
    date: NaiveDate,
    base: String,
    quote: String,
    rate: Decimal,
    source: Option<String>,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    if base == quote {
        return Err(Error::Validation(format!(
            "price base and quote are both {base}"
        )));
    }
    let (id, dest) = mint_free_id(root, PRICES_DIR, TOML_EXTENSION, |id| {
        price_relative_path(date, id)
    })?;
    let timestamp = now();
    let entry = PriceEntry {
        id: id.clone(),
        date,
        base,
        quote,
        rate,
        source,
        created_at: timestamp,
        updated_at: timestamp,
    };
    save_toml(&dest, &entry)?;
    Ok((id, dest))
}

/// Every budget on disk, with its path.
fn load_budgets(root: &Path) -> Result<Vec<(Budget, PathBuf)>> {
    walk_files(&root.join(BUDGETS_DIR), TOML_EXTENSION)?
        .into_iter()
        .map(|p| Ok((load_toml::<Budget>(&p)?, p)))
        .collect()
}

/// Resolve a budget's balance accounts to id and current name, refusing an
/// account that does not exist or cannot hold the budget's currency.
fn resolve_refs(
    refs: Vec<AccountRef>,
    currency: &str,
    accounts: &[Account],
) -> Result<Vec<AccountRef>> {
    let by_id: HashMap<&str, &Account> = accounts.iter().map(|a| (a.id.as_str(), a)).collect();
    let by_name: HashMap<&str, &Account> = accounts.iter().map(|a| (a.name.as_str(), a)).collect();
    refs.into_iter()
        .map(|r| {
            let account = resolve_account(
                r.account_id.as_deref(),
                r.account_name.as_deref(),
                &by_id,
                &by_name,
            )?;
            if !account.allows_currency(currency) {
                return Err(Error::Validation(format!(
                    "{} cannot hold {currency}; it only allows {}",
                    account.name,
                    account.currencies.join(", "),
                )));
            }
            Ok(AccountRef {
                account_id: Some(account.id.clone()),
                account_name: Some(account.name.clone()),
            })
        })
        .collect()
}

fn check_budget(budget: &Budget) -> Result<()> {
    for (field, value) in [
        ("name", &budget.name),
        ("tag", &budget.tag),
        ("currency", &budget.currency),
    ] {
        if value.trim().is_empty() {
            return Err(Error::Validation(format!("a budget needs a {field}")));
        }
    }
    if budget.monthly_amount <= Decimal::ZERO {
        return Err(Error::Validation(format!(
            "budget {}: monthly_amount {} is not positive",
            budget.name, budget.monthly_amount
        )));
    }
    Ok(())
}

/// Write a new budget. Its name must be unused: it is what people type.
pub fn create_budget(
    root: &Path,
    name: String,
    tag: String,
    currency: String,
    monthly_amount: Decimal,
    balance_accounts: Vec<AccountRef>,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let existing = load_budgets(root)?;
    let name = name.trim().to_owned();
    if existing.iter().any(|(b, _)| b.name == name) {
        return Err(Error::Validation(format!(
            "a budget named {name} already exists"
        )));
    }
    let accounts = load_accounts(root)?;
    let balance_accounts = resolve_refs(balance_accounts, currency.trim(), &accounts)?;
    let taken: HashSet<&str> = existing.iter().map(|(b, _)| b.id.as_str()).collect();
    let id = (0..ID_ATTEMPTS)
        .map(|_| new_random_id())
        .find(|candidate| !taken.contains(candidate.as_str()))
        .ok_or_else(|| {
            Error::Validation(format!(
                "could not mint a free budget id after {ID_ATTEMPTS} attempts"
            ))
        })?;
    let dest = root.join(budget_relative_path(&id));
    refuse_existing(&dest)?;
    let timestamp = now();
    let budget = Budget {
        id: id.clone(),
        name,
        tag: tag.trim().to_owned(),
        currency: currency.trim().to_owned(),
        monthly_amount,
        created_at: timestamp,
        updated_at: timestamp,
        balance_accounts,
    };
    check_budget(&budget)?;
    save_toml(&dest, &budget)?;
    Ok((id, dest))
}

/// What `update_budget` changes; `None` leaves a field as it is.
#[derive(Debug, Default)]
pub struct BudgetChanges {
    pub name: Option<String>,
    pub tag: Option<String>,
    pub currency: Option<String>,
    pub monthly_amount: Option<Decimal>,
    pub balance_accounts: Option<Vec<AccountRef>>,
}

/// Change an existing budget, found by id or name, in place. The file is
/// rewritten from the known fields, so unknown keys and comments in it are
/// not kept.
pub fn update_budget(
    root: &Path,
    budget: &str,
    changes: BudgetChanges,
) -> Result<(String, PathBuf)> {
    refuse_other_schema(root)?;
    let existing = load_budgets(root)?;
    let (mut found, path) = existing
        .iter()
        .find(|(b, _)| b.id == budget)
        .or_else(|| existing.iter().find(|(b, _)| b.name == budget))
        .cloned()
        .ok_or_else(|| Error::Validation(format!("no budget {budget}")))?;
    if let Some(name) = changes.name {
        let name = name.trim().to_owned();
        if existing
            .iter()
            .any(|(b, _)| b.name == name && b.id != found.id)
        {
            return Err(Error::Validation(format!(
                "a budget named {name} already exists"
            )));
        }
        found.name = name;
    }
    if let Some(tag) = changes.tag {
        found.tag = tag.trim().to_owned();
    }
    if let Some(currency) = changes.currency {
        found.currency = currency.trim().to_owned();
    }
    if let Some(amount) = changes.monthly_amount {
        found.monthly_amount = amount;
    }
    let refs = changes.balance_accounts.unwrap_or(found.balance_accounts);
    found.balance_accounts = resolve_refs(refs, &found.currency, &load_accounts(root)?)?;
    found.updated_at = now();
    check_budget(&found)?;
    save_toml(&path, &found)?;
    Ok((found.id, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn both_generators_produce_seven_chars() {
        assert_eq!(new_id().chars().count(), 7);
        assert_eq!(new_random_id().chars().count(), 7);
    }

    /// `mint_free_id` scopes uniqueness to the kind, not to a month directory.
    ///
    /// A record dated in May takes an id; a record minted in the same
    /// decisecond but dated in June asks `new_id()` for the *same* string,
    /// and its destination path is in a different directory — so only a
    /// kind-wide check can catch it.
    ///
    /// The clock is not controllable, so the round is bracketed with
    /// `new_id()` and only asserted on when it provably stayed inside one
    /// decisecond.
    #[test]
    fn mint_free_id_will_not_reuse_an_id_from_another_month() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let may: NaiveDate = "2026-05-21".parse().unwrap();
        let june: NaiveDate = "2026-06-03".parse().unwrap();

        let mut sampled = 0usize;
        for round in 0..40 {
            let before = new_id();
            let occupied = root.join(transaction_relative_path(may, &before));
            std::fs::create_dir_all(occupied.parent().unwrap()).unwrap();
            std::fs::write(&occupied, "").unwrap();

            let (minted, _) = mint_free_id(root, TRANSACTIONS_DIR, MARKDOWN_EXTENSION, |id| {
                transaction_relative_path(june, id)
            })
            .expect("mint");
            let after = new_id();
            if before == after {
                sampled += 1;
                assert_ne!(
                    minted, before,
                    "minted an id already taken by a record in another month (round {round})"
                );
            }
        }
        assert!(
            sampled > 0,
            "no round stayed inside one decisecond; the collision window was never exercised"
        );
    }

    #[test]
    fn random_ids_vary_in_their_leading_character() {
        // The whole reason accounts use random(): a burst of ids must not
        // share a prefix. 200 draws over a 32-char alphabet hitting only one
        // leading character would be about 1e-300 -- so this is a real signal,
        // not a flaky threshold.
        let leads: HashSet<char> = (0..200)
            .filter_map(|_| new_random_id().chars().next())
            .collect();
        assert!(
            leads.len() > 1,
            "random ids all began with the same character"
        );
    }
}
