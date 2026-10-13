//! Upgrade a ledger written by an older build to [`CURRENT_SCHEMA_VERSION`].
//!
//! Version 1 to 2: every account and transaction moves from TOML to Markdown
//! with YAML front matter, in place (`USD.toml` becomes `USD.md`), its
//! description or comment becoming the body. Assertions and prices stay TOML.
//!
//! This rewrites a ledger people are already keeping, so it is built to be
//! safe to re-run and safe to interrupt:
//!
//! - Nothing is written until every TOML record has parsed, has been shown to
//!   read back unchanged from its Markdown, and has been checked against any
//!   Markdown file already at its destination.
//! - `schema_version` is bumped last. An interrupted run leaves a version-1
//!   ledger with some records moved, which `load_workspace` refuses and a
//!   re-run finishes: a `.md` already holding the same record is kept, and
//!   only the `.toml` beside it is removed.
//!
//! TOML `#` comments have nowhere to go and are dropped; the files that look
//! like they had some are listed in the report.

use std::fs;
use std::path::{Path, PathBuf};

use crate::account::Account;
use crate::config::CURRENT_SCHEMA_VERSION;
use crate::error::{Error, Result};
use crate::repository::{
    MarkdownRecord, at, check_schema_version, from_markdown, load_markdown, load_toml, to_markdown,
    walk_toml_files,
};
use crate::transaction::Transaction;
use crate::workspace::{
    ACCOUNTS_DIR, CONFIG_FILE, MARKDOWN_EXTENSION, TRANSACTIONS_DIR, WORKSPACE_DIR,
};

/// What [`migrate_workspace`] did, or would do on a dry run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub from_version: u32,
    pub to_version: u32,
    /// Each TOML record and the Markdown file that replaces it.
    pub converted: Vec<(PathBuf, PathBuf)>,
    /// TOML records that look like they carry `#` comments, which the move
    /// drops.
    pub comments_dropped: Vec<PathBuf>,
}

/// One TOML record and its Markdown replacement.
struct Move {
    from: PathBuf,
    to: PathBuf,
    /// The Markdown to write, or `None` when `to` already holds this record.
    text: Option<String>,
}

/// Bring the ledger at `root` up to [`CURRENT_SCHEMA_VERSION`]. With
/// `dry_run`, check everything and report, but write nothing.
///
/// Also sweeps up TOML accounts and transactions in a ledger that is already
/// current — what an old client syncs in after another machine migrated.
pub fn migrate_workspace(root: &Path, dry_run: bool) -> Result<MigrationReport> {
    let config_path = root.join(WORKSPACE_DIR).join(CONFIG_FILE);
    let config_text = fs::read_to_string(&config_path).map_err(|e| at(&config_path)(e.into()))?;
    let from_version = read_schema_version(&config_text).map_err(at(&config_path))?;
    if from_version > CURRENT_SCHEMA_VERSION {
        check_schema_version(&config_path, from_version)?;
    }

    let mut moves = Vec::new();
    let mut comments_dropped = Vec::new();
    plan::<Account>(root, ACCOUNTS_DIR, &mut moves, &mut comments_dropped)?;
    plan::<Transaction>(root, TRANSACTIONS_DIR, &mut moves, &mut comments_dropped)?;

    let report = MigrationReport {
        from_version,
        to_version: CURRENT_SCHEMA_VERSION,
        converted: moves
            .iter()
            .map(|m| (m.from.clone(), m.to.clone()))
            .collect(),
        comments_dropped,
    };
    if dry_run {
        return Ok(report);
    }

    for Move { from, to, text } in &moves {
        if let Some(text) = text {
            fs::write(to, text).map_err(|e| at(to)(e.into()))?;
        }
        fs::remove_file(from).map_err(|e| at(from)(e.into()))?;
    }

    if from_version != CURRENT_SCHEMA_VERSION {
        let updated =
            set_schema_version(&config_text, CURRENT_SCHEMA_VERSION).ok_or_else(|| {
                at(&config_path)(Error::Validation(
                    "could not find a top-level `schema_version` to update".into(),
                ))
            })?;
        fs::write(&config_path, updated).map_err(|e| at(&config_path)(e.into()))?;
    }
    Ok(report)
}

