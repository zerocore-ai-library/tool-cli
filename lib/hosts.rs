//! MCP host definitions and configuration utilities.
//!
//! This module provides types and functions for managing MCP host configurations
//! (Claude Desktop, Cursor, Claude Code) with safety features like backups and
//! atomic writes.

use std::collections::BTreeMap;
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
}

/// Entry in the mcpServers/servers config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    /// Server type (required for VSCode: "stdio" or "http").
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub server_type: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
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
        }
    }

    /// Get canonical name for CLI output and file paths.
    pub fn canonical_name(&self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "claude-desktop",
            Self::Cursor => "cursor",
            Self::ClaudeCode => "claude-code",
            Self::Vscode => "vscode",
        }
    }

    /// Get the JSON key for server entries in this host's config.
    /// VSCode uses "servers", others use "mcpServers".
    pub fn server_key(&self) -> &'static str {
        match self {
            Self::Vscode => "servers",
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

    /// Get all supported hosts.
    pub fn all() -> &'static [McpHost] {
        &[
            Self::ClaudeDesktop,
            Self::Cursor,
            Self::ClaudeCode,
            Self::Vscode,
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

    serde_json::from_str(&content).map_err(|e| ToolError::HostConfigParseError {
        host: host.display_name().to_string(),
        message: format!("Invalid JSON: {}", e),
    })
}

/// Save host config file with atomic write (temp file + rename).
pub fn save_config(host: &McpHost, config: &Value) -> ToolResult<()> {
    let config_path = host.config_path()?;

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

/// Generate MCP server entry for a tool.
/// Uses `--expose stdio` to bridge any transport to stdio.
/// Uses `--yes` to skip interactive prompts during automated startup.
/// VSCode requires an explicit "type" field.
pub fn generate_server_entry(tool_ref: &str, host: &McpHost) -> McpServerEntry {
    McpServerEntry {
        server_type: match host {
            McpHost::Vscode => Some("stdio".to_string()),
            _ => None,
        },
        command: "tool".to_string(),
        args: vec![
            "run".to_string(),
            "--expose".to_string(),
            "stdio".to_string(),
            tool_ref.to_string(),
            "--yes".to_string(),
        ],
        env: None,
    }
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
        // Non-VSCode hosts don't have type field
        let entry = generate_server_entry("appcypher/filesystem", &McpHost::ClaudeDesktop);
        assert_eq!(entry.command, "tool");
        assert_eq!(
            entry.args,
            vec!["run", "--expose", "stdio", "appcypher/filesystem", "--yes"]
        );
        assert!(entry.server_type.is_none());
        assert!(entry.env.is_none());

        // VSCode requires type field
        let entry = generate_server_entry("appcypher/filesystem", &McpHost::Vscode);
        assert_eq!(entry.server_type, Some("stdio".to_string()));
    }

    #[test]
    fn test_host_canonical_names() {
        assert_eq!(McpHost::ClaudeDesktop.canonical_name(), "claude-desktop");
        assert_eq!(McpHost::Cursor.canonical_name(), "cursor");
        assert_eq!(McpHost::ClaudeCode.canonical_name(), "claude-code");
        assert_eq!(McpHost::Vscode.canonical_name(), "vscode");
    }

    #[test]
    fn test_host_display_names() {
        assert_eq!(McpHost::ClaudeDesktop.display_name(), "Claude Desktop");
        assert_eq!(McpHost::Cursor.display_name(), "Cursor");
        assert_eq!(McpHost::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(McpHost::Vscode.display_name(), "VS Code");
    }

    #[test]
    fn test_host_server_key() {
        assert_eq!(McpHost::ClaudeDesktop.server_key(), "mcpServers");
        assert_eq!(McpHost::Cursor.server_key(), "mcpServers");
        assert_eq!(McpHost::ClaudeCode.server_key(), "mcpServers");
        assert_eq!(McpHost::Vscode.server_key(), "servers");
    }
}
