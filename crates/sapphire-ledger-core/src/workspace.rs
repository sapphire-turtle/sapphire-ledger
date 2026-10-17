use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};

use crate::account::account_name_segments;
use crate::config::{CURRENT_SCHEMA_VERSION, CacheConfig, Config};
use crate::error::{Error, Result};

pub const WORKSPACE_DIR: &str = ".sapphire-ledger";
pub const CONFIG_FILE: &str = "config.toml";
pub const CACHE_FILE: &str = "cache.sqlite";

pub const TRANSACTIONS_DIR: &str = "transactions";
pub const ACCOUNTS_DIR: &str = "accounts";
pub const ASSERTIONS_DIR: &str = "assertions";
pub const PRICES_DIR: &str = "prices";
pub const BUDGETS_DIR: &str = "budgets";

pub const TOML_EXTENSION: &str = "toml";
pub const MARKDOWN_EXTENSION: &str = "md";

/// Walk upward from `start` looking for a directory containing `.sapphire-ledger/`.
/// Returns the workspace root (the directory containing `.sapphire-ledger/`, not
/// `.sapphire-ledger/` itself).
pub fn find_workspace_root(start: &Path) -> Result<PathBuf> {
    let start_canonical = start.canonicalize()?;
    let mut current: &Path = &start_canonical;
    loop {
        if current.join(WORKSPACE_DIR).is_dir() {
            return Ok(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return Err(Error::NotAWorkspace(start.to_path_buf())),
        }
    }
}

/// Convert an account name (e.g. `"Assets:Cash:USD"`) into its relative path
/// under `accounts/` (e.g. `accounts/Assets/Cash/USD.md`).
pub fn account_relative_path(name: &str) -> Result<PathBuf> {
    let segments = account_name_segments(name)?;
    let mut path = PathBuf::from(ACCOUNTS_DIR);
    for (i, seg) in segments.iter().enumerate() {
        if i + 1 == segments.len() {
            path.push(format!("{seg}.{MARKDOWN_EXTENSION}"));
        } else {
            path.push(seg);
        }
    }
    Ok(path)
}

/// Inverse of [`account_relative_path`]: given a path relative to the
/// workspace root (e.g. `accounts/Assets/Cash/USD.md`), reconstruct the
/// account name `"Assets:Cash:USD"`.
pub fn account_name_from_relative_path(rel: &Path) -> Result<String> {
    let stripped = rel.strip_prefix(ACCOUNTS_DIR).map_err(|_| {
        Error::Validation(format!(
            "path is not under {ACCOUNTS_DIR}/: {}",
            rel.display()
        ))
    })?;
    let mut segments: Vec<String> = Vec::new();
    let components: Vec<_> = stripped.components().collect();
    if components.is_empty() {
        return Err(Error::Validation(format!(
            "account path has no segments: {}",
            rel.display()
        )));
    }
    for (i, component) in components.iter().enumerate() {
        let part = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| Error::Validation(format!("non-UTF-8 path: {}", rel.display())))?;
        if i + 1 == components.len() {
            let trimmed = part
                .strip_suffix(&format!(".{MARKDOWN_EXTENSION}"))
                .ok_or_else(|| {
                    Error::Validation(format!(
                        "account file does not end with .{MARKDOWN_EXTENSION}: {}",
                        rel.display()
                    ))
                })?;
            segments.push(trimmed.to_string());
        } else {
            segments.push(part.to_string());
        }
    }
    Ok(segments.join(&crate::account::ACCOUNT_NAME_SEPARATOR.to_string()))
}

/// Relative path for a transaction file: `transactions/{year}/{MM}/{id}.md`.
pub fn transaction_relative_path(date: NaiveDate, id: &str) -> PathBuf {
    PathBuf::from(TRANSACTIONS_DIR)
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!("{id}.{MARKDOWN_EXTENSION}"))
}

/// Relative path for an assertion file: `assertions/{year}/{MM}/{id}.toml`.
pub fn assertion_relative_path(date: NaiveDate, id: &str) -> PathBuf {
    PathBuf::from(ASSERTIONS_DIR)
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!("{id}.{TOML_EXTENSION}"))
}

/// Relative path for a price-log entry: `prices/{year}/{MM}/{id}.toml`.
pub fn price_relative_path(date: NaiveDate, id: &str) -> PathBuf {
    PathBuf::from(PRICES_DIR)
        .join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!("{id}.{TOML_EXTENSION}"))
}

/// Relative path for a budget: `budgets/{id}.toml`. A budget is not dated,
/// so there is no `{year}/{MM}/`.
pub fn budget_relative_path(id: &str) -> PathBuf {
    PathBuf::from(BUDGETS_DIR).join(format!("{id}.{TOML_EXTENSION}"))
}

/// Create a new sapphire-ledger workspace at `target`. Creates the directory
/// if missing. Errors if `target` already holds a ledger.
///
/// A `.sapphire-ledger/` the framework made first (`sapphire-ledger workspace
/// init`) is not a ledger yet: since sapphire-framework 0.20 it holds no
/// `config.toml`, and an older one wrote only its own `[workspace.*]` tables
/// there. The ledger's settings are added beside whatever is there, and the
/// record directories are created. Only `schema_version` marks a ledger.
pub fn init_workspace(target: &Path, base_currency: &str) -> Result<()> {
    fs::create_dir_all(target)?;
    let workspace_dir = target.join(WORKSPACE_DIR);
    let config_path = workspace_dir.join(CONFIG_FILE);

    // What a marker someone else made already holds, kept as-is.
    let mut document = match fs::read_to_string(&config_path) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map_err(|e| Error::Validation(format!("{}: {e}", config_path.display())))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(e.into()),
    };
    if document.contains_key("schema_version") {
        return Err(Error::Validation(format!(
            "{} already contains a sapphire-ledger workspace",
            target.display()
        )));
    }

    fs::create_dir_all(&workspace_dir)?;
    fs::create_dir_all(target.join(TRANSACTIONS_DIR))?;
    fs::create_dir_all(target.join(ACCOUNTS_DIR))?;
    fs::create_dir_all(target.join(ASSERTIONS_DIR))?;
    fs::create_dir_all(target.join(PRICES_DIR))?;

    let config = Config {
        schema_version: CURRENT_SCHEMA_VERSION,
        base_currency: base_currency.to_string(),
        cache: CacheConfig::default(),
    };
    let toml::Value::Table(ours) = toml::Value::try_from(&config)? else {
        unreachable!("a struct serializes to a table");
    };
    document.extend(ours);
    fs::write(&config_path, toml::to_string_pretty(&document)?)?;
    let gitignore = workspace_dir.join(".gitignore");
    if !gitignore.exists() {
        fs::write(gitignore, format!("{CACHE_FILE}\n"))?;
    }

    Ok(())
}
