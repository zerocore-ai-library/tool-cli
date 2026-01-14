//! Tool grep command handlers.

use crate::error::{ToolError, ToolResult};
use crate::mcp::get_tool_info_from_path;
use crate::resolver::FilePluginResolver;
use colored::Colorize;
use std::collections::BTreeMap;

use super::info::format_schema_params_concise;
use super::list::resolve_tool_path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A grep match result.
struct GrepMatch {
    toolset: String,
    tool_name: String,
    field_type: String, // "name", "desc", "in", "out"
    field_name: String, // field name or empty for desc
    matched_text: String,
}

/// Tool info for signature generation in -l mode.
struct ToolSignatureInfo {
    toolset: String,
    tool_name: String,
    input_schema: std::sync::Arc<serde_json::Map<String, serde_json::Value>>,
    output_schema: Option<std::sync::Arc<serde_json::Map<String, serde_json::Value>>>,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Search tool schemas by pattern.
#[allow(clippy::too_many_arguments)]
pub async fn grep_tool(
    pattern: &str,
    tool: Option<String>,
    name_only: bool,
    description_only: bool,
    params_only: bool,
    ignore_case: bool,
    list_only: bool,
    json_output: bool,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    use regex::RegexBuilder;

    // Build regex
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| ToolError::Generic(format!("Invalid regex pattern: {}", e)))?;

    // Determine search scope - all search types if none specified
    let search_all = !name_only && !description_only && !params_only;

    // Collect matches and tool signatures (for -l mode)
    let mut all_matches: Vec<GrepMatch> = Vec::new();
    let mut tool_signatures: Vec<ToolSignatureInfo> = Vec::new();

