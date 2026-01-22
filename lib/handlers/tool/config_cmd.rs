//! Tool config command handlers.

use crate::commands::ConfigCommand;
use crate::constants::DEFAULT_CONFIG_PATH;
use crate::error::{ToolError, ToolResult};
use crate::mcpb::{McpbUserConfigField, McpbUserConfigType};
use crate::prompt::init_theme;
use crate::references::PluginRef;
use crate::resolver::load_tool_from_path;
use colored::Colorize;
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;

use super::list::resolve_tool_path;

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
    let tool_path = resolve_tool_path(&tool).await?;
    let resolved = load_tool_from_path(&tool_path)?;

    // Parse the original tool reference for storage (strip version for config path)
    let plugin_ref = parse_tool_ref_for_config(&tool, &resolved)?;

    let schema = resolved.template.user_config;

    // Check if tool has configurable options
    let schema = schema.ok_or_else(|| {
        ToolError::Generic(format!("Tool '{}' has no configurable options", plugin_ref))
    })?;

    if schema.is_empty() {
        return Err(ToolError::Generic(format!(
            "Tool '{}' has no configurable options",
            plugin_ref
        )));
    }

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

    // Save config
    save_tool_config(&plugin_ref, &final_config)?;

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
        println!();
    }

    Ok(())
}

/// Handle `config get` subcommand.
async fn config_get(
    tool: String,
    key: Option<String>,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    // Resolve tool and load manifest
    let tool_path = resolve_tool_path(&tool).await?;
    let resolved = load_tool_from_path(&tool_path)?;

    // Parse the original tool reference for storage
    let plugin_ref = parse_tool_ref_for_config(&tool, &resolved)?;

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
    let tool_path = resolve_tool_path(&tool).await?;
    let resolved = load_tool_from_path(&tool_path)?;

    // Parse the original tool reference for storage
    let plugin_ref = parse_tool_ref_for_config(&tool, &resolved)?;

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
        save_tool_config(&plugin_ref, &config)?;
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
    let tool_path = resolve_tool_path(&tool).await?;
    let resolved = load_tool_from_path(&tool_path)?;

    // Parse the original tool reference for storage
    let plugin_ref = parse_tool_ref_for_config(&tool, &resolved)?;

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
/// This handles three cases:
/// 1. Local path (e.g., ".") - uses manifest name
/// 2. Plugin reference (e.g., "appcypher/filesystem") - parses directly
/// 3. Versioned reference (e.g., "appcypher/filesystem@1.0.0") - strips version
pub fn parse_tool_ref_for_config(
    tool: &str,
    resolved: &crate::resolver::ResolvedPlugin<crate::mcpb::McpbManifest>,
) -> ToolResult<PluginRef> {
    // Check if it's a path-like reference
    if tool == "." || tool.starts_with("./") || tool.starts_with('/') || tool.contains("..") {
        // For path references, try to get name from manifest
        let name = resolved
            .template
            .name
            .as_ref()
            .ok_or_else(|| ToolError::Generic("Tool manifest has no name".into()))?;
        return PluginRef::new(name);
    }

    // Try to parse as plugin reference
    if let Ok(mut plugin_ref) = PluginRef::parse(tool) {
        // If parsed but no namespace, check if we can infer from resolved path
        if plugin_ref.namespace().is_none() {
            // Try to detect namespace from the resolved path
            // Path pattern: ~/.tool/tools/<namespace>/<name>@<version>/manifest.json
            if let Some(parent) = resolved.path.parent() {
                // parent is the tool directory (e.g., filesystem@0.1.2)
                if let Some(namespace_dir) = parent.parent() {
                    // namespace_dir might be the namespace or tools root
                    if let Some(ns_name) = namespace_dir.file_name().and_then(|n| n.to_str())
                        && ns_name != "tools"
                        && !ns_name.contains('@')
                        && PluginRef::new(ns_name)
                            .and_then(|r| r.with_namespace(ns_name))
                            .is_ok()
                        && let Ok(ref_with_ns) = plugin_ref.clone().with_namespace(ns_name)
                    {
                        plugin_ref = ref_with_ns;
                    }
                }
            }
        }
        return Ok(plugin_ref);
    }

    // Fallback: use manifest name
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
    let config: BTreeMap<String, String> = serde_json::from_str(&content)
        .map_err(|e| ToolError::Generic(format!("Failed to parse config file: {}", e)))?;

    Ok(config)
}

/// Save config for a tool.
pub fn save_tool_config(
    plugin_ref: &PluginRef,
    config: &BTreeMap<String, String>,
) -> ToolResult<()> {
    let config_dir = get_config_dir(plugin_ref);
    let config_path = get_config_path(plugin_ref);

    // Create directory
    std::fs::create_dir_all(&config_dir)?;

    // Write config
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&config_path, content)?;

    Ok(())
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
                    cliclack::password(&prompt_text).interact()?
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
