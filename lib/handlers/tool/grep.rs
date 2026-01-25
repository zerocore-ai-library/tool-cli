//! Tool grep command handlers.
//!
//! Searches the unified JSON structure from `tool list --json --full`
//! and returns matches with JavaScript accessor paths.

use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcp::get_tool_info;
use crate::output::{
    GrepOutput, js_path_schema_field, js_path_schema_field_prop, js_path_server,
    js_path_server_prop, js_path_tool, js_path_tool_prop,
};
use crate::resolver::{FilePluginResolver, load_tool_from_path};
use crate::system_config::allocate_system_config;
use colored::Colorize;
use std::collections::BTreeMap;

use super::list::resolve_tool_path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Internal grep match with path and value.
struct GrepMatch {
    /// JavaScript accessor path (e.g., "['server/name'].tools.tool_name")
    path: String,
    /// The matched value
    value: String,
    /// The server name (for grouping)
    server: String,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Search tool schemas by pattern.
#[allow(clippy::too_many_arguments)]
pub async fn grep_tool(
    pattern: &str,
    tool: Option<String>,
    method: Option<String>,
    input_only: bool,
    output_only: bool,
    name_only: bool,
    description_only: bool,
    ignore_case: bool,
    list_only: bool,
    json_output: bool,
    concise: bool,
    no_header: bool,
    _level: usize,
) -> ToolResult<()> {
    use regex::RegexBuilder;

    // Build regex
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| ToolError::Generic(format!("Invalid regex pattern: {}", e)))?;

    // Focus flags determine what to search
    // If none specified, search all
    let schema_all = !input_only && !output_only;
    let field_all = !name_only && !description_only;

    // Collect matches
    let mut all_matches: Vec<GrepMatch> = Vec::new();

    // Get list of tools to search
    let tools_to_search = match &tool {
        Some(t) => {
            // Single tool specified
            let resolved = resolve_tool_path(t).await?;
            vec![(t.clone(), resolved.path)]
        }
        None => {
            // Search all installed tools
            let resolver = FilePluginResolver::default();
            let tool_refs = resolver.list_tools().await?;
            let mut tools = Vec::new();
            for plugin_ref in tool_refs {
                let name = plugin_ref.to_string();
                if let Ok(Some(resolved)) = resolver.resolve_tool(&name).await {
                    let dir = resolved
                        .path
                        .parent()
                        .unwrap_or(&resolved.path)
                        .to_path_buf();
                    tools.push((name, dir));
                }
            }
            tools
        }
    };

    if tools_to_search.is_empty() {
        if !concise {
            println!("  {} No tools installed", "✗".bright_red());
        }
        return Ok(());
    }

