//! Tool config command handlers.

use crate::commands::ConfigCommand;
use crate::constants::DEFAULT_CONFIG_PATH;
use crate::error::{ToolError, ToolResult};
use crate::mcp::connect_with_oauth;
use crate::mcpb::{McpbTransport, McpbUserConfigField, McpbUserConfigType};
use crate::prompt::init_theme;
use crate::references::PluginRef;
use crate::resolver::load_tool_from_path;
use crate::security::get_credential_crypto;
use crate::system_config::allocate_system_config;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use super::list::resolve_tool_path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Encrypted config envelope stored on disk.
///
/// When a config contains sensitive fields, the entire config is encrypted
/// and stored in this envelope format.
#[derive(Debug, Serialize, Deserialize)]
struct EncryptedConfigEnvelope {
    /// Marker to identify encrypted configs.
    encrypted: bool,
    /// AES-GCM nonce (12 bytes, base64 encoded).
    nonce: String,
    /// AES-GCM authentication tag (16 bytes, base64 encoded).
    auth_tag: String,
    /// Encrypted config JSON (base64 encoded).
    ciphertext: String,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Main entry point for config command.
pub async fn config_tool(cmd: ConfigCommand, concise: bool, no_header: bool) -> ToolResult<()> {
    match cmd {
        ConfigCommand::Set {
            tool,
            values,
            yes,
            config,
        } => config_set(tool, values, yes, config, concise).await,
        ConfigCommand::Get { tool, key } => config_get(tool, key, concise, no_header).await,
        ConfigCommand::List => config_list(concise, no_header).await,
        ConfigCommand::Unset { tool, key } => config_unset(tool, key, concise).await,
        ConfigCommand::Reset { tool } => config_reset(tool, concise).await,
    }
}

/// Handle `config set` subcommand.
async fn config_set(
    tool: String,
    values: Vec<String>,
    yes: bool,
    config_flags: Vec<String>,
    concise: bool,
) -> ToolResult<()> {
    // Resolve tool and load manifest
    let resolved_path = resolve_tool_path(&tool).await?;
    let resolved_plugin = load_tool_from_path(&resolved_path.path)?;

    // Parse the original tool reference for storage (strip version for config path)
    let plugin_ref =
        parse_tool_ref_for_config(&tool, &resolved_plugin, resolved_path.is_installed)?;

    // Clone the schema since we need resolved_plugin later for OAuth
    let schema = resolved_plugin.template.user_config.clone();

    // Check if tool is an HTTP tool with OAuth (can be configured even without user_config)
    let is_http_with_oauth = resolved_plugin.template.server.transport == McpbTransport::Http
        && resolved_plugin
            .template
            .server
            .mcp_config
            .as_ref()
            .map(|c| c.oauth_config.is_some())
            .unwrap_or(false);

    // Check if tool has configurable options (user_config or OAuth)
    let has_user_config = schema.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

    if !has_user_config && !is_http_with_oauth {
        return Err(ToolError::Generic(format!(
            "Tool '{}' has no configurable options",
            plugin_ref
        )));
    }

    // Use empty schema if none defined
    let schema = schema.unwrap_or_default();

    // Parse provided values from both trailing args and -C flags
    let mut provided_config = parse_config_values(&values)?;
    let flag_config = parse_config_values(&config_flags)?;
    provided_config.extend(flag_config);

    // Validate provided keys against schema (warn for unknown keys)
    for key in provided_config.keys() {
        if !schema.contains_key(key) {
            eprintln!(
                "  {} Unknown config key '{}' (not in tool schema)",
                "warning".bright_yellow(),
                key
            );
        }
    }

    // Validate provided values
    for (key, value) in &provided_config {
        if let Some(field) = schema.get(key) {
            validate_field_value(key, value, field)?;
        }
    }

    // Load existing config
    let existing_config = load_tool_config(&plugin_ref).unwrap_or_default();

    // Determine final config
    let final_config = if yes {
        // Non-interactive: only use provided values
        if provided_config.is_empty() {
            return Err(ToolError::Generic(
                "No configuration values provided. Use key=value arguments or -C flags.".into(),
            ));
        }
        // Merge with existing (provided values override)
        let mut config = existing_config;
        config.extend(provided_config);
        config
    } else if !std::io::stdin().is_terminal() {
        // Non-TTY without -y
        if provided_config.is_empty() {
            return Err(ToolError::Generic(
                "Non-interactive mode requires -y flag with values, or run in a terminal.".into(),
            ));
        }
        // Merge with existing
        let mut config = existing_config;
        config.extend(provided_config);
        config
    } else {
        // Interactive: merge provided with existing, then prompt for remaining
        let mut config = existing_config.clone();
        config.extend(provided_config.clone());

        // Prompt for fields not yet provided
        let prompted = prompt_all_user_config(&schema, &config)?;
        config.extend(prompted);
        config
    };

    // Save config (with encryption if schema has sensitive fields)
    save_tool_config_with_schema(&plugin_ref, &final_config, Some(&schema))?;

    // For HTTP tools, attempt connection to trigger OAuth flow if needed
    let oauth_result =
        try_oauth_for_http_tool(&resolved_plugin, &final_config, &plugin_ref.to_string()).await;

    // Output
    if concise {
        println!("ok");
    } else {
        println!(
            "\n  {} Configuration saved for {}\n",
            "✓".bright_green(),
            plugin_ref.to_string().bold()
        );

        // Show saved values
        for (key, value) in &final_config {
            let display_value = if schema
                .get(key)
                .map(|f| f.sensitive.unwrap_or(false))
                .unwrap_or(false)
            {
                mask_sensitive(value)
            } else {
                value.clone()
            };
            println!("    {:<20} {}", key, display_value.dimmed());
        }

        // Show OAuth status
        match oauth_result {
            OAuthSetupResult::NotRequired => {}
            OAuthSetupResult::AlreadyAuthenticated => {
                println!("    {:<20} {}", "OAuth", "authenticated".bright_green());
            }
            OAuthSetupResult::Authenticated => {
                println!("    {:<20} {}", "OAuth", "authenticated".bright_green());
            }
            OAuthSetupResult::Skipped(reason) => {
                println!("    {:<20} {}", "OAuth", reason.dimmed());
            }
            OAuthSetupResult::Failed(err) => {
                println!(
                    "    {:<20} {} ({})",
                    "OAuth",
                    "failed".bright_red(),
                    err.dimmed()
                );
            }
        }
        println!();
    }

    Ok(())
}

/// Result of attempting OAuth setup for HTTP tools.
enum OAuthSetupResult {
    /// Tool doesn't use HTTP transport, no OAuth needed.
    NotRequired,
    /// Already had valid credentials.
    AlreadyAuthenticated,
    /// Successfully authenticated via OAuth flow.
    Authenticated,
    /// OAuth was skipped (e.g., couldn't initialize credential storage).
    Skipped(String),
    /// OAuth failed with an error.
    Failed(String),
}

/// Try to set up OAuth for HTTP tools.
///
/// This attempts a connection to trigger the OAuth flow if the tool uses HTTP
/// transport and requires authentication.
async fn try_oauth_for_http_tool(
    resolved_plugin: &crate::resolver::ResolvedPlugin<crate::mcpb::McpbManifest>,
    user_config: &BTreeMap<String, String>,
    tool_ref: &str,
) -> OAuthSetupResult {
    // Check if tool uses HTTP transport
    if resolved_plugin.template.server.transport != McpbTransport::Http {
        return OAuthSetupResult::NotRequired;
    }

    // Check if credential storage is available (should auto-initialize)
    if crate::security::get_credential_crypto().is_none() {
        return OAuthSetupResult::Skipped("Could not initialize credential storage".to_string());
    }

    // Allocate system config and resolve manifest
    let system_config =
        match allocate_system_config(resolved_plugin.template.system_config.as_ref()) {
            Ok(config) => config,
            Err(e) => {
                return OAuthSetupResult::Failed(format!(
                    "Failed to allocate system config: {}",
                    e
                ));
            }
        };

    let resolved = match resolved_plugin
        .template
        .resolve(user_config, &system_config)
    {
        Ok(r) => r,
        Err(e) => return OAuthSetupResult::Failed(format!("Failed to resolve manifest: {}", e)),
    };

    // Check if we already have credentials
    if let Ok(Some(_)) = crate::oauth::load_credentials(tool_ref).await {
        return OAuthSetupResult::AlreadyAuthenticated;
    }

    // Attempt connection (this will trigger OAuth if needed)
    match connect_with_oauth(&resolved, tool_ref, false).await {
        Ok(_conn) => OAuthSetupResult::Authenticated,
        Err(ToolError::OAuthNotConfigured) => {
            OAuthSetupResult::Skipped("OAuth not configured".to_string())
        }
        Err(ToolError::AuthRequired { .. }) => {
            OAuthSetupResult::Skipped("OAuth required but not configured".to_string())
        }
        Err(e) => {
            // Connection might fail for reasons other than auth (e.g., server not running)
            // This is not necessarily an error for config set
            OAuthSetupResult::Skipped(format!("Could not connect: {}", e))
        }
    }
}

/// Handle `config get` subcommand.
async fn config_get(
    tool: String,
    key: Option<String>,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    // Resolve tool and load manifest
    let resolved_path = resolve_tool_path(&tool).await?;
    let resolved = load_tool_from_path(&resolved_path.path)?;

    // Parse the original tool reference for storage
    let plugin_ref = parse_tool_ref_for_config(&tool, &resolved, resolved_path.is_installed)?;

    let schema = resolved.template.user_config;

    // Load config
    let config = load_tool_config(&plugin_ref)?;

    if config.is_empty() {
        if concise {
            // Empty output for concise mode
            return Ok(());
        }
        println!("\n  No configuration saved for {}\n", plugin_ref);
        if let Some(schema) = schema {
            println!("    Available config fields:");
            for (key, field) in &schema {
                let title = &field.title;
                let req = if field.required.unwrap_or(false) {
                    " (required)"
                } else {
                    ""
                };
                println!("      {:<20} {}{}", key, title.dimmed(), req.dimmed());
            }
            println!();
        }
        return Ok(());
    }

    // Get specific key
    if let Some(key) = key {
        let value = config.get(&key).ok_or_else(|| {
            ToolError::Generic(format!("Config key '{}' not set for {}", key, plugin_ref))
        })?;

        if concise {
            println!("{}", value);
        } else {
            println!("\n  {}.{} = {}\n", plugin_ref, key, value);
        }
        return Ok(());
    }

    // Show all config
    if concise {
        if !no_header {
            println!("#key\tvalue\tsensitive");
        }
        for (key, value) in &config {
            let sensitive = schema
                .as_ref()
                .and_then(|s| s.get(key))
                .map(|f| f.sensitive.unwrap_or(false))
                .unwrap_or(false);
            let display_value = if sensitive {
                mask_sensitive(value)
            } else {
                value.clone()
            };
            println!("{}\t{}\t{}", key, display_value, sensitive);
        }
    } else {
        let config_path = get_config_path(&plugin_ref);
        println!("\n  Tool: {}\n", plugin_ref.to_string().bold());

        for (key, value) in &config {
            let sensitive = schema
                .as_ref()
                .and_then(|s| s.get(key))
                .map(|f| f.sensitive.unwrap_or(false))
                .unwrap_or(false);
            let display_value = if sensitive {
                format!("{}  {}", mask_sensitive(value), "(sensitive)".dimmed())
            } else {
                value.clone()
            };
            println!("    {:<20} {}", key, display_value);
        }

        println!(
            "\n    {}: {}\n",
            "Path".dimmed(),
            config_path.display().to_string().dimmed()
        );
    }

    Ok(())
}

/// Handle `config list` subcommand.
async fn config_list(concise: bool, no_header: bool) -> ToolResult<()> {
    let tools = list_configured_tools()?;

    if tools.is_empty() {
        if concise {
            return Ok(());
        }
        println!("\n  No tools have saved configuration.\n");
        println!(
            "    Use {} to configure a tool.\n",
            "tool config set <tool>".bright_cyan()
        );
        return Ok(());
    }

    if concise {
        if !no_header {
            println!("#tool\tkeys\tpath");
        }
        for (name, path, count) in &tools {
            println!("{}\t{}\t{}", name, count, path.display());
        }
    } else {
        println!("\n  Configured tools:\n");
        for (name, path, count) in &tools {
            let key_word = if *count == 1 { "key" } else { "keys" };
            println!(
                "    {:<30} {} {}    {}",
                name.bold(),
                count,
                key_word.dimmed(),
                path.display().to_string().dimmed()
            );
        }
        println!();
    }

    Ok(())
}

/// Handle `config unset` subcommand.
async fn config_unset(tool: String, key: String, concise: bool) -> ToolResult<()> {
    // Resolve tool
    let resolved_path = resolve_tool_path(&tool).await?;
    let resolved = load_tool_from_path(&resolved_path.path)?;

    // Parse the original tool reference for storage
    let plugin_ref = parse_tool_ref_for_config(&tool, &resolved, resolved_path.is_installed)?;

    // Get schema for encryption decision
    let schema = resolved.template.user_config.as_ref();

    // Load existing config
    let mut config = load_tool_config(&plugin_ref).unwrap_or_default();

    if !config.contains_key(&key) {
        if concise {
            println!("ok");
        } else {
            println!(
                "\n  {} Key '{}' was not set for {}\n",
                "!".bright_yellow(),
                key,
                plugin_ref
            );
        }
        return Ok(());
    }

    // Remove key
    config.remove(&key);

    // Save or delete config file
    if config.is_empty() {
        delete_tool_config(&plugin_ref)?;
    } else {
        save_tool_config_with_schema(&plugin_ref, &config, schema)?;
    }

    if concise {
        println!("ok");
    } else {
        println!(
            "\n  {} Removed '{}' from {}\n",
            "✓".bright_green(),
            key,
            plugin_ref
        );
    }

    Ok(())
}

/// Handle `config reset` subcommand.
async fn config_reset(tool: String, concise: bool) -> ToolResult<()> {
    // Resolve tool
    let resolved_path = resolve_tool_path(&tool).await?;
    let resolved = load_tool_from_path(&resolved_path.path)?;

    // Parse the original tool reference for storage
    let plugin_ref = parse_tool_ref_for_config(&tool, &resolved, resolved_path.is_installed)?;

    // Check if config exists
    let config_path = get_config_path(&plugin_ref);
    if !config_path.exists() {
        if concise {
            println!("ok");
        } else {
            println!(
                "\n  {} No configuration to reset for {}\n",
                "!".bright_yellow(),
                plugin_ref
            );
        }
        return Ok(());
    }

    // Delete config
    delete_tool_config(&plugin_ref)?;

    if concise {
        println!("ok");
    } else {
        println!(
            "\n  {} Removed all configuration for {}\n",
            "✓".bright_green(),
            plugin_ref
        );
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: Helpers
//--------------------------------------------------------------------------------------------------

/// Parse a tool reference for config storage.
///
/// Config storage is based on:
/// 1. For installed tools - use the reference as provided
/// 2. For local paths or fallback resolution - use the manifest name only
///
/// Namespace is ONLY preserved when the tool actually resolves as an installed plugin.
/// If "appcypher/sensitive" doesn't exist as installed and falls back to a local path,
/// config uses the manifest name without namespace.
///
/// The `is_installed` flag should come from `resolve_tool_path` which uses
/// `FilePluginResolver` to determine if a tool is installed.
pub fn parse_tool_ref_for_config(
    tool: &str,
    resolved: &crate::resolver::ResolvedPlugin<crate::mcpb::McpbManifest>,
    is_installed: bool,
) -> ToolResult<PluginRef> {
    // Only use the reference as-is if it's an installed tool
    if is_installed && let Ok(plugin_ref) = PluginRef::parse(tool) {
        return Ok(plugin_ref);
    }

    // For paths and local fallbacks, use manifest name only
    let name = resolved
        .template
        .name
        .as_ref()
        .ok_or_else(|| ToolError::Generic("Tool manifest has no name".into()))?;
    PluginRef::new(name)
}

/// Get config file path for a tool reference (without version).
fn get_config_path(plugin_ref: &PluginRef) -> PathBuf {
    let mut path = DEFAULT_CONFIG_PATH.clone();

    if let Some(ns) = plugin_ref.namespace() {
        path = path.join(ns);
    }

    path.join(plugin_ref.name()).join("config.json")
}

/// Get config directory path for a tool reference (without version).
fn get_config_dir(plugin_ref: &PluginRef) -> PathBuf {
    let mut path = DEFAULT_CONFIG_PATH.clone();

    if let Some(ns) = plugin_ref.namespace() {
        path = path.join(ns);
    }

    path.join(plugin_ref.name())
}

/// Load saved config for a tool.
pub fn load_tool_config(plugin_ref: &PluginRef) -> ToolResult<BTreeMap<String, String>> {
    let config_path = get_config_path(plugin_ref);

    if !config_path.exists() {
        return Ok(BTreeMap::new());
    }

    let content = std::fs::read_to_string(&config_path)?;

    // Check if config is encrypted
    if let Ok(envelope) = serde_json::from_str::<EncryptedConfigEnvelope>(&content)
        && envelope.encrypted
    {
        return decrypt_config(&envelope);
    }

    // Plain config
    let config: BTreeMap<String, String> = serde_json::from_str(&content)
        .map_err(|e| ToolError::Generic(format!("Failed to parse config file: {}", e)))?;

    Ok(config)
}

/// Save config for a tool with optional schema for encryption.
///
/// If schema contains sensitive fields, the entire config is encrypted.
pub fn save_tool_config_with_schema(
    plugin_ref: &PluginRef,
    config: &BTreeMap<String, String>,
    schema: Option<&BTreeMap<String, McpbUserConfigField>>,
) -> ToolResult<()> {
    let config_dir = get_config_dir(plugin_ref);
    let config_path = get_config_path(plugin_ref);

    // Create directory
    std::fs::create_dir_all(&config_dir)?;

    // Check if we need to encrypt (schema has sensitive fields that are in config)
    let should_encrypt = schema
        .map(|s| has_sensitive_values(s, config))
        .unwrap_or(false);

    let content = if should_encrypt {
        encrypt_config(config)?
    } else {
        serde_json::to_string_pretty(config)?
    };

    // Write config
    std::fs::write(&config_path, &content)?;

    // Set secure permissions (Unix only: owner read/write only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&config_path, permissions)?;
    }

    Ok(())
}

/// Check if schema has sensitive fields that are present in config.
fn has_sensitive_values(
    schema: &BTreeMap<String, McpbUserConfigField>,
    config: &BTreeMap<String, String>,
) -> bool {
    schema
        .iter()
        .any(|(key, field)| field.sensitive.unwrap_or(false) && config.contains_key(key))
}

/// Encrypt config using AES-256-GCM.
fn encrypt_config(config: &BTreeMap<String, String>) -> ToolResult<String> {
    let crypto = get_credential_crypto().ok_or_else(|| {
        ToolError::Generic("Could not initialize encryption for sensitive config".to_string())
    })?;

    let payload = serde_json::to_value(config)?;
    let encrypted = crypto
        .encrypt(&payload)
        .map_err(|e| ToolError::Generic(format!("Failed to encrypt config: {}", e)))?;

    let envelope = EncryptedConfigEnvelope {
        encrypted: true,
        nonce: BASE64.encode(&encrypted.nonce),
        auth_tag: BASE64.encode(&encrypted.auth_tag),
        ciphertext: BASE64.encode(&encrypted.ciphertext),
    };

    Ok(serde_json::to_string_pretty(&envelope)?)
}

/// Decrypt config from encrypted envelope.
fn decrypt_config(envelope: &EncryptedConfigEnvelope) -> ToolResult<BTreeMap<String, String>> {
    let crypto = get_credential_crypto().ok_or_else(|| {
        ToolError::Generic("Could not initialize decryption for sensitive config".to_string())
    })?;

    let nonce = BASE64
        .decode(&envelope.nonce)
        .map_err(|e| ToolError::Generic(format!("Invalid nonce encoding: {}", e)))?;
    let auth_tag = BASE64
        .decode(&envelope.auth_tag)
        .map_err(|e| ToolError::Generic(format!("Invalid auth_tag encoding: {}", e)))?;
    let ciphertext = BASE64
        .decode(&envelope.ciphertext)
        .map_err(|e| ToolError::Generic(format!("Invalid ciphertext encoding: {}", e)))?;

    let decrypted = crypto
        .decrypt(&ciphertext, &nonce, &auth_tag)
        .map_err(|e| ToolError::Generic(format!("Failed to decrypt config: {}", e)))?;

    let config: BTreeMap<String, String> = serde_json::from_value(decrypted)
        .map_err(|e| ToolError::Generic(format!("Failed to parse decrypted config: {}", e)))?;

    Ok(config)
}

/// Delete config for a tool.
fn delete_tool_config(plugin_ref: &PluginRef) -> ToolResult<()> {
    let config_dir = get_config_dir(plugin_ref);

    if config_dir.exists() {
        std::fs::remove_dir_all(&config_dir)?;
    }

    Ok(())
}

/// List all tools with saved config.
fn list_configured_tools() -> ToolResult<Vec<(String, PathBuf, usize)>> {
    let config_root = DEFAULT_CONFIG_PATH.clone();

    if !config_root.exists() {
        return Ok(Vec::new());
    }

    let mut tools = Vec::new();

    // Walk config directory
    for entry in std::fs::read_dir(&config_root)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();

            // Check if this is a namespace directory or a tool directory
            let config_file = path.join("config.json");
            if config_file.exists() {
                // This is a tool directory (no namespace)
                let count = count_config_keys(&config_file)?;
                tools.push((name, config_file, count));
            } else {
                // This might be a namespace directory
                for sub_entry in std::fs::read_dir(&path)? {
                    let sub_entry = sub_entry?;
                    let sub_path = sub_entry.path();

                    if sub_path.is_dir() {
                        let sub_name = sub_entry.file_name().to_string_lossy().to_string();
                        let sub_config_file = sub_path.join("config.json");

                        if sub_config_file.exists() {
                            let full_name = format!("{}/{}", name, sub_name);
                            let count = count_config_keys(&sub_config_file)?;
                            tools.push((full_name, sub_config_file, count));
                        }
                    }
                }
            }
        }
    }

