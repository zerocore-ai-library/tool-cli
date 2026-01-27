//! Constants for tool-cli.
//!
//! This module contains all path and configuration constants.
//! Review these to ensure they match your environment.

use std::path::PathBuf;
use std::sync::LazyLock;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// The manifest file name for MCPB bundles.
pub const MCPB_MANIFEST_FILE: &str = "manifest.json";

/// File extension for standard MCPB bundles.
pub const MCPB_EXT: &str = "mcpb";

/// File extension for MCPB extended bundles (reference mode, HTTP, system_config, etc.).
pub const MCPBX_EXT: &str = "mcpbx";

/// Default registry URL.
pub const DEFAULT_REGISTRY_URL: &str = "https://tool.store";

/// Environment variable for custom registry URL.
pub const TOOL_REGISTRY_ENV: &str = "TOOL_REGISTRY";

/// Environment variable for registry auth token.
pub const REGISTRY_TOKEN_ENV: &str = "TOOL_REGISTRY_TOKEN";

/// Environment variable for credentials encryption key.
pub const CREDENTIALS_SECRET_KEY_ENV: &str = "CREDENTIALS_SECRET_KEY";

/// Default home directory for tool configuration.
pub static DEFAULT_HOME_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    dirs::home_dir()
        .map(|h| h.join(".tool"))
        .unwrap_or_else(|| PathBuf::from(".tool"))
});

/// Default path for tool installations.
pub static DEFAULT_TOOLS_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_HOME_PATH.join("tools"));

/// Default path for persistent data storage.
pub static DEFAULT_DATA_PATH: LazyLock<PathBuf> = LazyLock::new(|| DEFAULT_HOME_PATH.join("data"));

/// Default path for temporary files.
pub static DEFAULT_TMP_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| std::env::temp_dir().join("tool"));

/// Default path for credentials storage.
pub static DEFAULT_CREDENTIALS_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_HOME_PATH.join("credentials"));

/// Default path for secrets storage (encryption keys).
pub static DEFAULT_SECRETS_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_HOME_PATH.join("secrets"));

/// Path to the auto-generated encryption key file.
pub static ENCRYPTION_KEY_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_SECRETS_PATH.join("encryption.key"));

/// Default path for registry authentication.
pub static REGISTRY_AUTH_DIR: LazyLock<PathBuf> = LazyLock::new(|| DEFAULT_HOME_PATH.join("auth"));

/// Default path for tool configuration storage.
pub static DEFAULT_CONFIG_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_HOME_PATH.join("config"));

/// Default path for host config backups.
pub static DEFAULT_BACKUPS_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_HOME_PATH.join("backups"));

/// Default path for host metadata (tracking managed tools).
pub static DEFAULT_HOSTS_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_HOME_PATH.join("hosts"));

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Get the registry URL, checking TOOL_REGISTRY env var first.
pub fn get_registry_url() -> String {
    std::env::var(TOOL_REGISTRY_ENV).unwrap_or_else(|_| DEFAULT_REGISTRY_URL.to_string())
}
