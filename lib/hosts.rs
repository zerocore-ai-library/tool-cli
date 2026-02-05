//! MCP host definitions and configuration utilities.
//!
//! This module provides types and functions for managing MCP host configurations
//! (Claude Desktop, Cursor, Claude Code) with safety features like backups and
//! atomic writes.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::constants::{DEFAULT_BACKUPS_PATH, DEFAULT_HOSTS_PATH};
use crate::error::{ToolError, ToolResult};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Supported MCP host applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpHost {
    ClaudeDesktop,
    Cursor,
    ClaudeCode,
    Vscode,
    Codex,
    Windsurf,
    Zed,
    GeminiCli,
    Kiro,
    RooCode,
    OpenCode,
}

/// Metadata for tracking tool-cli managed entries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostMetadata {
    /// Tool references managed by tool-cli.
    pub managed_tools: Vec<String>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl McpHost {
    /// Parse host name from string (case-insensitive, supports aliases).
    pub fn parse(s: &str) -> ToolResult<Self> {
        match s.to_lowercase().as_str() {
            "claude-desktop" | "claudedesktop" | "cd" => Ok(Self::ClaudeDesktop),
            "cursor" => Ok(Self::Cursor),
            "claude-code" | "claudecode" | "cc" => Ok(Self::ClaudeCode),
            "vscode" | "vs-code" | "vsc" | "code" => Ok(Self::Vscode),
            "codex" => Ok(Self::Codex),
            "windsurf" => Ok(Self::Windsurf),
            "zed" => Ok(Self::Zed),
            "gemini-cli" | "geminicli" | "gemini" => Ok(Self::GeminiCli),
            "kiro" => Ok(Self::Kiro),
            "roo-code" | "roocode" | "roo" => Ok(Self::RooCode),
            "opencode" | "open-code" | "oc" => Ok(Self::OpenCode),
            _ => Err(ToolError::InvalidHost(s.to_string())),
        }
    }

    /// Get human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Cursor => "Cursor",
            Self::ClaudeCode => "Claude Code",
            Self::Vscode => "VS Code",
            Self::Codex => "Codex",
            Self::Windsurf => "Windsurf",
            Self::Zed => "Zed",
            Self::GeminiCli => "Gemini CLI",
            Self::Kiro => "Kiro",
            Self::RooCode => "Roo Code",
            Self::OpenCode => "OpenCode",
        }
    }

    /// Get canonical name for CLI output and file paths.
    pub fn canonical_name(&self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "claude-desktop",
            Self::Cursor => "cursor",
            Self::ClaudeCode => "claude-code",
            Self::Vscode => "vscode",
            Self::Codex => "codex",
            Self::Windsurf => "windsurf",
            Self::Zed => "zed",
            Self::GeminiCli => "gemini-cli",
            Self::Kiro => "kiro",
            Self::RooCode => "roo-code",
            Self::OpenCode => "opencode",
        }
    }

    /// Get the JSON key for server entries in this host's config.
    /// VSCode uses "servers", others use "mcpServers".
    pub fn server_key(&self) -> &'static str {
        match self {
            Self::Vscode => "servers",
            Self::Zed => "context_servers",
            Self::Codex => "mcp_servers",
            Self::OpenCode => "mcp",
            _ => "mcpServers",
        }
    }

    /// Get the config file path for this host (cross-platform).
    pub fn config_path(&self) -> ToolResult<PathBuf> {
        match self {
            Self::ClaudeDesktop => Self::claude_desktop_path(),
            Self::Cursor => Self::cursor_path(),
            Self::ClaudeCode => Self::claude_code_path(),
            Self::Vscode => Self::vscode_path(),
            Self::Codex => Self::codex_path(),
            Self::Windsurf => Self::windsurf_path(),
            Self::Zed => Self::zed_path(),
            Self::GeminiCli => Self::gemini_cli_path(),
            Self::Kiro => Self::kiro_path(),
            Self::RooCode => Self::roo_code_path(),
            Self::OpenCode => Self::opencode_path(),
        }
    }

    fn claude_desktop_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join("Library/Application Support/Claude/claude_desktop_config.json"))
        }
        #[cfg(target_os = "windows")]
        {
            let appdata = std::env::var("APPDATA").map(PathBuf::from).or_else(|_| {
                dirs::config_dir().ok_or_else(|| {
                    ToolError::Generic("Could not determine config directory".to_string())
                })
            })?;
            Ok(appdata.join("Claude").join("claude_desktop_config.json"))
        }
        #[cfg(target_os = "linux")]
        {
            let config = dirs::config_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine config directory".to_string())
            })?;
            Ok(config.join("Claude").join("claude_desktop_config.json"))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Err(ToolError::Generic("Unsupported platform".to_string()))
        }
    }

    fn cursor_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let home = std::env::var("USERPROFILE")
                .map(PathBuf::from)
                .or_else(|_| {
                    dirs::home_dir().ok_or_else(|| {
                        ToolError::Generic("Could not determine home directory".to_string())
                    })
                })?;
            Ok(home.join(".cursor").join("mcp.json"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join(".cursor").join("mcp.json"))
        }
    }

    fn claude_code_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let home = std::env::var("USERPROFILE")
                .map(PathBuf::from)
                .or_else(|_| {
                    dirs::home_dir().ok_or_else(|| {
                        ToolError::Generic("Could not determine home directory".to_string())
                    })
                })?;
            Ok(home.join(".claude.json"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join(".claude.json"))
        }
    }

    fn vscode_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join("Library/Application Support/Code/User/mcp.json"))
        }
        #[cfg(target_os = "windows")]
        {
            let appdata = std::env::var("APPDATA").map(PathBuf::from).or_else(|_| {
                dirs::config_dir().ok_or_else(|| {
                    ToolError::Generic("Could not determine config directory".to_string())
                })
            })?;
            Ok(appdata.join("Code").join("User").join("mcp.json"))
        }
        #[cfg(target_os = "linux")]
        {
            let config = dirs::config_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine config directory".to_string())
            })?;
            Ok(config.join("Code").join("User").join("mcp.json"))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Err(ToolError::Generic("Unsupported platform".to_string()))
        }
    }

    fn codex_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            Err(ToolError::Generic(
                "Codex is not supported on Windows".to_string(),
            ))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join(".codex").join("config.toml"))
        }
    }

    fn windsurf_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let home = std::env::var("USERPROFILE")
                .map(PathBuf::from)
                .or_else(|_| {
                    dirs::home_dir().ok_or_else(|| {
                        ToolError::Generic("Could not determine home directory".to_string())
                    })
                })?;
            Ok(home
                .join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home
                .join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"))
        }
    }

    fn zed_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join("Library/Application Support/Zed/settings.json"))
        }
        #[cfg(target_os = "linux")]
        {
            let config = dirs::config_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine config directory".to_string())
            })?;
            Ok(config.join("zed").join("settings.json"))
        }
        #[cfg(target_os = "windows")]
        {
            Err(ToolError::Generic(
                "Zed is not supported on Windows".to_string(),
            ))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(ToolError::Generic("Unsupported platform".to_string()))
        }
    }

    fn gemini_cli_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let home = std::env::var("USERPROFILE")
                .map(PathBuf::from)
                .or_else(|_| {
                    dirs::home_dir().ok_or_else(|| {
                        ToolError::Generic("Could not determine home directory".to_string())
                    })
                })?;
            Ok(home.join(".gemini").join("settings.json"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join(".gemini").join("settings.json"))
        }
    }

    fn kiro_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let home = std::env::var("USERPROFILE")
                .map(PathBuf::from)
                .or_else(|_| {
                    dirs::home_dir().ok_or_else(|| {
                        ToolError::Generic("Could not determine home directory".to_string())
                    })
                })?;
            Ok(home.join(".kiro").join("settings").join("mcp.json"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home.join(".kiro").join("settings").join("mcp.json"))
        }
    }

    fn roo_code_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            let home = dirs::home_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine home directory".to_string())
            })?;
            Ok(home
                .join("Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings")
                .join("mcp_settings.json"))
        }
        #[cfg(target_os = "windows")]
        {
            let appdata = std::env::var("APPDATA").map(PathBuf::from).or_else(|_| {
                dirs::config_dir().ok_or_else(|| {
                    ToolError::Generic("Could not determine config directory".to_string())
                })
            })?;
            Ok(appdata
                .join("Code")
                .join("User")
                .join("globalStorage")
                .join("rooveterinaryinc.roo-cline")
                .join("settings")
                .join("mcp_settings.json"))
        }
        #[cfg(target_os = "linux")]
        {
            let config = dirs::config_dir().ok_or_else(|| {
                ToolError::Generic("Could not determine config directory".to_string())
            })?;
            Ok(config
                .join("Code")
                .join("User")
                .join("globalStorage")
                .join("rooveterinaryinc.roo-cline")
                .join("settings")
                .join("mcp_settings.json"))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            Err(ToolError::Generic("Unsupported platform".to_string()))
        }
    }

    fn opencode_path() -> ToolResult<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            let appdata = std::env::var("APPDATA").map(PathBuf::from).or_else(|_| {
                dirs::config_dir().ok_or_else(|| {
                    ToolError::Generic("Could not determine config directory".to_string())
                })
            })?;
            Ok(appdata.join("opencode").join("opencode.json"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            // OpenCode uses XDG basedir (xdg-basedir npm package), which resolves
            // to $XDG_CONFIG_HOME or ~/.config on all Unix platforms including macOS.
            let xdg_config = std::env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    dirs::home_dir()
                        .unwrap_or_else(|| PathBuf::from("~"))
                        .join(".config")
                });
            Ok(xdg_config.join("opencode").join("opencode.json"))
        }
    }

    /// Get all supported hosts.
    pub fn all() -> &'static [McpHost] {
        &[
            Self::ClaudeDesktop,
            Self::Cursor,
            Self::ClaudeCode,
            Self::Vscode,
            Self::Codex,
            Self::Windsurf,
            Self::Zed,
            Self::GeminiCli,
            Self::Kiro,
            Self::RooCode,
            Self::OpenCode,
        ]
    }

    /// Check if the config file exists.
    pub fn config_exists(&self) -> bool {
        self.config_path().map(|p| p.exists()).unwrap_or(false)
    }
}