    // Get list of tools to search
    let tools_to_search = match &tool {
        Some(t) => {
            // Single tool specified
            let tool_path = resolve_tool_path(t).await?;
            vec![(t.clone(), tool_path)]
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
        // Get tool info
        let user_config = BTreeMap::new();
        let (capabilities, _tool_type, _manifest_path) =
            match get_tool_info_from_path(tool_path, &user_config, false).await {
                Ok(result) => result,
                Err(_) => continue, // Skip tools that can't be connected
            };

        // Extract toolset name from tool_ref (e.g., "appcypher/filesystem" -> "appcypher/filesystem")
        let toolset = tool_ref.split('@').next().unwrap_or(tool_ref);

        // Search each tool in this toolset
        for tool_info in &capabilities.tools {
            let tool_name = &tool_info.name;

            // Search tool name
            if (search_all || name_only) && regex.is_match(tool_name) {
                all_matches.push(GrepMatch {
                    toolset: toolset.to_string(),
                    tool_name: tool_name.to_string(),
                    field_type: "name".to_string(),
                    field_name: tool_name.to_string(),
                    matched_text: tool_name.to_string(),
                });
            }

            // Search description
            if (search_all || description_only)
                && let Some(desc) = &tool_info.description
                && regex.is_match(desc)
            {
                all_matches.push(GrepMatch {
                    toolset: toolset.to_string(),
                    tool_name: tool_name.to_string(),
                    field_type: "desc".to_string(),
                    field_name: String::new(),
                    matched_text: desc.to_string(),
                });
            }

            // Search input parameters
            if (search_all || params_only)
                && let Some(props) = tool_info
                    .input_schema
                    .get("properties")
                    .and_then(|p| p.as_object())
            {
                for (param_name, param_schema) in props {
                    // Search param name
                    if regex.is_match(param_name) {
                        let desc = param_schema
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        all_matches.push(GrepMatch {
                            toolset: toolset.to_string(),
                            tool_name: tool_name.to_string(),
                            field_type: "in".to_string(),
                            field_name: param_name.clone(),
                            matched_text: desc.to_string(),
                        });
                    }
                    // Also search param description
                    if let Some(desc) = param_schema.get("description").and_then(|d| d.as_str())
                        && regex.is_match(desc)
                    {
                        // Avoid duplicate if param name already matched
                        let tool_name_str = tool_name.to_string();
                        let already_matched = all_matches.iter().any(|m| {
                            m.toolset == toolset
                                && m.tool_name == tool_name_str
                                && m.field_type == "in"
                                && m.field_name == *param_name
                        });
                        if !already_matched {
                            all_matches.push(GrepMatch {
                                toolset: toolset.to_string(),
                                tool_name: tool_name.to_string(),
                                field_type: "in".to_string(),
                                field_name: param_name.clone(),
                                matched_text: desc.to_string(),
                            });
                        }
                    }
                }
            }

            // Search output schema parameters
            if (search_all || params_only)
                && let Some(output_schema) = &tool_info.output_schema
                && let Some(props) = output_schema.get("properties").and_then(|p| p.as_object())
            {
                for (param_name, param_schema) in props {
                    if regex.is_match(param_name) {
                        let desc = param_schema
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        all_matches.push(GrepMatch {
                            toolset: toolset.to_string(),
                            tool_name: tool_name.to_string(),
                            field_type: "out".to_string(),
                            field_name: param_name.clone(),
                            matched_text: desc.to_string(),
                        });
                    }
                }
            }

            // Store tool signature info for -l mode (if this tool has any matches)
            let tool_key = format!("{}:{}", toolset, tool_name);
            let has_match = all_matches
                .iter()
                .any(|m| m.toolset == toolset && m.tool_name == *tool_name);
            if has_match
                && !tool_signatures
                    .iter()
                    .any(|s| format!("{}:{}", s.toolset, s.tool_name) == tool_key)
            {
                tool_signatures.push(ToolSignatureInfo {
                    toolset: toolset.to_string(),
                    tool_name: tool_name.to_string(),
                    input_schema: tool_info.input_schema.clone(),
                    output_schema: tool_info.output_schema.clone(),
                });
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
        output_json(&all_matches, concise);
        return Ok(());
    }

    // -l mode: show function signatures
    if list_only {
        output_list_only(&tool_signatures, concise, no_header);
        return Ok(());
    }

    // Concise mode: grouped by toolset with TSV format
    if concise {
        output_concise(&all_matches, no_header);
        return Ok(());
    }

    // Normal output: Design B - grouped by tool with symbols
    output_normal(&all_matches);

    Ok(())
}

/// Output grep results as JSON.
fn output_json(all_matches: &[GrepMatch], concise: bool) {
    let output: Vec<_> = all_matches
        .iter()
        .map(|m| {
            serde_json::json!({
                "toolset": m.toolset,
                "tool": m.tool_name,
                "type": m.field_type,
                "field": if m.field_name.is_empty() { None } else { Some(&m.field_name) },
                "text": m.matched_text,
            })
        })
        .collect();
    if concise {
        println!(
            "{}",
            serde_json::to_string(&output).expect("Failed to serialize JSON")
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&output).expect("Failed to serialize JSON")
        );
    }
}

/// Output grep results in list-only mode (function signatures).
fn output_list_only(tool_signatures: &[ToolSignatureInfo], concise: bool, no_header: bool) {
    if concise && !no_header {
        println!("#tool");
    }
    for sig in tool_signatures {
        let params = format_schema_params_concise(&sig.input_schema, true);
        let outputs = sig
            .output_schema
            .as_ref()
            .map(|s| format_schema_params_concise(s, false))
            .unwrap_or_default();

        let signature = if outputs.is_empty() {
            format!("{}:{}({})", sig.toolset, sig.tool_name, params)
        } else {
            format!(
                "{}:{}({}) -> {{{}}}",
                sig.toolset, sig.tool_name, params, outputs
            )
        };

        if concise {
            println!("{}", signature);
        } else {
            println!("{}", signature.bright_cyan());
        }
    }
}

/// Output grep results in concise TSV format.
fn output_concise(all_matches: &[GrepMatch], no_header: bool) {
    use crate::concise::quote;
    use std::collections::BTreeMap;

    // Group matches by toolset
    let mut by_toolset: BTreeMap<String, Vec<&GrepMatch>> = BTreeMap::new();
    for m in all_matches {
        // Skip name matches - tool name is already visible, redundant
        if m.field_type == "name" {
            continue;
        }
        by_toolset.entry(m.toolset.clone()).or_default().push(m);
    }

    // Output each toolset group
    for (toolset, matches) in &by_toolset {
        if !no_header {
            println!("#toolset\t{}", toolset);
            println!("#tool\ttype\tfield\ttext");
        }
        for m in matches {
            let field = if m.field_name.is_empty() {
                "-"
            } else {
                &m.field_name
            };
            println!(
                "{}\t{}\t{}\t{}",
                m.tool_name,
                m.field_type,
                field,
                quote(&m.matched_text)
            );
        }
    }
}

/// Output grep results in normal human-readable format.
fn output_normal(all_matches: &[GrepMatch]) {
    use std::collections::BTreeMap;

    // Group matches by toolset, then by tool_name
    let mut by_toolset: BTreeMap<String, BTreeMap<String, Vec<&GrepMatch>>> = BTreeMap::new();
    for m in all_matches {
        by_toolset
            .entry(m.toolset.clone())
            .or_default()
            .entry(m.tool_name.clone())
            .or_default()
            .push(m);
    }

    let match_count = all_matches.len();
    let label = if match_count == 1 { "match" } else { "matches" };

    // Print header with count per toolset
    for (toolset, tools) in &by_toolset {
        let toolset_matches: usize = tools.values().map(|v| v.len()).sum();
        println!(
            "{} {} in {}:\n",
            toolset_matches.to_string().bold(),
            label,
            toolset.bright_blue()
        );

        for (tool_name, matches) in tools {
            println!("  {}", tool_name.bright_white().bold());

            for m in matches {
                let (symbol, field_display) = match m.field_type.as_str() {
                    "name" => continue, // Don't show name matches as separate lines
                    "desc" => ("◆".bright_magenta(), None),
                    "in" => ("→".bright_yellow(), Some(m.field_name.clone())),
                    "out" => ("←".bright_green(), Some(m.field_name.clone())),
                    _ => ("•".normal(), Some(m.field_name.clone())),
                };

                // Truncate long text
                let display_text = if m.matched_text.len() > 60 {
                    format!("{}...", &m.matched_text[..57])
                } else {
                    m.matched_text.clone()
                };

                if let Some(field) = field_display {
                    println!(
                        "    {} {}: {}",
                        symbol,
                        field.bright_white(),
                        format!("\"{}\"", display_text).dimmed()
                    );
                } else {
                    println!(
                        "    {} {}",
                        symbol,
                        format!("\"{}\"", display_text).dimmed()
                    );
                }
            }
            println!();
        }
    }

    // Print legend
    println!(
        "{} {}  {} {}  {} {}",
        "◆".bright_magenta(),
        "desc".dimmed(),
        "→".bright_yellow(),
        "input field".dimmed(),
        "←".bright_green(),
        "output field".dimmed()
    );
}
