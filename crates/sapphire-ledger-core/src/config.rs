use serde::{Deserialize, Serialize};

/// The on-disk layout this build reads and writes.
///
/// 1. Every record is TOML.
/// 2. Accounts and transactions are Markdown with YAML front matter; their
///    description / comment is the body. `sapphire-ledger migrate` upgrades
///    a version-1 ledger.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub base_currency: String,
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_scan_interval")]
    pub scan_interval: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            scan_interval: default_scan_interval(),
        }
    }
}

fn default_scan_interval() -> u64 {
    60
}
