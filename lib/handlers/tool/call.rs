//! Tool call command handlers.

use crate::error::{ToolError, ToolResult};
use crate::mcp::call_tool;
use crate::mcpb::McpbUserConfigField;
use crate::styles::Spinner;
use crate::suggest::{
    McpErrorKind, analyze_mcp_error, extract_params_from_schema, find_similar_tools,
    format_suggestions, is_missing_param_error, is_unknown_tool_error,
};
use colored::Colorize;
use std::collections::BTreeMap;

use super::common::{PrepareToolOptions, PreparedTool, prepare_tool};
use super::config_cmd::{load_tool_config, tool_config_exists};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Get available tool names from a prepared tool's manifest.
///
/// First tries static_responses (full tool schemas), then falls back to
/// top-level tools list.
fn get_available_tools(prepared: &PreparedTool) -> Vec<String> {
    // Try static_responses first (more complete)
    if let Some(static_resp) = prepared.plugin.template.static_responses()
        && let Some(tools_list) = static_resp.tools_list
    {
        return tools_list.tools.iter().map(|t| t.name.clone()).collect();
    }

    // Fall back to top-level tools
    if let Some(ref tools) = prepared.plugin.template.tools {
        return tools.iter().map(|t| t.name.clone()).collect();
    }

    Vec::new()
}

/// Get the input schema for a specific tool from static_responses.
fn get_tool_input_schema(prepared: &PreparedTool, tool_name: &str) -> Option<serde_json::Value> {
    let static_resp = prepared.plugin.template.static_responses()?;
    let tools_list = static_resp.tools_list?;

    tools_list
        .tools
        .iter()
        .find(|t| t.name == tool_name)
        .and_then(|t| t.input_schema.clone())
}

/// Print enhanced error message for missing required parameters (typed version).
///
/// Shows all required and optional parameters from the tool's schema.
fn print_missing_param_error_typed(missing_param: &str, method: &str, prepared: &PreparedTool) {
    println!(
        "  {} Missing required parameter: {} for {} on {}\n",
        "✗".bright_red(),
        missing_param.bright_white(),
        method.bold(),
        prepared.tool_name.bold()
    );

    print_tool_params(method, prepared);
}

/// Print all parameters for a tool method from its schema.
fn print_tool_params(method: &str, prepared: &PreparedTool) {
    // Get the tool's input schema and show all parameters
    if let Some(schema) = get_tool_input_schema(prepared, method) {
        let params = extract_params_from_schema(&schema);

        if !params.is_empty() {
            let required_params: Vec<_> = params.iter().filter(|p| p.required).collect();
            let optional_params: Vec<_> = params.iter().filter(|p| !p.required).collect();

            if !required_params.is_empty() {
                println!("  {}:", "Required".dimmed());
                for param in &required_params {
                    let desc = param
                        .description
                        .as_ref()
                        .map(|d| format!(" - {}", d.dimmed()))
                        .unwrap_or_default();
                    println!(
                        "  · {} ({}){}",
                        param.name.bright_cyan(),
                        param.param_type.dimmed(),
                        desc
                    );
                }
            }

            if !optional_params.is_empty() {
                if !required_params.is_empty() {
                    println!();
                }
                println!("  {}:", "Optional".dimmed());
                for param in &optional_params {
                    let desc = param
                        .description
                        .as_ref()
                        .map(|d| format!(" - {}", d.dimmed()))
                        .unwrap_or_default();
                    println!(
                        "  · {} ({}){}",
                        param.name.dimmed(),
                        param.param_type.dimmed(),
                        desc
                    );
                }
            }
        }
    }
}

