use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::account::Account;
use crate::assertion::Assertion;
use crate::budget::Budget;
use crate::config::{CURRENT_SCHEMA_VERSION, Config};
use crate::error::{Error, Result};
use crate::prices::PriceEntry;
use crate::transaction::Transaction;
use crate::workspace::{
    ACCOUNTS_DIR, ASSERTIONS_DIR, BUDGETS_DIR, CONFIG_FILE, MARKDOWN_EXTENSION, PRICES_DIR,
    TOML_EXTENSION, TRANSACTIONS_DIR, WORKSPACE_DIR,
};

/// A record kept as Markdown: YAML front matter for its fields, and one
/// free-text field as the body.
///
/// The body field is an ordinary `Option<String>` on the type, so the record
/// serializes the same everywhere else (MCP, the app server); only the file
/// moves it out of the front matter.
pub trait MarkdownRecord: Serialize + DeserializeOwned + Clone {
    /// The field the body holds, for error messages.
    const BODY_FIELD: &'static str;

    fn body(&self) -> Option<&str>;

    fn set_body(&mut self, body: Option<String>);
}

/// Tag a failure with the file it came from.
///
/// `load_workspace` is fail-fast across the whole ledger, so without the path
/// a single bad record aborts the load with a parse error naming a line
/// number in a file nobody can identify.
pub(crate) fn at(path: &Path) -> impl Fn(Error) -> Error + '_ {
    move |source| Error::Record {
        path: path.to_path_buf(),
        source: Box::new(source),
    }
}

/// Read a TOML file and deserialize it into `T`.
pub fn load_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|e| at(path)(e.into()))?;
    toml::from_str(&text).map_err(|e| at(path)(e.into()))
}

/// Serialize `value` as TOML and write it to `path`, creating parent
/// directories as needed.
pub fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_creating_parents(path, &toml::to_string_pretty(value)?)
}

/// Read a Markdown record: the front matter into `T`, the body into
/// [`MarkdownRecord::BODY_FIELD`].
///
/// A body field written in the front matter instead is accepted while the
/// body is empty — someone writing the file by hand should not have to know
/// which field lives where — and rejected when both are present, since there
/// is then no telling which one was meant.
pub fn from_markdown<T: MarkdownRecord>(text: &str) -> Result<T> {
    let (mut record, body): (T, &str) = sapphire_frontmatter::parse(text)?;
    let body = body.trim_start_matches(['\n', '\r']).trim_end();
    if !body.is_empty() {
        if record.body().is_some() {
            return Err(Error::Validation(format!(
                "`{}` is both in the front matter and the body; keep only the body",
                T::BODY_FIELD
            )));
        }
        record.set_body(Some(body.to_string()));
    }
    Ok(record)
}

/// Render a Markdown record: every field but the body field as front matter,
/// then that field as the body.
pub fn to_markdown<T: MarkdownRecord>(record: &T) -> Result<String> {
    let mut front = record.clone();
    front.set_body(None);
    let body = match record.body().map(str::trim_end) {
        Some(body) if !body.is_empty() => format!("{body}\n"),
        _ => String::new(),
    };
    Ok(sapphire_frontmatter::render(&front, &body)?)
}

/// Read a Markdown record file. See [`from_markdown`].
pub fn load_markdown<T: MarkdownRecord>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).map_err(|e| at(path)(e.into()))?;
    from_markdown(&text).map_err(at(path))
}

/// Write a Markdown record file, creating parent directories as needed. See
/// [`to_markdown`].
pub fn save_markdown<T: MarkdownRecord>(path: &Path, record: &T) -> Result<()> {
    write_creating_parents(path, &to_markdown(record)?)
}

fn write_creating_parents(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

/// Recursively collect all files under `root` with the given extension.
/// Returns an empty Vec if `root` does not exist (the directory is allowed to
/// be absent for an empty workspace).
pub fn walk_files(root: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    if !root.exists() {
        return Ok(result);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == extension) {
                result.push(path);
            }
        }
    }
    result.sort();
    Ok(result)
}

/// Recursively collect all `.toml` files under `root`. See [`walk_files`].
pub fn walk_toml_files(root: &Path) -> Result<Vec<PathBuf>> {
    walk_files(root, TOML_EXTENSION)
}

/// Recursively collect all `.md` files under `root`. See [`walk_files`].
pub fn walk_markdown_files(root: &Path) -> Result<Vec<PathBuf>> {
    walk_files(root, MARKDOWN_EXTENSION)
}

/// An eagerly-loaded in-memory view of a sapphire-ledger workspace.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub config: Config,
    pub accounts: Vec<Account>,
    pub transactions: Vec<Transaction>,
    pub assertions: Vec<Assertion>,
    pub prices: Vec<PriceEntry>,
    pub budgets: Vec<Budget>,
}

/// Load all records from a workspace at `root` (the directory containing
/// `.sapphire-ledger/`).
pub fn load_workspace(root: &Path) -> Result<Workspace> {
    let config_path = root.join(WORKSPACE_DIR).join(CONFIG_FILE);
    let config: Config = load_toml(&config_path)?;
    check_schema_version(&config_path, config.schema_version)?;

    // A TOML account or transaction is a schema-1 record: one an old client
    // synced in after the migration, or one the migration has not reached.
    // Skipping it would leave the books quietly wrong.
    for dir in [ACCOUNTS_DIR, TRANSACTIONS_DIR] {
        if let Some(path) = walk_toml_files(&root.join(dir))?.into_iter().next() {
            return Err(at(&path)(Error::Validation(
                "a TOML record in a Markdown ledger; run `sapphire-ledger migrate`".into(),
            )));
        }
    }

    let accounts = walk_markdown_files(&root.join(ACCOUNTS_DIR))?
        .iter()
        .map(|p| load_markdown::<Account>(p))
        .collect::<Result<Vec<_>>>()?;

    let transactions = walk_markdown_files(&root.join(TRANSACTIONS_DIR))?
        .iter()
        .map(|p| load_markdown::<Transaction>(p))
        .collect::<Result<Vec<_>>>()?;

    let assertions = walk_toml_files(&root.join(ASSERTIONS_DIR))?
        .iter()
        .map(|p| load_toml::<Assertion>(p))
        .collect::<Result<Vec<_>>>()?;

    let prices = walk_toml_files(&root.join(PRICES_DIR))?
        .iter()
        .map(|p| load_toml::<PriceEntry>(p))
        .collect::<Result<Vec<_>>>()?;

    let budgets = walk_toml_files(&root.join(BUDGETS_DIR))?
        .iter()
        .map(|p| load_toml::<Budget>(p))
        .collect::<Result<Vec<_>>>()?;

    Ok(Workspace {
        root: root.to_path_buf(),
        config,
        accounts,
        transactions,
        assertions,
        prices,
        budgets,
    })
}

/// Refuse a ledger this build does not read: an older one has to be
/// migrated first, a newer one needs a newer build.
pub fn check_schema_version(config_path: &Path, found: u32) -> Result<()> {
    if found == CURRENT_SCHEMA_VERSION {
        return Ok(());
    }
    let reason = if found < CURRENT_SCHEMA_VERSION {
        format!(
            "schema version {found} is older than {CURRENT_SCHEMA_VERSION}; \
             run `sapphire-ledger migrate`"
        )
    } else {
        format!(
            "schema version {found} is newer than {CURRENT_SCHEMA_VERSION}, \
             the newest this build reads; upgrade sapphire-ledger"
        )
    };
    Err(at(config_path)(Error::Validation(reason)))
}