/// Work out the move for every TOML record of kind `T` under `root/dir`,
/// failing before anything is written if any of them cannot move safely.
fn plan<T: MarkdownRecord + PartialEq>(
    root: &Path,
    dir: &str,
    moves: &mut Vec<Move>,
    comments_dropped: &mut Vec<PathBuf>,
) -> Result<()> {
    for from in walk_toml_files(&root.join(dir))? {
        let mut record: T = load_toml(&from)?;
        // The body is trimmed on the way in and out; trim it here too, so a
        // description that ended in a newline compares equal after the move.
        let body = record
            .body()
            .map(|b| b.trim_start_matches(['\n', '\r']).trim_end().to_string())
            .filter(|b| !b.is_empty());
        record.set_body(body);

        let text = to_markdown(&record).map_err(at(&from))?;
        if from_markdown::<T>(&text).map_err(at(&from))? != record {
            return Err(at(&from)(Error::Validation(
                "does not read back unchanged as Markdown; left as TOML".into(),
            )));
        }

        let to = from.with_extension(MARKDOWN_EXTENSION);
        let text = if to.exists() {
            if load_markdown::<T>(&to)? != record {
                return Err(at(&to)(Error::Validation(format!(
                    "already exists and differs from {}; reconcile the two by hand",
                    from.display()
                ))));
            }
            None
        } else {
            Some(text)
        };

        let raw = fs::read_to_string(&from).map_err(|e| at(&from)(e.into()))?;
        if has_comments(&raw) {
            comments_dropped.push(from.clone());
        }
        moves.push(Move { from, to, text });
    }
    Ok(())
}

fn read_schema_version(config_text: &str) -> Result<u32> {
    let table: toml::Table = config_text.parse()?;
    table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| Error::Validation("no `schema_version`; not a sapphire-ledger".into()))
}

/// Rewrite the top-level `schema_version` line of `config.toml` and nothing
/// else, so the comments and the framework's own tables stay as written.
fn set_schema_version(config_text: &str, version: u32) -> Option<String> {
    let mut out = String::with_capacity(config_text.len());
    let mut top_level = true;
    let mut replaced = false;
    for line in config_text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            top_level = false;
        }
        let is_key = top_level
            && !replaced
            && trimmed
                .strip_prefix("schema_version")
                .is_some_and(|rest| rest.trim_start().starts_with('='));
        if is_key {
            // Only the number: a trailing comment stays.
            let eq = line.find('=')? + 1;
            let start = eq + (line[eq..].len() - line[eq..].trim_start().len());
            let end = start
                + line[start..]
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(line.len() - start);
            out.push_str(&line[..start]);
            out.push_str(&version.to_string());
            out.push_str(&line[end..]);
            replaced = true;
        } else {
            out.push_str(line);
        }
    }
    (replaced && read_schema_version(&out).ok() == Some(version)).then_some(out)
}

/// Whether a TOML file looks like it has a `#` comment: a `#` with an even
/// number of each quote character before it on its line. A `#` inside a
/// multi-line string can fool it; it errs towards listing a file.
fn has_comments(toml_text: &str) -> bool {
    toml_text.lines().any(|line| {
        line.match_indices('#').any(|(i, _)| {
            let before = &line[..i];
            before.matches('"').count() % 2 == 0 && before.matches('\'').count() % 2 == 0
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_schema_version_keeps_everything_else() {
        let text = "# my ledger\nschema_version = 1 # the layout\nbase_currency = \"JPY\"\n\n[workspace.sync]\nschema_version = 1\n";
        let updated = set_schema_version(text, 2).expect("updated");
        assert_eq!(
            updated,
            "# my ledger\nschema_version = 2 # the layout\nbase_currency = \"JPY\"\n\n[workspace.sync]\nschema_version = 1\n"
        );
    }

    #[test]
    fn set_schema_version_keeps_crlf() {
        let updated = set_schema_version("schema_version = 1\r\nbase_currency = \"JPY\"\r\n", 2)
            .expect("updated");
        assert_eq!(updated, "schema_version = 2\r\nbase_currency = \"JPY\"\r\n");
    }

    #[test]
    fn set_schema_version_needs_the_key() {
        assert_eq!(set_schema_version("base_currency = \"JPY\"\n", 2), None);
    }

    #[test]
    fn has_comments_ignores_a_hash_in_a_string() {
        assert!(!has_comments("narration = \"room #3\"\n"));
        assert!(has_comments("# a note\nid = \"x\"\n"));
        assert!(has_comments("amount = \"1200\" # tax incl.\n"));
    }
}