    // Search each tool
    for (tool_ref, tool_path) in &tools_to_search {
        // Load manifest and resolve config
        let resolved_plugin = match load_tool_from_path(tool_path) {
            Ok(p) => p,
            Err(_) => continue, // Skip tools that can't be loaded
        };

        let user_config = BTreeMap::new();
        let system_config =
            match allocate_system_config(resolved_plugin.template.system_config.as_ref()) {
                Ok(c) => c,
                Err(_) => continue,
            };

        let resolved = match resolved_plugin
            .template
            .resolve(&user_config, &system_config)
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let tool_name = resolved_plugin.template.name.clone().unwrap_or_else(|| {
            tool_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        });

        // Get tool info
        let capabilities = match get_tool_info(&resolved, &tool_name, false).await {
            Ok(result) => result,
            Err(_) => continue, // Skip tools that can't be connected
        };

        // Extract server name from tool_ref (e.g., "appcypher/filesystem@1.0" -> "appcypher/filesystem")
        let server_name = tool_ref.split('@').next().unwrap_or(tool_ref);

        // Server-level searches only happen if not filtering by --input/--output
        // (servers don't have input/output schemas)
        if schema_all {
            // Search server name (key match)
            if (field_all || name_only) && regex.is_match(server_name) {
                all_matches.push(GrepMatch {
                    path: js_path_server(server_name),
                    value: server_name.to_string(),
                    server: server_name.to_string(),
                });
            }

            // Search server description (value match)
            if (field_all || description_only)
                && let Some(desc) = &resolved_plugin.template.description
                && regex.is_match(desc)
            {
                all_matches.push(GrepMatch {
                    path: js_path_server_prop(server_name, "description"),
                    value: desc.to_string(),
                    server: server_name.to_string(),
                });
            }
        }

        // Search each tool in this server
        for tool_info in &capabilities.tools {
            let mcp_tool_name = &tool_info.name;

            // If -m specified, skip tools that don't match
            if let Some(ref method_filter) = method
                && mcp_tool_name != method_filter
            {
                continue;
            }

            // Tool-level searches only happen if not filtering by --input/--output
            if schema_all {
                // Search tool name (key match)
                if (field_all || name_only) && regex.is_match(mcp_tool_name) {
                    all_matches.push(GrepMatch {
                        path: js_path_tool(server_name, mcp_tool_name),
                        value: mcp_tool_name.to_string(),
                        server: server_name.to_string(),
                    });
                }

                // Search tool description (value match)
                if (field_all || description_only)
                    && let Some(desc) = &tool_info.description
                    && regex.is_match(desc)
                {
                    all_matches.push(GrepMatch {
                        path: js_path_tool_prop(server_name, mcp_tool_name, "description"),
                        value: desc.to_string(),
                        server: server_name.to_string(),
                    });
                }
            }

            // Search input schema properties
            if (schema_all || input_only)
                && let Some(props) = tool_info
                    .input_schema
                    .get("properties")
                    .and_then(|p| p.as_object())
            {
                for (field_name, field_schema) in props {
                    // Search field name (key match)
                    if (field_all || name_only) && regex.is_match(field_name) {
                        all_matches.push(GrepMatch {
                            path: js_path_schema_field(
                                server_name,
                                mcp_tool_name,
                                "input_schema",
                                field_name,
                            ),
                            value: field_name.clone(),
                            server: server_name.to_string(),
                        });
                    }

                    // Search field description (value match)
                    if (field_all || description_only)
                        && let Some(desc) = field_schema.get("description").and_then(|d| d.as_str())
                        && regex.is_match(desc)
                    {
                        all_matches.push(GrepMatch {
                            path: js_path_schema_field_prop(
                                server_name,
                                mcp_tool_name,
                                "input_schema",
                                field_name,
                                "description",
                            ),
                            value: desc.to_string(),
                            server: server_name.to_string(),
                        });
                    }

                    // Search field type (value match) - only if searching all fields
                    if field_all
                        && let Some(type_str) = field_schema.get("type").and_then(|t| t.as_str())
                        && regex.is_match(type_str)
                    {
                        all_matches.push(GrepMatch {
                            path: js_path_schema_field_prop(
                                server_name,
                                mcp_tool_name,
                                "input_schema",
                                field_name,
                                "type",
                            ),
                            value: type_str.to_string(),
                            server: server_name.to_string(),
                        });
                    }
                }
            }

            // Search output schema properties
            if (schema_all || output_only)
                && let Some(output_schema) = &tool_info.output_schema
                && let Some(props) = output_schema.get("properties").and_then(|p| p.as_object())
            {
                for (field_name, field_schema) in props {
                    // Search field name (key match)
                    if (field_all || name_only) && regex.is_match(field_name) {
                        all_matches.push(GrepMatch {
                            path: js_path_schema_field(
                                server_name,
                                mcp_tool_name,
                                "output_schema",
                                field_name,
                            ),
                            value: field_name.clone(),
                            server: server_name.to_string(),
                        });
                    }

                    // Search field description (value match)
                    if (field_all || description_only)
                        && let Some(desc) = field_schema.get("description").and_then(|d| d.as_str())
                        && regex.is_match(desc)
                    {
                        all_matches.push(GrepMatch {
                            path: js_path_schema_field_prop(
                                server_name,
                                mcp_tool_name,
                                "output_schema",
                                field_name,
                                "description",
                            ),
                            value: desc.to_string(),
                            server: server_name.to_string(),
                        });
                    }

                    // Search field type (value match) - only if searching all fields
                    if field_all
                        && let Some(type_str) = field_schema.get("type").and_then(|t| t.as_str())
                        && regex.is_match(type_str)
                    {
                        all_matches.push(GrepMatch {
                            path: js_path_schema_field_prop(
                                server_name,
                                mcp_tool_name,
                                "output_schema",
                                field_name,
                                "type",
                            ),
                            value: type_str.to_string(),
                            server: server_name.to_string(),
                        });
                    }
                }
            }
        }
    }

    if all_matches.is_empty() {
        if !concise {
            println!(
                "  {} No matches found for pattern: {}",
                "✗".bright_red(),
                pattern.bright_white().bold()
            );
        }
        return Ok(());
    }

    // Output results
    if json_output {
        output_json(pattern, &all_matches, concise);
        return Ok(());
    }

    // -l mode: show unique paths only
    if list_only {
        output_list_only(&all_matches, concise, no_header);
        return Ok(());
    }

    // Concise mode: TSV format
    if concise {
        output_concise(&all_matches, no_header);
        return Ok(());
    }

