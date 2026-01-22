//! Tool call command handlers.

use crate::error::{ToolError, ToolResult};
use crate::mcp::call_tool_from_path;
use crate::mcpb::McpbUserConfigField;
use crate::resolver::load_tool_from_path;
use colored::Colorize;
use std::collections::BTreeMap;

use super::config_cmd::load_tool_config;
use super::list::resolve_tool_path;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Expand shorthand method syntax.
///
/// - `.exec` → `{tool}__exec`
/// - `.fs.read` → `{tool}__fs__read`
/// - `bash__exec` → `bash__exec` (unchanged)
fn expand_method_shorthand(method: &str, tool_name: &str) -> String {
    if let Some(suffix) = method.strip_prefix('.') {
        let expanded_suffix = suffix.replace('.', "__");
        format!("{}__{}", tool_name, expanded_suffix)
    } else {
        method.to_string()
    }
}

/// Extract tool name from reference for method expansion.
///
/// - `bash` → `bash`
/// - `appcypher/filesystem` → `filesystem`
/// - `appcypher/filesystem@1.0.0` → `filesystem`
fn extract_tool_name_for_expansion(tool_ref: &str) -> &str {
    tool_ref
        .split('@')
        .next()
        .unwrap_or(tool_ref) // strip version
        .rsplit('/')
        .next()
        .unwrap_or(tool_ref) // strip namespace
}

