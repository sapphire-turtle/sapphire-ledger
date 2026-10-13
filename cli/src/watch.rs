//! Watches the workspace for one id claimed by more than one file, and does
//! nothing else about it.
//!
//! A ledger record's path embeds mutable data: a transaction's or
//! assertion's path is derived from its *date*, an account's from its
//! *name*. Sync resolves conflicts per path, last-writer-wins, with deletes
//! as tombstones under the same comparison. A client that was offline across a
//! rename or a date correction can push to the old path with a later
//! timestamp; the tombstone loses, and the record now exists at two paths
//! under one id.
//!
//! This module reports that and stops. It never resolves anything, and that
//! is a decision, not an omission:
//!
//! - The sibling `sapphire-journal` converges automatically by re-iding the
//!   later arrival and keeping both records. That is right for notes -- a
//!   note that exists twice is at worst annoying. For a ledger it would
//!   double-count a transaction, which is a wrong balance.
//! - The obvious alternative -- last-writer-wins on the record's own
//!   `updated_at` -- is worse, not better: that timestamp is client-supplied
//!   wall clock, so a machine with a skewed clock could silently delete a
//!   correct transaction.
//!
//! Where money is concerned, a loud broken state beats a quiet resolved one.
//! Automatic convergence is a decision to make with a real duplicate in
//! front of you, not one to bake in ahead of time.
//!
//! Nothing here ever writes, moves or deletes a file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sapphire_ledger_core::workspace::{ACCOUNTS_DIR, ASSERTIONS_DIR, PRICES_DIR, TRANSACTIONS_DIR};
use sapphire_ledger_core::{
    Account, Assertion, PriceEntry, Transaction, load_markdown, load_toml, walk_markdown_files,
    walk_toml_files,
};

/// How often the watch re-walks the workspace looking for duplicate ids.
///
/// A constant, not a setting: nothing about this task suggests a workspace
/// needs a different cadence, and a knob nobody asked for is a knob someone
/// else has to maintain.
const CHECK_INTERVAL: Duration = Duration::from_secs(300);

/// Walk the record directories under `root` and report every id (or, for
/// accounts, every id *and* every name) claimed by more than one file.
///
/// Returns one human-readable line per duplicate, naming the id (or name)
/// and every path carrying it. `ValidationIssue` -- what `Workspace::validate()`
/// already returns, and which already flags duplicate ids -- carries the ids
/// but not the paths, and the paths are what a human needs to fix this. So
/// this walks the directories itself rather than
/// reformatting `validate()`'s output.
///
/// A file that fails to parse is skipped, not fatal, and so is a directory
/// that fails to walk at all (for instance a file removed by another writer
/// between listing and reading it): this runs on a timer against a
/// directory other processes are actively writing to, and one bad record or
/// one raced kind must not silence the duplicate report for the rest. Both
/// are logged at `debug`, and the four kinds below are walked independently
/// so a failure in one never discards what the other three found.
pub fn duplicate_report(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::new();
    extend_or_log(
        &mut lines,
        "transaction",
        duplicates_of(
            root,
            TRANSACTIONS_DIR,
            Markdown,
            "transaction id",
            |t: &Transaction| t.id.clone(),
        ),
    );
    extend_or_log(
        &mut lines,
        "assertion",
        duplicates_of(
            root,
            ASSERTIONS_DIR,
            Toml,
            "assertion id",
            |a: &Assertion| a.id.clone(),
        ),
    );
    extend_or_log(
        &mut lines,
        "price",
        duplicates_of(root, PRICES_DIR, Toml, "price id", |p: &PriceEntry| {
            p.id.clone()
        }),
    );
    // Accounts need both keys. The path is derived from the account's
    // *name*, so the canonical raced-rename artifact is one id living at two
    // paths under two *different* names -- name-grouping alone reports
    // nothing for that case, because the two names really are distinct.
    // Id-grouping is what catches it. Name-grouping still earns its own
    // pass: two accounts that keep distinct ids but claim the same name are
    // also broken, since a name-only reference would resolve to whichever
    // one happened to win the lookup.
    extend_or_log(
        &mut lines,
        "account",
        duplicates_of(root, ACCOUNTS_DIR, Markdown, "account id", |a: &Account| {
            a.id.clone()
        }),
    );
    extend_or_log(
        &mut lines,
        "account",
        duplicates_of(
            root,
            ACCOUNTS_DIR,
            Markdown,
            "account name",
            |a: &Account| a.name.clone(),
        ),
    );
    lines.sort();
    Ok(lines)
}