    // Sort by name
    tools.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(tools)
}

/// Count keys in a config file.
fn count_config_keys(path: &PathBuf) -> ToolResult<usize> {
    let content = std::fs::read_to_string(path)?;
    let config: BTreeMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
    Ok(config.len())
}

/// Parse config values from KEY=VALUE strings.
fn parse_config_values(values: &[String]) -> ToolResult<BTreeMap<String, String>> {
    let mut config = BTreeMap::new();

    for value in values {
        if let Some((key, val)) = value.split_once('=') {
            config.insert(key.to_string(), val.to_string());
        } else {
            return Err(ToolError::Generic(format!(
                "Invalid config format '{}'. Expected key=value",
                value
            )));
        }
    }

    Ok(config)
}

/// Validate a value against its schema field.
fn validate_field_value(key: &str, value: &str, field: &McpbUserConfigField) -> ToolResult<()> {
    match &field.field_type {
        McpbUserConfigType::Number => {
            let num: f64 = value.parse().map_err(|_| {
                ToolError::Generic(format!("'{}' must be a valid number, got '{}'", key, value))
            })?;

            if let Some(min) = field.min
                && num < min
            {
                return Err(ToolError::Generic(format!(
                    "'{}' must be at least {}, got {}",
                    key, min, num
                )));
            }

            if let Some(max) = field.max
                && num > max
            {
                return Err(ToolError::Generic(format!(
                    "'{}' must be at most {}, got {}",
                    key, max, num
                )));
            }
        }
        McpbUserConfigType::Boolean => {
            if value != "true" && value != "false" {
                return Err(ToolError::Generic(format!(
                    "'{}' must be 'true' or 'false', got '{}'",
                    key, value
                )));
            }
        }
        _ => {}
    }

    Ok(())
}

