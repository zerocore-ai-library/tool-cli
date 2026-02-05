//! Tool config command handlers.

use crate::commands::ConfigCommand;
use crate::constants::DEFAULT_CONFIG_PATH;
use crate::error::{ToolError, ToolResult};
use crate::mcp::connect_with_oauth;
use crate::mcpb::{McpbTransport, McpbUserConfigField, McpbUserConfigType};
use crate::output::{
    ConfigGetEntry, ConfigGetOutput, ConfigListEntry, ConfigListOutput, ConfigOAuthOutput,
    ConfigPropertyOutput, ConfigSchemaOutput,
};
use crate::prompt::init_theme;
use crate::references::PluginRef;
use crate::security::get_credential_crypto;
use crate::system_config::allocate_system_config;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use super::common::resolve_tool;

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
        ConfigCommand::Get { tool, key, json } => {
            config_get(tool, key, json, concise, no_header).await
        }
        ConfigCommand::List { tool, json } => config_list(tool, json, concise, no_header).await,
        ConfigCommand::Unset {
            tool,
            keys,
            all,
            yes,
        } => config_unset(tool, keys, all, yes, concise).await,
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
    // Resolve tool, load manifest, auto-install, derive config key
    let resolved = resolve_tool(&tool, true, yes).await?;
    let plugin_ref = resolved.plugin_ref;

    // Clone the schema since we need resolved.plugin later for OAuth
    let schema = resolved.plugin.template.user_config.clone();

    // Check if tool uses HTTP transport (may need OAuth via MCP-Auth discovery)
    let is_http_tool = resolved.plugin.template.server.transport == McpbTransport::Http;

    // Check if tool has configurable options (user_config or HTTP transport)
    let has_user_config = schema.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

    if !has_user_config && !is_http_tool {
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
        try_oauth_for_http_tool(&resolved.plugin, &final_config, &plugin_ref.to_string()).await;

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
            println!("  · {:<20} {}", key, display_value.dimmed());
        }

        // Show OAuth status
        match oauth_result {
            OAuthSetupResult::NotRequired => {}
            OAuthSetupResult::AlreadyAuthenticated => {
                println!("  · {:<20} {}", "OAuth", "authenticated".bright_green());
            }
            OAuthSetupResult::Authenticated => {
                println!("  · {:<20} {}", "OAuth", "authenticated".bright_green());
            }
            OAuthSetupResult::Skipped(reason) => {
                println!("  · {:<20} {}", "OAuth", reason.dimmed());
            }
            OAuthSetupResult::Failed(err) => {
                println!(
                    "  · {:<20} {} ({})",
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
    json_output: bool,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    // Resolve tool (with fallback for uninstalled tools that have saved config)
    let (plugin_ref, schema) = resolve_tool_for_config(&tool).await?;

    // Load config
    let config = load_tool_config(&plugin_ref)?;

    if config.is_empty() {
        if json_output {
            let output = ConfigGetOutput {
                tool: plugin_ref.to_string(),
                config: BTreeMap::new(),
            };
            if concise {
                println!("{}", serde_json::to_string(&output)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            return Ok(());
        }
        if concise {
            // Empty output for concise mode
            return Ok(());
        }
        println!("\n  No configuration saved for {}\n", plugin_ref);
        if let Some(schema) = schema {
            println!("  Available config fields:");
            for (key, field) in &schema {
                let title = &field.title;
                let req = if field.required.unwrap_or(false) {
                    " (required)"
                } else {
                    ""
                };
                println!("  · {:<20} {}{}", key, title.dimmed(), req.dimmed());
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

        let sensitive = schema
            .as_ref()
            .and_then(|s| s.get(&key))
            .map(|f| f.sensitive.unwrap_or(false))
            .unwrap_or(false);

        if json_output {
            let mut entries = BTreeMap::new();
            entries.insert(
                key.clone(),
                ConfigGetEntry {
                    value: if sensitive {
                        mask_sensitive(value)
                    } else {
                        value.clone()
                    },
                    sensitive,
                },
            );
            let output = ConfigGetOutput {
                tool: plugin_ref.to_string(),
                config: entries,
            };
            if concise {
                println!("{}", serde_json::to_string(&output)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
        } else if concise {
            println!("{}", value);
        } else {
            println!("\n  {}.{} = {}\n", plugin_ref, key, value);
        }
        return Ok(());
    }

    // Show all config
    if json_output {
        let mut entries = BTreeMap::new();
        for (key, value) in &config {
            let sensitive = schema
                .as_ref()
                .and_then(|s| s.get(key))
                .map(|f| f.sensitive.unwrap_or(false))
                .unwrap_or(false);
            entries.insert(
                key.clone(),
                ConfigGetEntry {
                    value: if sensitive {
                        mask_sensitive(value)
                    } else {
                        value.clone()
                    },
                    sensitive,
                },
            );
        }
        let output = ConfigGetOutput {
            tool: plugin_ref.to_string(),
            config: entries,
        };
        if concise {
            println!("{}", serde_json::to_string(&output)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    } else if concise {
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
            println!("  · {:<20} {}", key, display_value);
        }

        println!(
            "\n  · {}: {}\n",
            "Path".dimmed(),
            config_path.display().to_string().dimmed()
        );
    }

    Ok(())
}

/// Handle `config list` subcommand.
async fn config_list(
    tool: Option<String>,
    json_output: bool,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    if let Some(tool) = tool {
        return config_list_tool(&tool, json_output, concise, no_header).await;
    }

    let tools = list_configured_tools()?;

    if tools.is_empty() {
        if concise || json_output {
            if json_output {
                let output = ConfigListOutput {
                    tools: BTreeMap::new(),
                };
                if concise {
                    println!("{}", serde_json::to_string(&output)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
            }
            return Ok(());
        }
        println!("\n  No tools have saved configuration.\n");
        println!(
            "  · Use {} to configure a tool.\n",
            "tool config set <tool>".bright_cyan()
        );
        return Ok(());
    }

    if json_output {
        let mut entries = BTreeMap::new();
        for (name, path, count) in &tools {
            entries.insert(
                name.clone(),
                ConfigListEntry {
                    keys: *count,
                    path: path.display().to_string(),
                },
            );
        }
        let output = ConfigListOutput { tools: entries };
        if concise {
            println!("{}", serde_json::to_string(&output)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    } else if concise {
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
                "  · {:<30} {} {}    {}",
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

/// Handle `config list <tool>` - show config schema for a specific tool.
async fn config_list_tool(
    tool: &str,
    json_output: bool,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    let resolved = resolve_tool(tool, false, false).await?;
    let plugin_ref = &resolved.plugin_ref;
    let schema = resolved
        .plugin
        .template
        .user_config
        .clone()
        .unwrap_or_default();
    let is_http = resolved.plugin.template.server.transport == McpbTransport::Http;

    // Check OAuth status for HTTP tools
    let oauth_status = if is_http {
        Some(check_oauth_status(&plugin_ref.to_string()).await)
    } else {
        None
    };

    let has_content = !schema.is_empty() || oauth_status.is_some();

    if !has_content {
        if json_output {
            let output = ConfigSchemaOutput {
                tool: plugin_ref.to_string(),
                user_config: BTreeMap::new(),
                oauth: None,
            };
            if concise {
                println!("{}", serde_json::to_string(&output)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
        } else if !concise {
            println!("\n  Config: {}\n", plugin_ref.to_string().bold());
            println!("    {}", "No configurable options.".dimmed());
            println!();
        }
        return Ok(());
    }

    if json_output {
        let mut user_config = BTreeMap::new();
        for (key, field) in &schema {
            user_config.insert(
                key.clone(),
                ConfigPropertyOutput {
                    field_type: config_type_str(&field.field_type),
                    title: field.title.clone(),
                    description: field.description.clone(),
                    required: field.required,
                    default: field.default.clone(),
                    sensitive: field.sensitive,
                    min: field.min,
                    max: field.max,
                    enum_values: field.enum_values.clone(),
                },
            );
        }
        let oauth = oauth_status.map(|(authenticated, expired)| ConfigOAuthOutput {
            authenticated,
            expired: if authenticated { Some(expired) } else { None },
        });
        let output = ConfigSchemaOutput {
            tool: plugin_ref.to_string(),
            user_config,
            oauth,
        };
        if concise {
            println!("{}", serde_json::to_string(&output)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    } else if concise {
        if !no_header {
            println!("#key\ttype\trequired\tsensitive\tdefault\ttitle");
        }
        for (key, field) in &schema {
            let required = field.required.unwrap_or(false);
            let sensitive = field.sensitive.unwrap_or(false);
            let default = field
                .default
                .as_ref()
                .map(|d| match d {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                key,
                config_type_str(&field.field_type),
                required,
                sensitive,
                default,
                field.title,
            );
        }
        if let Some((authenticated, expired)) = oauth_status {
            let status = if !authenticated {
                "not authenticated"
            } else if expired {
                "expired"
            } else {
                "authenticated"
            };
            println!("oauth\t\t\t\t\t{}", status);
        }
    } else {
        println!("\n  Config: {}\n", plugin_ref.to_string().bold());

        let schema_vec: Vec<_> = schema.iter().collect();
        for (idx, (key, field)) in schema_vec.iter().enumerate() {
            let req_marker = if field.required.unwrap_or(false) {
                "*"
            } else {
                ""
            };
            let name = format!("{}{}", key, req_marker);

            // First line: name + description
            let desc = field
                .description
                .as_deref()
                .and_then(|d| crate::format::format_description(d, false, ""))
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("  {}{}", name.bright_cyan(), desc);

            // Properties below
            println!("  · {:<14} {}", "Title".dimmed(), field.title);
            println!(
                "  · {:<14} {}",
                "Type".dimmed(),
                config_type_str(&field.field_type)
            );

            if field.sensitive.unwrap_or(false) {
                println!("  · {:<14} {}", "Sensitive".dimmed(), "yes".dimmed());
            }

            if let Some(default) = &field.default {
                let val = match default {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                println!("  · {:<14} {}", "Default".dimmed(), val.dimmed());
            }

            if let Some(min) = field.min {
                println!("  · {:<14} {}", "Min".dimmed(), min.to_string().dimmed());
            }

            if let Some(max) = field.max {
                println!("  · {:<14} {}", "Max".dimmed(), max.to_string().dimmed());
            }

            if let Some(enum_values) = &field.enum_values {
                println!(
                    "  · {:<14} {}",
                    "Enum".dimmed(),
                    enum_values.join(", ").dimmed()
                );
            }

            if idx < schema_vec.len() - 1 {
                println!();
            }
        }

        if let Some((authenticated, expired)) = oauth_status {
            if !schema.is_empty() {
                println!();
            }
            let status = if !authenticated {
                "not authenticated".dimmed().to_string()
            } else if expired {
                "expired".bright_yellow().to_string()
            } else {
                "authenticated".bright_green().to_string()
            };
            println!("  · {:<14} {}", "OAuth".dimmed(), status);
        }

        println!();
    }

    Ok(())
}

/// Check OAuth credential status for a tool.
///
/// Returns `(authenticated, expired)` tuple.
async fn check_oauth_status(tool_ref: &str) -> (bool, bool) {
    match crate::oauth::load_credentials(tool_ref).await {
        Ok(Some(creds)) => (true, creds.is_expired()),
        _ => (false, false),
    }
}

/// Convert McpbUserConfigType to a display string.
fn config_type_str(t: &McpbUserConfigType) -> String {
    match t {
        McpbUserConfigType::String => "string".to_string(),
        McpbUserConfigType::Number => "number".to_string(),
        McpbUserConfigType::Boolean => "boolean".to_string(),
        McpbUserConfigType::Directory => "directory".to_string(),
        McpbUserConfigType::File => "file".to_string(),
    }
}

/// Handle `config unset` subcommand.
async fn config_unset(
    tool: Option<String>,
    keys: Vec<String>,
    all: bool,
    yes: bool,
    concise: bool,
) -> ToolResult<()> {
    match (tool, all) {
        // `tool config unset --all` or `tool config unset --all <keys...>`
        (None, true) => unset_all_tools(keys, yes, concise).await,

        // `tool config unset <tool> --all`
        (Some(tool), true) => unset_tool_all_keys(&tool, yes, concise).await,

        // `tool config unset <tool> <keys...>`
        (Some(tool), false) => {
            if keys.is_empty() {
                return Err(ToolError::Generic(
                    "No keys specified. Use --all to remove all keys.".into(),
                ));
            }
            unset_tool_keys(&tool, &keys, concise).await
        }

        // `tool config unset` (no tool, no --all)
        (None, false) => Err(ToolError::Generic(
            "Either specify a tool or use --all.".into(),
        )),
    }
}

/// Unset specific keys from a single tool.
async fn unset_tool_keys(tool: &str, keys: &[String], concise: bool) -> ToolResult<()> {
    let (plugin_ref, schema) = resolve_tool_for_config(tool).await?;

    let mut config = load_tool_config(&plugin_ref).unwrap_or_default();
    let mut removed = Vec::new();
    let mut not_found = Vec::new();

    for key in keys {
        if config.remove(key).is_some() {
            removed.push(key.as_str());
        } else {
            not_found.push(key.as_str());
        }
    }

    if !removed.is_empty() {
        if config.is_empty() {
            delete_tool_config(&plugin_ref)?;
        } else {
            save_tool_config_with_schema(&plugin_ref, &config, schema.as_ref())?;
        }
    }

    if concise {
        println!("ok");
    } else {
        if !removed.is_empty() {
            println!(
                "\n  {} Removed {} from {}\n",
                "✓".bright_green(),
                removed.join(", "),
                plugin_ref
            );
        }
        if !not_found.is_empty() {
            println!(
                "  {} Keys not found: {}\n",
                "!".bright_yellow(),
                not_found.join(", ")
            );
        }
    }

    Ok(())
}

/// Unset all keys from a single tool.
async fn unset_tool_all_keys(tool: &str, yes: bool, concise: bool) -> ToolResult<()> {
    let (plugin_ref, _schema) = resolve_tool_for_config(tool).await?;

    let config_path = get_config_path(&plugin_ref);
    if !config_path.exists() {
        if concise {
            println!("ok");
        } else {
            println!(
                "\n  {} No configuration to remove for {}\n",
                "!".bright_yellow(),
                plugin_ref
            );
        }
        return Ok(());
    }

    // Confirm if not --yes
    if !yes && !concise {
        println!();
        println!(
            "  {} This will remove all configuration for {}",
            "!".bright_yellow(),
            plugin_ref
        );
        println!();
        print!("  Continue? [y/N] ");
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| ToolError::Generic(format!("Failed to read input: {}", e)))?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!();
            println!("  {} Cancelled", "✗".bright_red());
            println!();
            return Ok(());
        }
    }

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

/// Unset config for all tools, optionally filtering by keys.
async fn unset_all_tools(keys: Vec<String>, yes: bool, concise: bool) -> ToolResult<()> {
    let tools = list_configured_tools()?;

    if tools.is_empty() {
        if concise {
            println!("ok");
        } else {
            println!(
                "\n  {} No tools have saved configuration.\n",
                "!".bright_yellow()
            );
        }
        return Ok(());
    }

    // Confirm if not --yes
    if !yes && !concise {
        println!();
        let message = if keys.is_empty() {
            format!("remove all configuration for {} tool(s)", tools.len())
        } else {
            format!("remove {} from {} tool(s)", keys.join(", "), tools.len())
        };
        println!("  {} This will {}", "!".bright_yellow(), message);
        println!();
        print!("  Continue? [y/N] ");
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| ToolError::Generic(format!("Failed to read input: {}", e)))?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!();
            println!("  {} Cancelled", "✗".bright_red());
            println!();
            return Ok(());
        }
        println!();
    }

    let config_root = DEFAULT_CONFIG_PATH.clone();
    let mut removed_count = 0usize;

    if keys.is_empty() {
        // Remove all config for all tools
        for (name, _, _) in &tools {
            let tool_dir = config_root.join(name);
            if tool_dir.exists() {
                std::fs::remove_dir_all(&tool_dir)?;
                removed_count += 1;
                if !concise {
                    println!("  {} Removed all config for {}", "✓".bright_green(), name);
                }
            }
        }
    } else {
        // Remove specific keys from all tools
        for (name, config_path, _) in &tools {
            let mut config = load_config_from_path(config_path)?;
            let mut changed = false;

            for key in &keys {
                if config.remove(key).is_some() {
                    changed = true;
                }
            }

            if changed {
                if config.is_empty() {
                    // Delete the entire config directory
                    if let Some(parent) = config_path.parent() {
                        std::fs::remove_dir_all(parent)?;
                    }
                } else {
                    // Save updated config (without encryption since we don't have schema)
                    let json = serde_json::to_string_pretty(&config)?;
                    std::fs::write(config_path, json)?;
                }
                removed_count += 1;
                if !concise {
                    println!(
                        "  {} Removed {} from {}",
                        "✓".bright_green(),
                        keys.join(", "),
                        name
                    );
                }
            }
        }
    }

    if concise {
        println!("ok");
    } else {
        println!();
        if removed_count == 0 {
            println!(
                "  {} No matching configuration found.\n",
                "!".bright_yellow()
            );
        } else {
            println!(
                "  Updated {} tool{}.\n",
                removed_count,
                if removed_count == 1 { "" } else { "s" }
            );
        }
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: Helpers
//--------------------------------------------------------------------------------------------------

/// Resolve a tool reference for config operations (get/unset).
///
/// Tries normal resolution first (installed tools, paths). If that fails,
/// falls back to parsing the name directly and checking if config exists.
/// This allows `config unset mongodb` to work even when mongodb isn't installed.
async fn resolve_tool_for_config(
    tool: &str,
) -> ToolResult<(PluginRef, Option<BTreeMap<String, McpbUserConfigField>>)> {
    // Try normal resolution first (no auto-install for read operations)
    if let Ok(resolved) = resolve_tool(tool, false, false).await {
        let schema = resolved.plugin.template.user_config.clone();
        return Ok((resolved.plugin_ref, schema));
    }

    // Fallback: parse as a plugin ref and check if config exists
    let plugin_ref = PluginRef::parse(tool)
        .or_else(|_| PluginRef::new(tool))
        .map_err(|_| {
            ToolError::Generic(format!(
                "Tool '{}' not found and no saved configuration exists",
                tool
            ))
        })?;

    if !tool_config_exists(&plugin_ref) {
        return Err(ToolError::Generic(format!(
            "No configuration found for '{}'",
            tool
        )));
    }

    Ok((plugin_ref, None))
}

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
/// Check if a saved config file exists for the given tool.
pub fn tool_config_exists(plugin_ref: &PluginRef) -> bool {
    get_config_path(plugin_ref).exists()
}

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

/// Load config from a specific path (used for batch operations).
fn load_config_from_path(path: &PathBuf) -> ToolResult<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let content = std::fs::read_to_string(path)?;

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