/// Fold one kind's duplicate lines into the running report, logging (not
/// propagating) a failure to walk that kind's directory. A file vanishing
/// mid-walk because another process is writing to the same tree must not
/// silence the other kinds' results.
fn extend_or_log(lines: &mut Vec<String>, kind: &str, result: anyhow::Result<Vec<String>>) {
    match result {
        Ok(found) => lines.extend(found),
        Err(err) => {
            tracing::debug!("could not walk {kind} records for duplicates: {err:#}");
        }
    }
}

/// How a record kind is kept on disk.
trait Format<T> {
    fn walk(&self, dir: &Path) -> sapphire_ledger_core::Result<Vec<PathBuf>>;
    fn load(&self, path: &Path) -> sapphire_ledger_core::Result<T>;
}

struct Toml;
struct Markdown;

impl<T: serde::de::DeserializeOwned> Format<T> for Toml {
    fn walk(&self, dir: &Path) -> sapphire_ledger_core::Result<Vec<PathBuf>> {
        walk_toml_files(dir)
    }
    fn load(&self, path: &Path) -> sapphire_ledger_core::Result<T> {
        load_toml(path)
    }
}

impl<T: sapphire_ledger_core::MarkdownRecord> Format<T> for Markdown {
    fn walk(&self, dir: &Path) -> sapphire_ledger_core::Result<Vec<PathBuf>> {
        walk_markdown_files(dir)
    }
    fn load(&self, path: &Path) -> sapphire_ledger_core::Result<T> {
        load_markdown(path)
    }
}

/// Load every record of type `T` under `root/dir`, group by the key `key_of`
/// extracts, and return one report line for every key with more than one
/// path.
fn duplicates_of<T, F>(
    root: &Path,
    dir: &str,
    format: impl Format<T>,
    kind: &str,
    key_of: F,
) -> anyhow::Result<Vec<String>>
where
    F: Fn(&T) -> String,
{
    let mut by_key: HashMap<String, Vec<PathBuf>> = HashMap::new();
    for path in format.walk(&root.join(dir))? {
        match format.load(&path) {
            Ok(record) => by_key.entry(key_of(&record)).or_default().push(path),
            Err(err) => {
                let err = anyhow::Error::from(err);
                tracing::debug!(
                    path = %path.display(),
                    "skipping a record that failed to parse: {err:#}"
                );
            }
        }
    }

    // Not sorted here: `duplicate_report` sorts the full concatenation of
    // every kind's lines, so sorting this slice too would be redundant.
    Ok(by_key
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|(key, mut paths)| {
            paths.sort();
            let rendered: Vec<String> = paths
                .iter()
                .map(|path| display_relative(root, path))
                .collect();
            format!("duplicate {kind} {key}: {}", rendered.join(", "))
        })
        .collect())
}

/// Render `path` relative to `root` with `/` separators regardless of
/// platform, so a report line reads the same on the machine that generated
/// it as it does wherever it gets pasted.
fn display_relative(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Spawn the background watch: run [`duplicate_report`] immediately, then
/// again every [`CHECK_INTERVAL`], logging each line it produces at `WARN`.
///
/// `root` is asked on every pass, so the watch follows the server's current
/// workspace; a pass with no workspace selected does nothing.
///
/// Only duplicates are logged here, and only at `WARN` -- the rest of
/// `Workspace::validate()`'s output is the ledger's ordinary business and
/// belongs to the `validate_workspace` MCP tool, not to the server's log.
pub fn spawn(root: impl Fn() -> Option<PathBuf> + Send + 'static) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CHECK_INTERVAL);
        loop {
            interval.tick().await;
            let Some(root) = root() else { continue };
            match duplicate_report(&root) {
                Ok(lines) => {
                    for line in lines {
                        tracing::warn!("{line}");
                    }
                }
                Err(err) => {
                    tracing::debug!(
                        root = %root.display(),
                        "duplicate-id watch could not walk the workspace: {err:#}"
                    );
                }
            }
        }
    })
}