    // Normal human-readable output
    output_normal(pattern, &all_matches);

    Ok(())
}

/// Output grep results as JSON using GrepOutput structure.
fn output_json(pattern: &str, matches: &[GrepMatch], concise: bool) {
    let mut output = GrepOutput::new(pattern);
    for m in matches {
        output.add_match(&m.path, &m.value);
    }
    if concise {
        println!("{}", output.to_json().expect("Failed to serialize JSON"));
    } else {
        println!(
            "{}",
            output.to_json_pretty().expect("Failed to serialize JSON")
        );
    }
}

/// Output unique paths only (-l mode).
fn output_list_only(matches: &[GrepMatch], concise: bool, no_header: bool) {
    // Collect unique paths
    let mut paths: Vec<&str> = matches.iter().map(|m| m.path.as_str()).collect();
    paths.sort();
    paths.dedup();

    if concise && !no_header {
        println!("#path");
    }

    for path in paths {
        if concise {
            println!("{}", path);
        } else {
            println!("{}", path.bright_cyan());
        }
    }
}

/// Output grep results in concise TSV format.
fn output_concise(matches: &[GrepMatch], no_header: bool) {
    use crate::concise::quote;

    if !no_header {
        println!("#path\tvalue");
    }
    for m in matches {
        println!("{}\t{}", m.path, quote(&m.value));
    }
}

/// Output grep results in human-readable format with hierarchical grouping.
fn output_normal(pattern: &str, matches: &[GrepMatch]) {
    let match_count = matches.len();
    let label = if match_count == 1 { "match" } else { "matches" };
    println!(
        "  {} Found {} {} for pattern: {}\n",
        "✓".bright_green(),
        match_count.to_string().bold(),
        label,
        pattern.bright_white().bold()
    );

    // Parse each match into (server, parent_path, leaf, value)
    // parent_path is the entity being matched (server, tool, or field)
    // leaf is either "[key]" or ".property"
    struct ParsedMatch<'a> {
        server: &'a str,
        parent_path: String, // relative to server, e.g., "" or ".tools.tool_name"
        leaf: String,        // "[key]" or ".description"
        value: &'a str,
    }

    let mut parsed: Vec<ParsedMatch> = Vec::new();
    for m in matches {
        let server = &m.server;
        let relative_path = m
            .path
            .strip_prefix(&format!("['{}']", server))
            .unwrap_or(&m.path);

        // Determine if this is a key match or value match
        // Value matches end with known properties: .description, .type
        let (parent_path, leaf) = if relative_path.is_empty() {
            // Server key match
            (String::new(), "[key]".to_string())
        } else if let Some(idx) = relative_path.rfind(".description") {
            if relative_path.ends_with(".description") {
                (relative_path[..idx].to_string(), ".description".to_string())
            } else {
                // Key match (path doesn't end with a property)
                (relative_path.to_string(), "[key]".to_string())
            }
        } else if let Some(idx) = relative_path.rfind(".type") {
            if relative_path.ends_with(".type") {
                (relative_path[..idx].to_string(), ".type".to_string())
            } else {
                (relative_path.to_string(), "[key]".to_string())
            }
        } else {
            // Key match (tool name, field name, etc.)
            (relative_path.to_string(), "[key]".to_string())
        };

        parsed.push(ParsedMatch {
            server,
            parent_path,
            leaf,
            value: &m.value,
        });
    }

    // Group by server, then by parent_path
    let mut by_server: BTreeMap<&str, BTreeMap<&str, Vec<&ParsedMatch>>> = BTreeMap::new();
    for pm in &parsed {
        by_server
            .entry(pm.server)
            .or_default()
            .entry(&pm.parent_path)
            .or_default()
            .push(pm);
    }

    for (server, by_parent) in &by_server {
        println!("  {}", server.bright_cyan().bold());

        for (parent_path, group) in by_parent {
            // Print parent path (if not empty)
            if !parent_path.is_empty() {
                println!("    {}", parent_path.bright_white());
            }

            // Print each leaf under this parent
            for pm in group {
                let display_value =
                    format_description(pm.value, false, "").unwrap_or_else(|| pm.value.to_string());

                let indent = if parent_path.is_empty() {
                    "    "
                } else {
                    "      "
                };

                if pm.leaf == "[key]" {
                    println!("{}{}", indent, "[key]".dimmed());
                    println!("{}  \"{}\"", indent, display_value.dimmed());
                } else {
                    println!("{}{}", indent, pm.leaf.bright_white());
                    println!("{}  \"{}\"", indent, display_value.dimmed());
                }
            }
        }
        println!();
    }
}