//--------------------------------------------------------------------------------------------------
// Functions: Config Operations
//--------------------------------------------------------------------------------------------------

/// Load host config file as JSON. Returns empty object if file doesn't exist.
/// For TOML hosts (Codex), the TOML is converted to a JSON Value.
pub fn load_config(host: &McpHost) -> ToolResult<Value> {
    let path = host.config_path()?;

    if !path.exists() {
        return Ok(serde_json::json!({}));
    }

    let content = fs::read_to_string(&path).map_err(|e| ToolError::HostConfigParseError {
        host: host.display_name().to_string(),
        message: format!("Failed to read config: {}", e),
    })?;

    // Handle empty files
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }

    if is_toml_host(host) {
        // Parse TOML into a serde_json::Value via toml::Value
        let toml_val: toml::Value =
            toml::from_str(&content).map_err(|e| ToolError::HostConfigParseError {
                host: host.display_name().to_string(),
                message: format!("Invalid TOML: {}", e),
            })?;
        serde_json::to_value(toml_val).map_err(|e| ToolError::HostConfigParseError {
            host: host.display_name().to_string(),
            message: format!("TOML to JSON conversion failed: {}", e),
        })
    } else {
        serde_json::from_str(&content).map_err(|e| ToolError::HostConfigParseError {
            host: host.display_name().to_string(),
            message: format!("Invalid JSON: {}", e),
        })
    }
}