/// Interactive prompt for all user_config fields.
fn prompt_all_user_config(
    schema: &BTreeMap<String, McpbUserConfigField>,
    existing: &BTreeMap<String, String>,
) -> ToolResult<BTreeMap<String, String>> {
    init_theme();
    cliclack::intro("Tool configuration")?;

    let mut result = BTreeMap::new();

    for (key, field) in schema {
        // Skip if already provided
        if existing.contains_key(key) {
            continue;
        }

        let is_sensitive = field.sensitive.unwrap_or(false);
        let is_required = field.required.unwrap_or(false);

        // Use title for display
        let display_name = &field.title;
        let description = field.description.as_deref().unwrap_or("");

        // Build prompt text
        let prompt_text = if description.is_empty() {
            display_name.clone()
        } else {
            format!("{} ({})", display_name, description)
        };

        // Get default value (resolve variables)
        let default_value = field.default.as_ref().map(|d| {
            let raw = match d {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => d.to_string(),
            };
            resolve_variables(&raw)
        });

        // Copy min/max for validation closure
        let min_val = field.min;
        let max_val = field.max;

        let value: String = match &field.field_type {
            McpbUserConfigType::Boolean => {
                // Use confirm prompt for booleans
                let default_bool = default_value.as_ref().map(|v| v == "true").unwrap_or(false);
                let confirmed = cliclack::confirm(&prompt_text)
                    .initial_value(default_bool)
                    .interact()?;
                confirmed.to_string()
            }
            McpbUserConfigType::Number => {
                // Text input with validation
                let input: String = if let Some(default) = &default_value {
                    cliclack::input(&prompt_text)
                        .default_input(default)
                        .validate(move |v: &String| validate_number_range(v, min_val, max_val))
                        .interact()?
                } else {
                    cliclack::input(&prompt_text)
                        .required(is_required)
                        .validate(move |v: &String| validate_number_range(v, min_val, max_val))
                        .interact()?
                };
                input
            }
            _ => {
                // string, directory, file
                if is_sensitive {
                    let mut password = cliclack::password(&prompt_text);
                    if !is_required {
                        password = password.allow_empty();
                    }
                    password.interact()?
                } else if let Some(default) = &default_value {
                    cliclack::input(&prompt_text)
                        .default_input(default)
                        .interact()?
                } else {
                    cliclack::input(&prompt_text)
                        .required(is_required)
                        .interact()?
                }
            }
        };

        // Expand ~ for directory/file types
        let final_value = match &field.field_type {
            McpbUserConfigType::Directory | McpbUserConfigType::File => expand_tilde(&value),
            _ => value,
        };

        if !final_value.is_empty() {
            result.insert(key.clone(), final_value);
        }
    }

    cliclack::outro("Configuration complete!")?;
    Ok(result)
}