/// Call a tool method.
#[allow(clippy::too_many_arguments)]
pub async fn tool_call(
    tool: String,
    method: String,
    param: Vec<String>,
    args: Vec<String>,
    config: Vec<String>,
    config_file: Option<String>,
    verbose: bool,
    concise: bool,
) -> ToolResult<()> {
    // Merge -p flags and trailing args
    let params: Vec<String> = param.into_iter().chain(args).collect();

    // Expand method shorthand (.exec → toolname__exec)
    let tool_name_for_expansion = extract_tool_name_for_expansion(&tool);
    let method = expand_method_shorthand(&method, tool_name_for_expansion);

    // Parse method parameters
    let arguments = parse_method_params(&params)?;

    // Resolve tool path
    let tool_path = resolve_tool_path(&tool).await?;

    // Load manifest to get user_config schema
    let resolved_plugin = load_tool_from_path(&tool_path)?;
    let manifest_schema = resolved_plugin.template.user_config.as_ref();

    // Parse user config from saved config, config file, and -C flags
    let mut user_config = parse_user_config(
        &config,
        config_file.as_deref(),
        Some(&resolved_plugin.plugin_ref),
    )?;

    // Prompt for missing required config values, then apply defaults
    prompt_missing_user_config(manifest_schema, &mut user_config)?;
    apply_user_config_defaults(manifest_schema, &mut user_config);

    // Get tool name for display
    let tool_name = tool_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&tool);

    // Call the tool - handle EntryPointNotFound specially
    let result =
        match call_tool_from_path(&tool_path, &method, arguments, &user_config, verbose).await {
            Ok(result) => result,
            Err(ToolError::EntryPointNotFound {
                entry_point,
                full_path: _,
                build_script,
                bundle_path,
            }) => {
                println!(
                    "  {} Entry point not found: {}\n",
                    "✗".bright_red(),
                    entry_point.bright_white()
                );
                if let Some(build_cmd) = build_script {
                    println!("    The tool needs to be built before it can be run.\n");
                    println!("    {}:", "To build".dimmed());
                    println!("      cd {} && tool build\n", bundle_path);
                    println!("    {}: {}", "Runs".dimmed(), build_cmd.dimmed());
                } else {
                    println!("    {}:", "If this tool requires building".dimmed());
                    println!("      Add a build script to manifest.json:\n");
                    println!("      {}", "\"_meta\": {".dimmed());
                    println!("        {}", "\"store.tool.mcpb\": {".dimmed());
                    println!(
                        "          {}",
                        "\"scripts\": { \"build\": \"...\" }".dimmed()
                    );
                    println!("        {}", "}".dimmed());
                    println!("      {}", "}".dimmed());
                }
                std::process::exit(1);
            }
            Err(ToolError::OAuthNotConfigured) | Err(ToolError::AuthRequired { tool_ref: _ }) => {
                println!(
                    "  {} This tool requires OAuth authentication\n",
                    "✗".bright_red()
                );
                println!(
                    "    To enable OAuth, set the {} environment variable:\n",
                    "CREDENTIALS_SECRET_KEY".bright_cyan()
                );
                println!("    {}  Generate a key:", "1.".dimmed());
                println!("       {}\n", "openssl rand -base64 32".bright_white());
                println!("    {}  Set it in your shell:", "2.".dimmed());
                println!(
                    "       {}\n",
                    "export CREDENTIALS_SECRET_KEY=\"<your-key>\"".bright_white()
                );
                println!(
                    "    {}  Re-run this command to start OAuth flow",
                    "3.".dimmed()
                );
                std::process::exit(1);
            }
            Err(e) => return Err(e),
        };

    let is_error = result.result.is_error.unwrap_or(false);

    // Concise output: just raw JSON
    if concise {
        for content in &result.result.content {
            match &**content {
                rmcp::model::RawContent::Text(text) => {
                    // Try to parse and re-serialize as minified JSON
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text.text) {
                        println!(
                            "{}",
                            serde_json::to_string(&json).unwrap_or(text.text.clone())
                        );
                    } else {
                        println!("{}", text.text);
                    }
                }
                rmcp::model::RawContent::Image(img) => {
                    println!("{{\"type\":\"image\",\"bytes\":{}}}", img.data.len());
                }
                rmcp::model::RawContent::Audio(audio) => {
                    println!("{{\"type\":\"audio\",\"bytes\":{}}}", audio.data.len());
                }
                rmcp::model::RawContent::Resource(res) => {
                    println!("{{\"type\":\"resource\",\"resource\":{:?}}}", res.resource);
                }
                rmcp::model::RawContent::ResourceLink(link) => {
                    println!("{{\"type\":\"resource_link\",\"uri\":\"{}\"}}", link.uri);
                }
            }
        }
        if is_error {
            std::process::exit(1);
        }
        return Ok(());
    }

    // Print header matching rad tool format
    if is_error {
        println!(
            "  {} {} {} on {}",
            "✗".bright_red(),
            "Error calling".bright_red(),
            method.bold(),
            tool_name.bold()
        );
    } else {
        println!(
            "  {} Called {} on {}\n",
            "✓".bright_green(),
            method.bold(),
            tool_name.bold()
        );
    }

    // Output the result content
    for content in &result.result.content {
        // Content is wrapped in Annotated, so we dereference to get the inner RawContent
        match &**content {
            rmcp::model::RawContent::Text(text) => {
                // Try to parse as JSON for pretty printing
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text.text) {
                    let pretty = serde_json::to_string_pretty(&json).unwrap_or(text.text.clone());
                    for line in pretty.lines() {
                        if is_error {
                            println!("    {}", line.bright_red());
                        } else {
                            println!("    {}", line);
                        }
                    }
                } else {
                    // Plain text output
                    for line in text.text.lines() {
                        if is_error {
                            println!("    {}", line.bright_red());
                        } else {
                            println!("    {}", line);
                        }
                    }
                }
            }
            rmcp::model::RawContent::Image(img) => {
                println!("    [Image: {} bytes]", img.data.len());
            }
            rmcp::model::RawContent::Audio(audio) => {
                println!("    [Audio: {} bytes]", audio.data.len());
            }
            rmcp::model::RawContent::Resource(res) => {
                println!("    [Resource: {:?}]", res.resource);
            }
            rmcp::model::RawContent::ResourceLink(link) => {
                println!("    [ResourceLink: {}]", link.uri);
            }
        }
    }

    if is_error {
        std::process::exit(1);
    }

    Ok(())
}