/// Print enhanced error message for unknown tool with fuzzy suggestions.
///
/// If static tools are not available in manifest, fetches from the live server.
async fn print_unknown_tool_error(unknown_tool: &str, prepared: &PreparedTool) {
    // Try to get tools from manifest first
    let mut available_tools = get_available_tools(prepared);

    // If manifest doesn't have tools, fetch from the live server
    if available_tools.is_empty()
        && let Ok(capabilities) =
            crate::mcp::get_tool_info(&prepared.resolved, &prepared.tool_name, false).await
    {
        available_tools = capabilities
            .tools
            .iter()
            .map(|t| t.name.to_string())
            .collect();
    }

    let suggestions = find_similar_tools(unknown_tool, &available_tools);

    println!(
        "  {} Tool {} not found on {}\n",
        "✗".bright_red(),
        format!("`{}`", unknown_tool).bright_white(),
        prepared.tool_name.bold()
    );

    if let Some(hint) = format_suggestions(&suggestions) {
        println!("  {} {}", "hint:".bright_cyan().bold(), hint);
    }

    // Show available tools if there are few
    if !available_tools.is_empty() && available_tools.len() <= 10 {
        println!();
        println!("  {}:", "Available tools".dimmed());
        for tool in &available_tools {
            println!("  · {}", tool.bright_cyan());
        }
    } else if !available_tools.is_empty() {
        println!(
            "  · Run {} to see available tools",
            format!("tool info {} --tools", prepared.tool_name).bright_cyan()
        );
    }
}

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
    no_save: bool,
    yes: bool,
    _verbose: bool,
    json_output: bool,
    concise: bool,
) -> ToolResult<()> {
    // Merge -p flags and trailing args
    let params: Vec<String> = param.into_iter().chain(args).collect();

    // Expand method shorthand (.exec → toolname__exec)
    let tool_name_for_expansion = extract_tool_name_for_expansion(&tool);
    let method = expand_method_shorthand(&method, tool_name_for_expansion);

    // Parse method parameters
    let arguments = parse_method_params(&params)?;

    // Prepare the tool (resolve, load config, prompt, save)
    let prepared = prepare_tool(
        &tool,
        PrepareToolOptions {
            config: &config,
            config_file: config_file.as_deref(),
            no_save,
            yes,
        },
    )
    .await?;

    // Show spinner while connecting (human-readable mode only)
    let show_spinner = !json_output && !concise;
    let spinner =
        show_spinner.then(|| Spinner::new(format!("Connecting to {}", prepared.tool_name)));

    // Call the tool - handle EntryPointNotFound specially
    // Never pass verbose to connection - verbose only affects output formatting
    let result = match call_tool(
        &prepared.resolved,
        &prepared.tool_name,
        &method,
        arguments,
        false,
    )
    .await
    {
        Ok(result) => {
            if let Some(s) = spinner {
                s.done();
            }
            result
        }
        Err(ToolError::EntryPointNotFound {
            entry_point,
            full_path: _,
            build_script,
            bundle_path,
        }) => {
            if let Some(s) = spinner {
                s.fail(None);
            }
            println!(
                "  {} Entry point not found: {}\n",
                "✗".bright_red(),
                entry_point.bright_white()
            );
            if let Some(build_cmd) = build_script {
                println!("  · The tool needs to be built before it can be run.\n");
                println!("  {}:", "To build".dimmed());
                println!("  · cd {} && tool build\n", bundle_path);
                println!("  · {}: {}", "Runs".dimmed(), build_cmd.dimmed());
            } else {
                println!("  {}:", "If this tool requires building".dimmed());
                println!("  · Add a build script to manifest.json:\n");
                println!("  · {}", "\"_meta\": {".dimmed());
                println!("  ·   {}", "\"store.tool.mcpb\": {".dimmed());
                println!("  ·     {}", "\"scripts\": { \"build\": \"...\" }".dimmed());
                println!("  ·   {}", "}".dimmed());
                println!("  · {}", "}".dimmed());
            }
            std::process::exit(1);
        }
        Err(ToolError::OAuthNotConfigured) | Err(ToolError::AuthRequired { tool_ref: _ }) => {
            if let Some(s) = spinner {
                s.fail(None);
            }
            println!("  {} OAuth authentication failed\n", "✗".bright_red());
            println!(
                "  · Could not initialize credential storage. Check that {} is writable.",
                "~/.tool/secrets/".bright_cyan()
            );
            std::process::exit(1);
        }
        Err(e) => {
            if let Some(s) = spinner {
                s.fail(None);
            }

            // Check for enhanced error handling with typed MCP errors
            if let ToolError::Mcp(ref service_error) = e
                && let Some(kind) = analyze_mcp_error(service_error)
            {
                match kind {
                    McpErrorKind::MissingParam(param) => {
                        print_missing_param_error_typed(&param, &method, &prepared);
                        std::process::exit(1);
                    }
                    McpErrorKind::UnknownTool(tool) => {
                        print_unknown_tool_error(&tool, &prepared).await;
                        std::process::exit(1);
                    }
                    McpErrorKind::Other { code, message } => {
                        // Fall through to default error display
                        // but we could add more specific handling here
                        println!("  {} MCP error ({}): {}\n", "✗".bright_red(), code, message);
                        std::process::exit(1);
                    }
                }
            }

            return Err(e);
        }
    };

    let is_error = result.result.is_error.unwrap_or(false);

    // Concise output: minified JSON (takes precedence over --json)
    if concise {
        // Prefer structuredContent if available
        if let Some(structured) = &result.result.structured_content {
            println!(
                "{}",
                serde_json::to_string(structured).unwrap_or_else(|_| structured.to_string())
            );
        } else {
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
        }
        if is_error {
            std::process::exit(1);
        }
        return Ok(());
    }

    // JSON output: structured content only, exit with null if absent
    if json_output {
        if let Some(structured) = &result.result.structured_content {
            println!(
                "{}",
                serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string())
            );
        } else {
            println!("null");
        }
        if is_error {
            std::process::exit(1);
        }
        return Ok(());
    }

    // Check for special error types in result content that we can enhance
    if is_error {
        // Extract text content to check for known error patterns
        let error_text: String = result
            .result
            .content
            .iter()
            .filter_map(|c| {
                if let rmcp::model::RawContent::Text(text) = &**c {
                    Some(text.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Handle missing required parameter errors with enhanced output
        if is_missing_param_error(&error_text) {
            // Try to extract the param name from the error text
            if let Some(param) = crate::suggest::extract_missing_field_from_message(&error_text) {
                print_missing_param_error_typed(&param, &method, &prepared);
            } else {
                // Fallback: show generic missing param message with params list
                println!(
                    "  {} Missing required parameter for {} on {}\n",
                    "✗".bright_red(),
                    method.bold(),
                    prepared.tool_name.bold()
                );
                print_tool_params(&method, &prepared);
            }
            std::process::exit(1);
        }

        // Handle unknown tool errors with suggestions from manifest
        if is_unknown_tool_error(&error_text) {
            print_unknown_tool_error(&method, &prepared).await;
            std::process::exit(1);
        }
    }

    // Print header matching rad tool format
    if is_error {
        println!(
            "  {} {} {} on {}",
            "✗".bright_red(),
            "Error calling".bright_red(),
            method.bold(),
            prepared.tool_name.bold()
        );
    } else {
        println!(
            "  {} Called {} on {}\n",
            "✓".bright_green(),
            method.bold(),
            prepared.tool_name.bold()
        );
    }

    // Output text content
    for content in &result.result.content {
        // Content is wrapped in Annotated, so we dereference to get the inner RawContent
        match &**content {
            rmcp::model::RawContent::Text(text) => {
                // Try to parse as JSON for pretty printing
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text.text) {
                    let pretty = serde_json::to_string_pretty(&json).unwrap_or(text.text.clone());
                    for line in pretty.lines() {
                        if is_error {
                            println!("  {}", line.bright_red());
                        } else {
                            println!("  {}", line);
                        }
                    }
                } else {
                    // Plain text output
                    for line in text.text.lines() {
                        if is_error {
                            println!("  {}", line.bright_red());
                        } else {
                            println!("  {}", line);
                        }
                    }
                }
            }
            rmcp::model::RawContent::Image(img) => {
                println!("  · [Image: {} bytes]", img.data.len());
            }
            rmcp::model::RawContent::Audio(audio) => {
                println!("  · [Audio: {} bytes]", audio.data.len());
            }
            rmcp::model::RawContent::Resource(res) => {
                println!("  · [Resource: {:?}]", res.resource);
            }
            rmcp::model::RawContent::ResourceLink(link) => {
                println!("  · [ResourceLink: {}]", link.uri);
            }
        }
    }

    // Output structured content if available
    if let Some(structured) = &result.result.structured_content {
        println!();
        let pretty =
            serde_json::to_string_pretty(structured).unwrap_or_else(|_| structured.to_string());
        for line in pretty.lines() {
            if is_error {
                println!("  {}", line.bright_red());
            } else {
                println!("  {}", line);
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
/// 3. CLI flags (`-k`) (highest priority)
///
/// Returns the merged config and whether a saved config file was found.
pub(super) fn parse_user_config(
    config_flags: &[String],
    config_file: Option<&str>,
    plugin_ref: &crate::references::PluginRef,
) -> ToolResult<(BTreeMap<String, String>, bool)> {
    let mut config = BTreeMap::new();

    // 1. Load saved config (lowest priority)
    let has_saved_config = tool_config_exists(plugin_ref);
    if let Ok(saved) = load_tool_config(plugin_ref) {
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

    Ok((config, has_saved_config))
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
/// On first use (no saved config), prompts for all fields so the user can
/// configure the tool. On subsequent runs (saved config exists), only prompts
/// for required fields without defaults that are still missing.
///
/// If `skip_interactive` is true, prompting is skipped and an error is returned
/// if any required fields are missing.
pub(super) fn prompt_missing_user_config(
    schema: Option<&BTreeMap<String, McpbUserConfigField>>,
    user_config: &mut BTreeMap<String, String>,
    skip_interactive: bool,
    has_saved_config: bool,
) -> ToolResult<()> {
    use std::io::IsTerminal;

    let Some(schema) = schema else {
        return Ok(());
    };

    // Determine which fields need prompting based on whether user has already configured.
    // - First time (no saved config): prompt all missing fields
    // - Already configured (saved config exists): only prompt required fields without defaults
    let to_prompt: Vec<(&String, &McpbUserConfigField)> = schema
        .iter()
        .filter(|(key, field)| {
            let is_missing = !user_config.contains_key(*key);
            if !is_missing {
                return false;
            }
            if has_saved_config {
                // Already configured: only prompt for required fields without defaults
                let is_required = field.required.unwrap_or(false);
                let has_default = field.default.is_some();
                is_required && !has_default
            } else {
                // First time: prompt for all missing fields
                true
            }
        })
        .collect();

    if to_prompt.is_empty() {
        return Ok(());
    }

    // Check if we should skip interactive prompting (--yes flag or no TTY)
    if skip_interactive || !std::io::stdin().is_terminal() {
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
                    format!("  -k {}=<value>", key)
                } else {
                    format!("  -k {}=<value>  ({})", key, desc)
                }
            })
            .collect();

        if !required_missing.is_empty() {
            return Err(ToolError::Generic(format!(
                "Missing required configuration:\n\n{}\n\nProvide via -k flags or run interactively (without --yes).",
                required_missing.join("\n")
            )));
        }
        return Ok(());
    }

    // Interactive: prompt for fields
    crate::prompt::init_theme();
    cliclack::intro("Tool configuration")?;

    for (key, field) in to_prompt {
        let is_required = field.required.unwrap_or(false);
        let is_sensitive = field.sensitive.unwrap_or(false);

        // Use title for display
        let display_name = &field.title;
        let description = field.description.as_deref().unwrap_or("");

        // Default can be number, string, or bool - convert to string
        // Don't show defaults for sensitive fields
        let default_value = if is_sensitive {
            None
        } else {
            field.default.as_ref().map(|d| match d {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => d.to_string(),
            })
        };

        // Build prompt text
        let prompt_text = if description.is_empty() {
            display_name.clone()
        } else {
            format!("{} ({})", display_name, description)
        };

        // Get user input using cliclack - use password prompt for sensitive fields
        let value: String = if is_sensitive {
            let mut password = cliclack::password(&prompt_text);
            if !is_required {
                password = password.allow_empty();
            }
            password.interact()?
        } else {
            match default_value {
                Some(default) => cliclack::input(&prompt_text)
                    .default_input(&default)
                    .interact()?,
                None => cliclack::input(&prompt_text)
                    .required(is_required)
                    .interact()?,
            }
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