/// Validate number input for cliclack.
fn validate_number_range(
    value: &str,
    min: Option<f64>,
    max: Option<f64>,
) -> Result<(), &'static str> {
    if value.is_empty() {
        return Ok(());
    }

    let num: f64 = value.parse().map_err(|_| "Must be a valid number")?;

    if let Some(min) = min
        && num < min
    {
        return Err("Value is below minimum");
    }

    if let Some(max) = max
        && num > max
    {
        return Err("Value is above maximum");
    }

    Ok(())
}

/// Expand ~ to home directory.
fn expand_tilde(path: &str) -> String {
    if let Some(suffix) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(suffix).to_string_lossy().to_string();
    }
    path.to_string()
}

/// Resolve ${VAR} placeholders in strings.
fn resolve_variables(s: &str) -> String {
    let mut result = s.to_string();

    if let Some(home) = dirs::home_dir() {
        result = result.replace("${HOME}", &home.to_string_lossy());
    }
    if let Some(desktop) = dirs::desktop_dir() {
        result = result.replace("${DESKTOP}", &desktop.to_string_lossy());
    }
    if let Some(docs) = dirs::document_dir() {
        result = result.replace("${DOCUMENTS}", &docs.to_string_lossy());
    }
    if let Some(downloads) = dirs::download_dir() {
        result = result.replace("${DOWNLOADS}", &downloads.to_string_lossy());
    }

    result
}