/// Parse user config from -c flags and config file.
///
/// Resolution order (later overrides earlier):
/// 1. Saved config from `~/.tool/config/...` (lowest priority)
/// 2. Config file (`--config-file`)
/// 3. CLI flags (`-C`) (highest priority)
pub(super) fn parse_user_config(
    config_flags: &[String],
    config_file: Option<&str>,
    tool_ref: Option<&crate::references::PluginRef>,
) -> ToolResult<BTreeMap<String, String>> {
    let mut config = BTreeMap::new();

    // 1. Load saved config (lowest priority)
    if let Some(ref_) = tool_ref
        && let Ok(saved) = load_tool_config(ref_)
    {
        config.extend(saved);
    }

    // 2. Load from config file
    if let Some(file_path) = config_file {
        let content = std::fs::read_to_string(file_path)?;
        let file_config: BTreeMap<String, String> = serde_json::from_str(&content)
            .or_else(|_| toml::from_str(&content))
            .map_err(|e| ToolError::Generic(format!("Failed to parse config file: {}", e)))?;
        config.extend(file_config);
    }

    // 3. Parse -C flags (highest priority)
    for flag in config_flags {
        if let Some((key, value)) = flag.split_once('=') {
            config.insert(key.to_string(), value.to_string());
        } else {
            return Err(ToolError::Generic(format!(
                "Invalid config format '{}'. Expected key=value",
                flag
            )));
        }
    }

    Ok(config)
}

/// Apply default values from user_config schema.
///
/// For any field in the schema that has a `default` value and isn't already
/// provided in user_config, applies the default. This ensures variable
/// substitution works even when users don't explicitly provide values.
pub(super) fn apply_user_config_defaults(
    schema: Option<&BTreeMap<String, McpbUserConfigField>>,
    user_config: &mut BTreeMap<String, String>,
) {
    let Some(schema) = schema else {
        return;
    };

    for (key, field) in schema {
        // Skip if already provided
        if user_config.contains_key(key) {
            continue;
        }

        // Apply default if present
        if let Some(default) = &field.default {
            let value = match default {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => default.to_string(),
            };
            user_config.insert(key.clone(), value);
        }
    }
}

/// Prompt for user_config values interactively.
///
/// Prompts for all config fields except those that have defaults and aren't required
/// (those are auto-applied by `apply_user_config_defaults`).
pub(super) fn prompt_missing_user_config(
    schema: Option<&BTreeMap<String, McpbUserConfigField>>,
    user_config: &mut BTreeMap<String, String>,
) -> ToolResult<()> {
    use std::io::IsTerminal;

    let Some(schema) = schema else {
        return Ok(());
    };

    // Find fields that need prompting:
    // - Already provided via --config: skip
    // - Has default AND not required: skip (auto-applied later)
    // - Otherwise: prompt
    let to_prompt: Vec<(&String, &McpbUserConfigField)> = schema
        .iter()
        .filter(|(key, field)| {
            // Skip if already provided
            if user_config.contains_key(*key) {
                return false;
            }

            let is_required = field.required.unwrap_or(false);
            let has_default = field.default.is_some();

            // Skip if has default and not required (will be auto-applied)
            if has_default && !is_required {
                return false;
            }

            // Prompt for: required fields OR fields without defaults
            true
        })
        .collect();

    if to_prompt.is_empty() {
        return Ok(());
    }

    // Check if we have a TTY for interactive prompting
    if !std::io::stdin().is_terminal() {
        // Non-interactive: only error for required fields without defaults
        let required_missing: Vec<String> = to_prompt
            .iter()
            .filter(|(_, field)| {
                let is_required = field.required.unwrap_or(false);
                let has_default = field.default.is_some();
                is_required && !has_default
            })
            .map(|(key, field)| {
                let desc = field.description.as_deref().unwrap_or("");
                if desc.is_empty() {
                    format!("  --config {}=<value>", key)
                } else {
                    format!("  --config {}=<value>  ({})", key, desc)
                }
            })
            .collect();

        if !required_missing.is_empty() {
            return Err(ToolError::Generic(format!(
                "Missing required configuration:\n\n{}\n\nProvide via --config flags or run interactively.",
                required_missing.join("\n")
            )));
        }
        return Ok(());
    }

    // Interactive: prompt for each field
    cliclack::intro("Tool configuration")?;

    for (key, field) in to_prompt {
        let is_required = field.required.unwrap_or(false);

        // Get description
        let description = field.description.as_deref().unwrap_or("");

        // Default can be number, string, or bool - convert to string
        let default_value = field.default.as_ref().map(|d| match d {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => d.to_string(),
        });

        // Build prompt text
        let prompt_text = if description.is_empty() {
            key.clone()
        } else {
            format!("{} ({})", key, description)
        };

        // Get user input using cliclack
        let value: String = match default_value {
            Some(default) => cliclack::input(&prompt_text)
                .default_input(&default)
                .interact()?,
            None => cliclack::input(&prompt_text)
                .required(is_required)
                .interact()?,
        };

        // Only insert non-empty values (skip optional fields left blank)
        if !value.is_empty() {
            user_config.insert(key.clone(), value);
        }
    }

    cliclack::outro("Configuration complete!")?;

    Ok(())
}