/// Save host config file with atomic write (temp file + rename).
/// For TOML hosts (Codex), uses toml_edit for non-destructive editing.
pub fn save_config(host: &McpHost, config: &Value) -> ToolResult<()> {
    let config_path = host.config_path()?;

    if is_toml_host(host) {
        return save_toml_config(host, config, &config_path);
    }

    // Serialize to pretty JSON
    let content =
        serde_json::to_string_pretty(config).map_err(|e| ToolError::HostConfigParseError {
            host: host.display_name().to_string(),
            message: format!("Failed to serialize config: {}", e),
        })?;

    // Create parent directories if needed
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write to temp file first
    let temp_path = config_path.with_extension("json.tmp");

    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    // Validate temp file is readable JSON
    let verify_content = fs::read_to_string(&temp_path)?;
    let _: Value = serde_json::from_str(&verify_content).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        ToolError::HostConfigParseError {
            host: host.display_name().to_string(),
            message: format!("Verification failed: {}", e),
        }
    })?;

    // Atomic rename
    fs::rename(&temp_path, &config_path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        ToolError::Io(e)
    })?;

    Ok(())
}

/// Save TOML config using toml_edit for non-destructive editing.
/// Reads the existing file, applies changes from the JSON Value, and writes back.
fn save_toml_config(host: &McpHost, config: &Value, config_path: &Path) -> ToolResult<()> {
    use toml_edit::DocumentMut;

    // Load existing document or create new one
    let mut doc: DocumentMut = if config_path.exists() {
        let content =
            fs::read_to_string(config_path).map_err(|e| ToolError::HostConfigParseError {
                host: host.display_name().to_string(),
                message: format!("Failed to read config: {}", e),
            })?;
        content
            .parse()
            .map_err(|e| ToolError::HostConfigParseError {
                host: host.display_name().to_string(),
                message: format!("Failed to parse TOML: {}", e),
            })?
    } else {
        DocumentMut::new()
    };

    // Update mcp_servers section from config JSON
    let server_key = host.server_key();
    if let Some(servers) = config.get(server_key).and_then(|v| v.as_object()) {
        // Ensure [mcp_servers] table exists
        if doc.get(server_key).is_none() {
            doc[server_key] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let mcp_table =
            doc[server_key]
                .as_table_mut()
                .ok_or_else(|| ToolError::HostConfigParseError {
                    host: host.display_name().to_string(),
                    message: format!("{} is not a table", server_key),
                })?;

        // Remove entries not in config (handles removals)
        let existing_keys: Vec<String> = mcp_table.iter().map(|(k, _)| k.to_string()).collect();
        for key in &existing_keys {
            if !servers.contains_key(key) {
                mcp_table.remove(key);
            }
        }

        // Add/update entries
        for (name, value) in servers {
            let mut server_table = toml_edit::Table::new();
            if let Some(obj) = value.as_object() {
                if let Some(cmd) = obj.get("command").and_then(|v| v.as_str()) {
                    server_table["command"] = toml_edit::value(cmd);
                }
                if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
                    let mut arr = toml_edit::Array::new();
                    for arg in args {
                        if let Some(s) = arg.as_str() {
                            arr.push(s);
                        }
                    }
                    server_table["args"] = toml_edit::value(arr);
                }
                if let Some(enabled) = obj.get("enabled").and_then(|v| v.as_bool()) {
                    server_table["enabled"] = toml_edit::value(enabled);
                }
            }
            mcp_table[name] = toml_edit::Item::Table(server_table);
        }
    } else {
        // No servers — remove the section entirely
        doc.remove(server_key);
    }

    // Create parent directories if needed
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = doc.to_string();
    let temp_path = config_path.with_extension("toml.tmp");

    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    // Validate temp file is parseable TOML
    let verify_content = fs::read_to_string(&temp_path)?;
    let _: DocumentMut = verify_content.parse().map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        ToolError::HostConfigParseError {
            host: host.display_name().to_string(),
            message: format!("TOML verification failed: {}", e),
        }
    })?;

    // Atomic rename
    fs::rename(&temp_path, config_path).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        ToolError::Io(e)
    })?;

    Ok(())
}