/// Mask a sensitive value for display.
fn mask_sensitive(value: &str) -> String {
    if value.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}...{}", &value[..3], &value[value.len() - 3..])
    }
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcpb::McpbManifest;
    use crate::references::PluginRef;
    use crate::resolver::ResolvedPlugin;
    use std::path::PathBuf;

    fn create_resolved_plugin(name: &str) -> ResolvedPlugin<McpbManifest> {
        let json = format!(
            r#"{{
                "manifest_version": "0.3",
                "name": "{}",
                "version": "1.0.0",
                "description": "Test tool",
                "author": {{ "name": "Test" }},
                "server": {{ "type": "node", "entry_point": "index.js" }}
            }}"#,
            name
        );
        let manifest: McpbManifest = serde_json::from_str(&json).unwrap();
        ResolvedPlugin {
            template: manifest,
            path: PathBuf::from("/tmp/test/manifest.json"),
            plugin_ref: PluginRef::new(name).unwrap(),
        }
    }

    #[test]
    fn test_parse_tool_ref_installed_simple_name() {
        let resolved = create_resolved_plugin("my-tool");
        let result = parse_tool_ref_for_config("my-tool", &resolved, true).unwrap();
        assert_eq!(result.name(), "my-tool");
        assert!(result.namespace().is_none());
    }

    #[test]
    fn test_parse_tool_ref_installed_with_namespace() {
        let resolved = create_resolved_plugin("my-tool");
        let result = parse_tool_ref_for_config("appcypher/my-tool", &resolved, true).unwrap();
        assert_eq!(result.name(), "my-tool");
        assert_eq!(result.namespace(), Some("appcypher"));
    }

    #[test]
    fn test_parse_tool_ref_installed_strips_version() {
        let resolved = create_resolved_plugin("my-tool");
        let result = parse_tool_ref_for_config("appcypher/my-tool@1.0.0", &resolved, true).unwrap();
        assert_eq!(result.name(), "my-tool");
        assert_eq!(result.namespace(), Some("appcypher"));
    }

    #[test]
    fn test_parse_tool_ref_not_installed_uses_manifest_name() {
        let resolved = create_resolved_plugin("manifest-name");
        // Even though ref has namespace, not installed means use manifest name
        let result = parse_tool_ref_for_config("appcypher/my-tool", &resolved, false).unwrap();
        assert_eq!(result.name(), "manifest-name");
        assert!(result.namespace().is_none());
    }

    #[test]
    fn test_parse_tool_ref_local_path_uses_manifest_name() {
        let resolved = create_resolved_plugin("local-tool");
        let result = parse_tool_ref_for_config("./my-dir", &resolved, false).unwrap();
        assert_eq!(result.name(), "local-tool");
        assert!(result.namespace().is_none());
    }

    #[test]
    fn test_parse_tool_ref_dot_path_uses_manifest_name() {
        let resolved = create_resolved_plugin("current-dir-tool");
        let result = parse_tool_ref_for_config(".", &resolved, false).unwrap();
        assert_eq!(result.name(), "current-dir-tool");
        assert!(result.namespace().is_none());
    }

    #[test]
    fn test_parse_tool_ref_invalid_ref_not_installed_uses_manifest() {
        let resolved = create_resolved_plugin("nested-tool");
        // "org/team/tool" is invalid plugin ref, not installed → manifest name
        let result = parse_tool_ref_for_config("org/team/tool", &resolved, false).unwrap();
        assert_eq!(result.name(), "nested-tool");
        assert!(result.namespace().is_none());
    }

    #[test]
    fn test_get_config_path_no_namespace() {
        let plugin_ref = PluginRef::new("my-tool").unwrap();
        let path = get_config_path(&plugin_ref);
        assert!(path.ends_with("my-tool/config.json"));
        assert!(!path.to_string_lossy().contains("//"));
    }

    #[test]
    fn test_get_config_path_with_namespace() {
        let plugin_ref = PluginRef::new("my-tool")
            .unwrap()
            .with_namespace("appcypher")
            .unwrap();
        let path = get_config_path(&plugin_ref);
        assert!(path.to_string_lossy().contains("appcypher"));
        assert!(path.ends_with("appcypher/my-tool/config.json"));
    }

    #[test]
    fn test_mask_sensitive_short() {
        assert_eq!(mask_sensitive("secret"), "***");
        assert_eq!(mask_sensitive("12345678"), "***");
    }

    #[test]
    fn test_mask_sensitive_long() {
        assert_eq!(mask_sensitive("123456789"), "123...789");
        assert_eq!(mask_sensitive("my-secret-api-key"), "my-...key");
    }
}