/// Parse method parameters from command line.
fn parse_method_params(params: &[String]) -> ToolResult<BTreeMap<String, serde_json::Value>> {
    let mut result = BTreeMap::new();

    for param in params {
        if let Some((key, value)) = param.split_once('=') {
            // Try to parse as JSON, otherwise treat as string
            let json_value = serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
            result.insert(key.to_string(), json_value);
        } else {
            return Err(ToolError::Generic(format!(
                "Invalid parameter format '{}'. Expected key=value",
                param
            )));
        }
    }

    Ok(result)
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_method_shorthand() {
        // Basic shorthand expansion
        assert_eq!(expand_method_shorthand(".exec", "bash"), "bash__exec");
        assert_eq!(
            expand_method_shorthand(".read", "filesystem"),
            "filesystem__read"
        );

        // Nested shorthand expansion
        assert_eq!(
            expand_method_shorthand(".fs.read", "files"),
            "files__fs__read"
        );
        assert_eq!(expand_method_shorthand(".a.b.c", "tool"), "tool__a__b__c");

        // Full method names pass through unchanged
        assert_eq!(expand_method_shorthand("bash__exec", "bash"), "bash__exec");
        assert_eq!(
            expand_method_shorthand("custom_method", "bash"),
            "custom_method"
        );

        // Methods without prefix pass through unchanged
        assert_eq!(expand_method_shorthand("exec", "bash"), "exec");
    }

    #[test]
    fn test_extract_tool_name_for_expansion() {
        // Simple tool name
        assert_eq!(extract_tool_name_for_expansion("bash"), "bash");
        assert_eq!(extract_tool_name_for_expansion("filesystem"), "filesystem");

        // Namespaced tool
        assert_eq!(
            extract_tool_name_for_expansion("appcypher/filesystem"),
            "filesystem"
        );
        assert_eq!(
            extract_tool_name_for_expansion("org/tool-name"),
            "tool-name"
        );

        // Versioned tool
        assert_eq!(extract_tool_name_for_expansion("bash@1.0.0"), "bash");
        assert_eq!(
            extract_tool_name_for_expansion("filesystem@0.2.1"),
            "filesystem"
        );

        // Namespaced and versioned
        assert_eq!(
            extract_tool_name_for_expansion("appcypher/filesystem@1.0.0"),
            "filesystem"
        );
        assert_eq!(
            extract_tool_name_for_expansion("org/my-tool@2.0.0-beta"),
            "my-tool"
        );
    }
}