/// Generate a Codex TOML server entry as a JSON Value.
/// Codex uses `enabled = true` by default.
pub fn generate_codex_server_entry(tool_ref: &str) -> Value {
    serde_json::json!({
        "command": "tool",
        "args": ["run", "--expose", "stdio", tool_ref, "--yes"],
        "enabled": true,
    })
}

//--------------------------------------------------------------------------------------------------
// Functions: Backup Operations
//--------------------------------------------------------------------------------------------------

/// Create a timestamped backup of the config file before modification.
/// Returns the backup path if a backup was created.
pub fn create_backup(host: &McpHost) -> ToolResult<Option<PathBuf>> {
    let config_path = host.config_path()?;

    if !config_path.exists() {
        return Ok(None);
    }

    let backup_dir = DEFAULT_BACKUPS_PATH.join(host.canonical_name());
    fs::create_dir_all(&backup_dir)?;

    let timestamp = Local::now().format("%Y-%m-%dT%H-%M-%S");
    let backup_path = backup_dir.join(format!("{}.json", timestamp));

    fs::copy(&config_path, &backup_path)?;

    // Prune old backups (keep last 5)
    prune_old_backups(&backup_dir, 5)?;

    Ok(Some(backup_path))
}

/// Remove old backups, keeping only the most recent N.
fn prune_old_backups(backup_dir: &Path, keep: usize) -> ToolResult<()> {
    let mut backups: Vec<_> = fs::read_dir(backup_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        })
        .collect();

    // Sort by modification time, newest first
    backups.sort_by(|a, b| {
        b.metadata()
            .and_then(|m| m.modified())
            .ok()
            .cmp(&a.metadata().and_then(|m| m.modified()).ok())
    });

    // Remove excess backups
    for entry in backups.into_iter().skip(keep) {
        let _ = fs::remove_file(entry.path());
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: Metadata Operations
//--------------------------------------------------------------------------------------------------

/// Get metadata file path for a host.
fn metadata_path(host: &McpHost) -> PathBuf {
    DEFAULT_HOSTS_PATH.join(format!("{}.json", host.canonical_name()))
}

/// Load metadata tracking which tools are managed by tool-cli.
pub fn load_metadata(host: &McpHost) -> ToolResult<HostMetadata> {
    let path = metadata_path(host);

    if !path.exists() {
        return Ok(HostMetadata::default());
    }

    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

/// Save host metadata.
pub fn save_metadata(host: &McpHost, metadata: &HostMetadata) -> ToolResult<()> {
    let path = metadata_path(host);
    fs::create_dir_all(&*DEFAULT_HOSTS_PATH)?;
    let content = serde_json::to_string_pretty(metadata)?;
    fs::write(&path, content)?;
    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: Server Entry Generation
//--------------------------------------------------------------------------------------------------

/// Convert tool ref to server name.
/// e.g., "appcypher/filesystem" -> "appcypher__filesystem"
pub fn tool_ref_to_server_name(tool_ref: &str) -> String {
    tool_ref.replace(['/', '@'], "__")
}

/// Generate MCP server entry for a tool as a JSON value.
/// Uses `--expose stdio` to bridge any transport to stdio.
/// Uses `--yes` to skip interactive prompts during automated startup.
/// VSCode requires an explicit "type" field.
/// Zed uses a different schema with `command.path` and `command.args`.
pub fn generate_server_entry(tool_ref: &str, host: &McpHost) -> Value {
    let args = vec![
        "run".to_string(),
        "--expose".to_string(),
        "stdio".to_string(),
        tool_ref.to_string(),
        "--yes".to_string(),
    ];

    match host {
        McpHost::Zed => {
            serde_json::json!({
                "command": {
                    "path": "tool",
                    "args": args,
                }
            })
        }
        McpHost::Vscode => {
            serde_json::json!({
                "type": "stdio",
                "command": "tool",
                "args": args,
            })
        }
        McpHost::OpenCode => {
            let mut command = vec!["tool".to_string()];
            command.extend(args);
            serde_json::json!({
                "type": "local",
                "command": command,
            })
        }
        _ => {
            serde_json::json!({
                "command": "tool",
                "args": args,
            })
        }
    }
}

/// Check if the host uses TOML config format.
pub fn is_toml_host(host: &McpHost) -> bool {
    matches!(host, McpHost::Codex)
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_parse() {
        assert!(matches!(
            McpHost::parse("claude-desktop"),
            Ok(McpHost::ClaudeDesktop)
        ));
        assert!(matches!(McpHost::parse("cd"), Ok(McpHost::ClaudeDesktop)));
        assert!(matches!(McpHost::parse("cursor"), Ok(McpHost::Cursor)));
        assert!(matches!(
            McpHost::parse("claude-code"),
            Ok(McpHost::ClaudeCode)
        ));
        assert!(matches!(McpHost::parse("cc"), Ok(McpHost::ClaudeCode)));
        assert!(matches!(McpHost::parse("vscode"), Ok(McpHost::Vscode)));
        assert!(matches!(McpHost::parse("vs-code"), Ok(McpHost::Vscode)));
        assert!(matches!(McpHost::parse("vsc"), Ok(McpHost::Vscode)));
        assert!(matches!(McpHost::parse("code"), Ok(McpHost::Vscode)));
        assert!(matches!(McpHost::parse("codex"), Ok(McpHost::Codex)));
        assert!(matches!(McpHost::parse("windsurf"), Ok(McpHost::Windsurf)));
        assert!(matches!(McpHost::parse("zed"), Ok(McpHost::Zed)));
        assert!(matches!(
            McpHost::parse("gemini-cli"),
            Ok(McpHost::GeminiCli)
        ));
        assert!(matches!(
            McpHost::parse("geminicli"),
            Ok(McpHost::GeminiCli)
        ));
        assert!(matches!(McpHost::parse("gemini"), Ok(McpHost::GeminiCli)));
        assert!(matches!(McpHost::parse("kiro"), Ok(McpHost::Kiro)));
        assert!(matches!(McpHost::parse("roo-code"), Ok(McpHost::RooCode)));
        assert!(matches!(McpHost::parse("roocode"), Ok(McpHost::RooCode)));
        assert!(matches!(McpHost::parse("roo"), Ok(McpHost::RooCode)));
        assert!(matches!(McpHost::parse("opencode"), Ok(McpHost::OpenCode)));
        assert!(matches!(McpHost::parse("open-code"), Ok(McpHost::OpenCode)));
        assert!(matches!(McpHost::parse("oc"), Ok(McpHost::OpenCode)));
        assert!(McpHost::parse("invalid").is_err());
    }

    #[test]
    fn test_tool_ref_to_server_name() {
        assert_eq!(tool_ref_to_server_name("bash"), "bash");
        assert_eq!(
            tool_ref_to_server_name("appcypher/filesystem"),
            "appcypher__filesystem"
        );
        assert_eq!(tool_ref_to_server_name("ns/tool@1.0.0"), "ns__tool__1.0.0");
    }

    #[test]
    fn test_generate_server_entry() {
        // Standard hosts: command + args, no type
        let entry = generate_server_entry("appcypher/filesystem", &McpHost::ClaudeDesktop);
        assert_eq!(entry["command"], "tool");
        assert_eq!(entry["args"][0], "run");
        assert!(entry.get("type").is_none());

        // VSCode requires type field
        let entry = generate_server_entry("appcypher/filesystem", &McpHost::Vscode);
        assert_eq!(entry["type"], "stdio");
        assert_eq!(entry["command"], "tool");

        // Zed uses command.path + command.args
        let entry = generate_server_entry("appcypher/filesystem", &McpHost::Zed);
        assert_eq!(entry["command"]["path"], "tool");
        assert_eq!(entry["command"]["args"][0], "run");

        // OpenCode uses type "local" and command as array
        let entry = generate_server_entry("appcypher/filesystem", &McpHost::OpenCode);
        assert_eq!(entry["type"], "local");
        assert_eq!(entry["command"][0], "tool");
        assert_eq!(entry["command"][1], "run");
        assert!(entry.get("args").is_none());

        // Codex entry has enabled field
        let entry = generate_codex_server_entry("appcypher/filesystem");
        assert_eq!(entry["command"], "tool");
        assert_eq!(entry["enabled"], true);
    }

    #[test]
    fn test_host_canonical_names() {
        assert_eq!(McpHost::ClaudeDesktop.canonical_name(), "claude-desktop");
        assert_eq!(McpHost::Cursor.canonical_name(), "cursor");
        assert_eq!(McpHost::ClaudeCode.canonical_name(), "claude-code");
        assert_eq!(McpHost::Vscode.canonical_name(), "vscode");
        assert_eq!(McpHost::Codex.canonical_name(), "codex");
        assert_eq!(McpHost::Windsurf.canonical_name(), "windsurf");
        assert_eq!(McpHost::Zed.canonical_name(), "zed");
        assert_eq!(McpHost::GeminiCli.canonical_name(), "gemini-cli");
        assert_eq!(McpHost::Kiro.canonical_name(), "kiro");
        assert_eq!(McpHost::RooCode.canonical_name(), "roo-code");
        assert_eq!(McpHost::OpenCode.canonical_name(), "opencode");
    }

    #[test]
    fn test_host_display_names() {
        assert_eq!(McpHost::ClaudeDesktop.display_name(), "Claude Desktop");
        assert_eq!(McpHost::Cursor.display_name(), "Cursor");
        assert_eq!(McpHost::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(McpHost::Vscode.display_name(), "VS Code");
        assert_eq!(McpHost::Codex.display_name(), "Codex");
        assert_eq!(McpHost::Windsurf.display_name(), "Windsurf");
        assert_eq!(McpHost::Zed.display_name(), "Zed");
        assert_eq!(McpHost::GeminiCli.display_name(), "Gemini CLI");
        assert_eq!(McpHost::Kiro.display_name(), "Kiro");
        assert_eq!(McpHost::RooCode.display_name(), "Roo Code");
        assert_eq!(McpHost::OpenCode.display_name(), "OpenCode");
    }

    #[test]
    fn test_host_server_key() {
        assert_eq!(McpHost::ClaudeDesktop.server_key(), "mcpServers");
        assert_eq!(McpHost::Cursor.server_key(), "mcpServers");
        assert_eq!(McpHost::ClaudeCode.server_key(), "mcpServers");
        assert_eq!(McpHost::Vscode.server_key(), "servers");
        assert_eq!(McpHost::Codex.server_key(), "mcp_servers");
        assert_eq!(McpHost::Zed.server_key(), "context_servers");
        assert_eq!(McpHost::Windsurf.server_key(), "mcpServers");
        assert_eq!(McpHost::GeminiCli.server_key(), "mcpServers");
        assert_eq!(McpHost::Kiro.server_key(), "mcpServers");
        assert_eq!(McpHost::RooCode.server_key(), "mcpServers");
        assert_eq!(McpHost::OpenCode.server_key(), "mcp");
    }
}
