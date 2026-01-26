//! Tool info command handlers.

use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcp::{ToolCapabilities, ToolType, get_tool_info, get_tool_type};
use crate::output::ToolInfoOutput;
use colored::Colorize;
use rmcp::model::Tool;
use std::path::Path;

use super::common::{PrepareToolOptions, prepare_tool};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Get info about a tool (list tools, prompts, resources).
#[allow(clippy::too_many_arguments)]
pub async fn tool_info(
    tool: String,
    methods: Vec<String>,
    input_only: bool,
    output_only: bool,
    description_only: bool,
    show_tools: bool,
    show_prompts: bool,
    show_resources: bool,
    show_all: bool,
    json_output: bool,
    config: Vec<String>,
    config_file: Option<String>,
    no_save: bool,
    yes: bool,
    verbose: bool,
    concise: bool,
    no_header: bool,
    level: usize,
) -> ToolResult<()> {
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

    // Get tool metadata
    let tool_type = get_tool_type(&prepared.plugin.template);

    // Get tool info - handle EntryPointNotFound specially
    // Never pass verbose to connection - verbose only affects output formatting, not debug logging
    let capabilities = match get_tool_info(&prepared.resolved, &prepared.tool_name, false).await {
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
            println!("  {} OAuth authentication failed\n", "✗".bright_red());
            println!(
                "    Could not initialize credential storage. Check that {} is writable.",
                "~/.tool/secrets/".bright_cyan()
            );
            std::process::exit(1);
        }
        Err(e) => return Err(e),
    };

    // Extract toolset name from the tool reference
    let toolset = tool.split('@').next().unwrap_or(&tool);

    // If -m is specified, we're drilling down to specific methods
    if !methods.is_empty() {
        // Find all matching tools
        let matching_tools: Vec<&Tool> = capabilities
            .tools
            .iter()
            .filter(|t| methods.iter().any(|m| m == t.name.as_ref()))
            .collect();

        // Validate all requested methods exist
        for method_name in &methods {
            if !matching_tools.iter().any(|t| t.name == *method_name) {
                if !concise {
                    println!(
                        "  {} Method not found: {}",
                        "✗".bright_red(),
                        method_name.bright_white()
                    );
                }
                std::process::exit(1);
            }
        }

        // Handle method-specific output
        if json_output {
            output_methods_json(&matching_tools, concise)?;
        } else if concise {
            output_methods_concise(
                toolset,
                &matching_tools,
                input_only,
                output_only,
                description_only,
                no_header,
                level,
            );
        } else {
            output_methods_normal(
                &matching_tools,
                input_only,
                output_only,
                description_only,
                verbose,
                level,
            );
        }
        return Ok(());
    }

    if json_output {
        output_tool_info_json(&capabilities, tool_type, &prepared.manifest_path, concise)?;
        return Ok(());
    }

    // Concise output (Header + TSV format)
    if concise {
        output_tool_info_concise(
            &capabilities,
            tool_type,
            &prepared.manifest_path,
            toolset,
            show_tools,
            show_prompts,
            show_resources,
            show_all,
            no_header,
            level,
        );
        return Ok(());
    }

    // Determine what to show
    let show_all = show_all || (!show_tools && !show_prompts && !show_resources);

    // Header - skip in verbose mode since connection debug info was already shown
    if !verbose {
        println!(
            "  {} Connected to {} v{}\n",
            "✓".bright_green(),
            capabilities.server_info.name.bold(),
            capabilities.server_info.version
        );
    }

    // Show server metadata
    println!("    {}       {}", "Type".dimmed(), tool_type);
    println!(
        "    {}   {}",
        "Location".dimmed(),
        prepared.manifest_path.display().to_string().dimmed()
    );
    println!();

    // Tools section
    if (show_all || show_tools) && !capabilities.tools.is_empty() {
        output_tools_section(&capabilities, verbose, level);
    }

    // Prompts section
    if (show_all || show_prompts) && !capabilities.prompts.is_empty() {
        output_prompts_section(&capabilities, verbose);
    }

    // Resources section
    if (show_all || show_resources) && !capabilities.resources.is_empty() {
        output_resources_section(&capabilities, verbose);
    }

    Ok(())
}

/// Output tools section in human-readable format.
fn output_tools_section(capabilities: &ToolCapabilities, verbose: bool, level: usize) {
    println!("    {}:", "Tools".dimmed());
    for (idx, tool) in capabilities.tools.iter().enumerate() {
        if verbose {
            // Verbose: name on its own line, description block below
            println!("      {}", tool.name.bright_cyan());
            if let Some(desc) = tool
                .description
                .as_ref()
                .and_then(|d| format_description(d, true, "        "))
            {
                println!("{}", desc.dimmed());
            }
        } else {
            // Default: name + first line inline
            let desc = tool
                .description
                .as_ref()
                .and_then(|d| format_description(d, false, ""))
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("      {}{}", tool.name.bright_cyan(), desc);
        }

        let has_input = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .is_some_and(|p| !p.is_empty());
        let has_output = tool.output_schema.is_some();

        // Show input parameters with tree structure
        if has_input {
            let schema = &tool.input_schema;
            let defs = schema.get("$defs").and_then(|d| d.as_object());
            let props = schema
                .get("properties")
                .and_then(|p| p.as_object())
                .unwrap();
            let required: Vec<&str> = schema
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let input_branch = if has_output { "├──" } else { "└──" };
            println!("      {} {}", input_branch.dimmed(), "Input".dimmed());

            let prop_count = props.len();
            for (i, (name, prop)) in props.iter().enumerate() {
                let is_last = i == prop_count - 1;
                let prefix = if has_output { "│" } else { " " };
                let branch = if is_last { "└──" } else { "├──" };
                let type_str = format_schema_type(prop, defs, level);
                let req_marker = if required.contains(&name.as_str()) {
                    "*"
                } else {
                    ""
                };
                let param_desc = prop
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");

                let param_name = format!("{}{}", name, req_marker);
                println!(
                    "      {}   {} {:<20} {:<10} {}",
                    prefix.dimmed(),
                    branch.dimmed(),
                    param_name,
                    type_str.dimmed(),
                    param_desc.dimmed()
                );
            }
        }

        // Show output schema with tree structure
        if let Some(output_schema) = &tool.output_schema {
            println!("      {} {}", "└──".dimmed(), "Output".dimmed());

            // Resolve the actual output schema (handles FastMCP's wrapped format with $defs)
            let defs = output_schema.get("$defs").and_then(|d| d.as_object());
            if let Some((resolved, required)) = resolve_output_schema(output_schema)
                && let Some(props) = resolved.get("properties").and_then(|p| p.as_object())
            {
                let prop_count = props.len();
                for (i, (name, prop)) in props.iter().enumerate() {
                    let is_last = i == prop_count - 1;
                    let branch = if is_last { "└──" } else { "├──" };
                    let type_str = format_schema_type(prop, defs, level);
                    let req_marker = if required.contains(&name.as_str()) {
                        "*"
                    } else {
                        ""
                    };
                    let param_desc = prop
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");

                    let param_name = format!("{}{}", name, req_marker);
                    println!(
                        "          {} {:<20} {:<10} {}",
                        branch.dimmed(),
                        param_name,
                        type_str.dimmed(),
                        param_desc.dimmed()
                    );
                }
            }
        }

        // Add spacing between tools
        if idx < capabilities.tools.len() - 1 {
            println!();
        }
    }
    println!();
}

/// Output prompts section in human-readable format.
fn output_prompts_section(capabilities: &ToolCapabilities, verbose: bool) {
    println!("    {}:", "Prompts".dimmed());
    for (idx, prompt) in capabilities.prompts.iter().enumerate() {
        if verbose {
            // Verbose: name on its own line, description block below
            println!("      {}", prompt.name.to_string().bright_magenta());
            if let Some(desc) = prompt
                .description
                .as_ref()
                .and_then(|d| format_description(d, true, "        "))
            {
                println!("{}", desc.dimmed());
            }
        } else {
            // Default: name + first line inline
            let desc = prompt
                .description
                .as_ref()
                .and_then(|d| format_description(d, false, ""))
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("      {}{}", prompt.name.to_string().bright_magenta(), desc);
        }

        // Show arguments if available
        if let Some(args) = &prompt.arguments
            && !args.is_empty()
        {
            for (i, arg) in args.iter().enumerate() {
                let is_last = i == args.len() - 1;
                let req_marker = if arg.required.unwrap_or(false) {
                    "*"
                } else {
                    ""
                };
                let arg_name = format!("{}{}", arg.name, req_marker);
                let arg_desc = arg.description.as_deref().unwrap_or("");
                let branch = if is_last { "└──" } else { "├──" };
                println!(
                    "      {} {:<20} {:<10} {}",
                    branch.dimmed(),
                    arg_name.bright_white(),
                    "string".dimmed(),
                    arg_desc.dimmed()
                );
            }
        }

        if idx < capabilities.prompts.len() - 1 {
            println!();
        }
    }
    println!();
}

/// Output resources section in human-readable format.
fn output_resources_section(capabilities: &ToolCapabilities, verbose: bool) {
    println!("    {}:", "Resources".dimmed());
    for (idx, resource) in capabilities.resources.iter().enumerate() {
        if verbose {
            // Verbose: uri on its own line, description block below
            println!("      {}", resource.uri.to_string().bright_yellow());
            if let Some(desc) = resource
                .description
                .as_ref()
                .and_then(|d| format_description(d, true, "        "))
            {
                println!("{}", desc.dimmed());
            }
        } else {
            // Default: uri + first line inline
            let desc = resource
                .description
                .as_ref()
                .and_then(|d| format_description(d, false, ""))
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("      {}{}", resource.uri.to_string().bright_yellow(), desc);
        }

        // Show resource details
        let has_name = !resource.name.is_empty();
        let has_mime = resource.mime_type.is_some();

        if has_name {
            let branch = if has_mime { "├──" } else { "└──" };
            println!(
                "      {} {:<12} {}",
                branch.dimmed(),
                "name".dimmed(),
                resource.name
            );
        }

        if let Some(mime) = &resource.mime_type {
            println!("      {} {:<12} {}", "└──".dimmed(), "mime".dimmed(), mime);
        }

        if idx < capabilities.resources.len() - 1 {
            println!();
        }
    }
    println!();
}

/// Output tool info in concise TSV format.
#[allow(clippy::too_many_arguments)]
fn output_tool_info_concise(
    capabilities: &ToolCapabilities,
    tool_type: ToolType,
    manifest_path: &Path,
    toolset: &str,
    show_tools: bool,
    show_prompts: bool,
    show_resources: bool,
    show_all: bool,
    no_header: bool,
    level: usize,
) {
    use crate::concise::quote;
    // Determine what to show
    let show_all_concise = show_all || (!show_tools && !show_prompts && !show_resources);

    // Metadata header + data
    if !no_header {
        println!("#type\tlocation");
    }
    println!(
        "{}\t{}",
        tool_type,
        quote(&manifest_path.display().to_string())
    );

    // Tools section
    if (show_all_concise || show_tools) && !capabilities.tools.is_empty() {
        if !no_header {
            println!("#tool");
        }
        for tool_item in &capabilities.tools {
            let params = format_schema_params_concise(&tool_item.input_schema, true, level);
            let outputs = tool_item
                .output_schema
                .as_ref()
                .map(|s| format_schema_params_concise(s, false, level))
                .unwrap_or_default();

            if outputs.is_empty() {
                println!("{}:{}({})", toolset, tool_item.name, params);
            } else {
                println!("{}:{}({}) -> {}", toolset, tool_item.name, params, outputs);
            }
        }
    }

    // Prompts section
    if (show_all_concise || show_prompts) && !capabilities.prompts.is_empty() {
        if !no_header {
            println!("#prompt\targs");
        }
        for prompt in &capabilities.prompts {
            let args = prompt
                .arguments
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|a| {
                            let marker = if a.required.unwrap_or(false) {
                                "*"
                            } else {
                                "?"
                            };
                            format!("{}{}: string", a.name, marker)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            println!("{}:{}\t{}", toolset, prompt.name, quote(&args));
        }
    }

    // Resources section
    if (show_all_concise || show_resources) && !capabilities.resources.is_empty() {
        if !no_header {
            println!("#uri\tname\tmime");
        }
        for resource in &capabilities.resources {
            println!(
                "{}\t{}\t{}",
                resource.uri,
                quote(&resource.name),
                resource.mime_type.as_deref().unwrap_or("-")
            );
        }
    }
}

/// Resolve a JSON schema, handling $ref and $defs.
/// For output schemas with union types (anyOf), extract the primary (non-Error) type.
fn resolve_output_schema(
    schema: &serde_json::Map<String, serde_json::Value>,
) -> Option<(&serde_json::Map<String, serde_json::Value>, Vec<&str>)> {
    let defs = schema.get("$defs").and_then(|d| d.as_object());

    // Check if this is a wrapped result schema (FastMCP style)
    if let Some(props) = schema.get("properties").and_then(|p| p.as_object())
        && let Some(result_prop) = props.get("result")
    {
        // Handle anyOf (union types like Output | ErrorOutput)
        if let Some(any_of) = result_prop.get("anyOf").and_then(|a| a.as_array()) {
            // Find the first non-Error type
            for variant in any_of {
                if let Some(ref_path) = variant.get("$ref").and_then(|r| r.as_str()) {
                    // Extract type name from "#/$defs/TypeName"
                    let type_name = ref_path.rsplit('/').next()?;
                    // Skip error types
                    if type_name.contains("Error") {
                        continue;
                    }
                    // Resolve the reference
                    if let Some(resolved) = defs
                        .and_then(|d| d.get(type_name))
                        .and_then(|t| t.as_object())
                    {
                        let required: Vec<&str> = resolved
                            .get("required")
                            .and_then(|r| r.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                            .unwrap_or_default();
                        return Some((resolved, required));
                    }
                }
            }
        }
        // Handle single $ref
        if let Some(ref_path) = result_prop.get("$ref").and_then(|r| r.as_str()) {
            let type_name = ref_path.rsplit('/').next()?;
            if let Some(resolved) = defs
                .and_then(|d| d.get(type_name))
                .and_then(|t| t.as_object())
            {
                let required: Vec<&str> = resolved
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                return Some((resolved, required));
            }
        }
    }

    // Fallback: use the schema directly
    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    Some((schema, required))
}

/// Get the type string for a schema property, resolving $ref and expanding nested objects.
fn format_schema_type(
    prop: &serde_json::Value,
    defs: Option<&serde_json::Map<String, serde_json::Value>>,
    max_depth: usize,
) -> String {
    format_schema_type_recursive(prop, defs, 0, max_depth, &mut Vec::new())
}

/// Recursive helper with depth limit and cycle detection.
fn format_schema_type_recursive(
    prop: &serde_json::Value,
    defs: Option<&serde_json::Map<String, serde_json::Value>>,
    depth: usize,
    max_depth: usize,
    seen: &mut Vec<String>,
) -> String {
    // Handle $ref - resolve and expand
    if let Some(ref_path) = prop.get("$ref").and_then(|r| r.as_str()) {
        let type_name = ref_path.rsplit('/').next().unwrap_or("object");

        // Cycle detection
        if seen.contains(&type_name.to_string()) {
            return type_name.to_string();
        }

        // Resolve the reference
        if let Some(resolved) = defs
            .and_then(|d| d.get(type_name))
            .and_then(|t| t.as_object())
        {
            seen.push(type_name.to_string());
            let result = format_schema_type_recursive(
                &serde_json::Value::Object(resolved.clone()),
                defs,
                depth,
                max_depth,
                seen,
            );
            seen.pop();
            return result;
        }
        return type_name.to_string();
    }

    // Handle direct type
    if let Some(type_str) = prop.get("type").and_then(|t| t.as_str()) {
        // Handle array types
        if type_str == "array" {
            if let Some(items) = prop.get("items") {
                let item_type = format_schema_type_recursive(items, defs, depth, max_depth, seen);
                return format!("{}[]", item_type);
            }
            return "array".to_string();
        }

        // Handle object types - expand properties
        if type_str == "object" {
            if depth >= max_depth {
                return "object".to_string();
            }
            if let Some(props) = prop.get("properties").and_then(|p| p.as_object()) {
                let required: Vec<&str> = prop
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                let fields: Vec<String> = props
                    .iter()
                    .map(|(name, p)| {
                        let field_type =
                            format_schema_type_recursive(p, defs, depth + 1, max_depth, seen);
                        let marker = if required.contains(&name.as_str()) {
                            "*"
                        } else {
                            "?"
                        };
                        format!("{}{}: {}", name, marker, field_type)
                    })
                    .collect();

                if fields.is_empty() {
                    return "object".to_string();
                }
                return format!("{{{}}}", fields.join(", "));
            }
        }

        return type_str.to_string();
    }

    // Handle anyOf
    if let Some(any_of) = prop.get("anyOf").and_then(|a| a.as_array()) {
        // Find first non-null type
        for variant in any_of {
            let t = format_schema_type_recursive(variant, defs, depth, max_depth, seen);
            if t != "null" {
                return t;
            }
        }
    }

    "any".to_string()
}

/// Format schema properties as param list for concise output.
pub(super) fn format_schema_params_concise(
    schema: &std::sync::Arc<serde_json::Map<String, serde_json::Value>>,
    is_input: bool,
    level: usize,
) -> String {
    let defs = schema.get("$defs").and_then(|d| d.as_object());

    // For output schemas, resolve the actual type and use format_schema_type for consistent depth handling
    if !is_input {
        let resolved_schema = match resolve_output_schema(schema) {
            Some((s, _)) => s,
            None => return String::new(),
        };
        let schema_value = serde_json::Value::Object(resolved_schema.clone());
        return format_schema_type(&schema_value, defs, level);
    }

    // For input schemas, always show parameter names (level only affects nested type expansion)
    let props = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return String::new(),
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let params: Vec<String> = props
        .iter()
        .map(|(name, prop)| {
            let type_str = format_schema_type(prop, defs, level);
            let marker = if required.contains(&name.as_str()) {
                "*"
            } else {
                "?"
            };
            format!("{}{}: {}", name, marker, type_str)
        })
        .collect();

    params.join(", ")
}

/// Output tool info as JSON.
fn output_tool_info_json(
    capabilities: &ToolCapabilities,
    tool_type: ToolType,
    manifest_path: &Path,
    concise: bool,
) -> ToolResult<()> {
    let output =
        ToolInfoOutput::from_capabilities(capabilities, tool_type.to_string(), manifest_path);
    if concise {
        println!("{}", output.to_json()?);
    } else {
        println!("{}", output.to_json_pretty()?);
    }
    Ok(())
}

/// Output methods as JSON (object keyed by method name).
fn output_methods_json(tools: &[&Tool], concise: bool) -> ToolResult<()> {
    let mut map = serde_json::Map::new();
    for tool in tools {
        let value = serde_json::json!({
            "description": tool.description,
            "input_schema": tool.input_schema,
            "output_schema": tool.output_schema,
        });
        map.insert(tool.name.to_string(), value);
    }
    let json = serde_json::Value::Object(map);
    if concise {
        println!("{}", serde_json::to_string(&json)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&json)?);
    }
    Ok(())
}

/// Output methods in concise TSV format.
#[allow(clippy::too_many_arguments)]
fn output_methods_concise(
    toolset: &str,
    tools: &[&Tool],
    input_only: bool,
    output_only: bool,
    description_only: bool,
    no_header: bool,
    level: usize,
) {
    use crate::concise::quote;

    // If description only, print method + description
    if description_only {
        if !no_header {
            println!("#method\tdescription");
        }
        for tool in tools {
            let desc = tool.description.as_deref().unwrap_or("");
            println!("{}\t{}", tool.name, quote(desc));
        }
        return;
    }

    // If input only, show input schema parameters with method column
    if input_only {
        if !no_header {
            println!("#method\tparam\ttype\trequired\tdescription");
        }
        for tool in tools {
            if let Some(props) = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
            {
                let required: Vec<&str> = tool
                    .input_schema
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let defs = tool.input_schema.get("$defs").and_then(|d| d.as_object());

                for (name, prop) in props {
                    let is_required = required.contains(&name.as_str());
                    let type_str = format_schema_type(prop, defs, level);
                    let desc = prop
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        tool.name,
                        name,
                        type_str,
                        is_required,
                        quote(desc)
                    );
                }
            }
        }
        return;
    }

    // If output only, show output schema parameters with method column
    if output_only {
        if !no_header {
            println!("#method\tparam\ttype\trequired\tdescription");
        }
        for tool in tools {
            if let Some(output_schema) = &tool.output_schema {
                let defs = output_schema.get("$defs").and_then(|d| d.as_object());
                if let Some((resolved, required)) = resolve_output_schema(output_schema)
                    && let Some(props) = resolved.get("properties").and_then(|p| p.as_object())
                {
                    for (name, prop) in props {
                        let is_required = required.contains(&name.as_str());
                        let type_str = format_schema_type(prop, defs, level);
                        let desc = prop
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        println!(
                            "{}\t{}\t{}\t{}\t{}",
                            tool.name,
                            name,
                            type_str,
                            is_required,
                            quote(desc)
                        );
                    }
                }
            }
        }
        return;
    }

    // Default: show function signatures
    if !no_header {
        println!("#tool");
    }
    for tool in tools {
        let params = format_schema_params_concise(&tool.input_schema, true, level);
        let outputs = tool
            .output_schema
            .as_ref()
            .map(|s| format_schema_params_concise(s, false, level))
            .unwrap_or_default();

        if outputs.is_empty() {
            println!("{}:{}({})", toolset, tool.name, params);
        } else {
            println!("{}:{}({}) -> {}", toolset, tool.name, params, outputs);
        }
    }
}

/// Output methods in human-readable format.
fn output_methods_normal(
    tools: &[&Tool],
    input_only: bool,
    output_only: bool,
    description_only: bool,
    verbose: bool,
    level: usize,
) {
    for (idx, tool) in tools.iter().enumerate() {
        // If description only, just print description
        if description_only {
            if let Some(desc) = &tool.description {
                if verbose {
                    // Verbose: show full description with method name
                    println!("      {}", tool.name.bright_cyan());
                    if let Some(formatted) = format_description(desc, true, "        ") {
                        println!("{}", formatted.dimmed());
                    }
                } else {
                    // Non-verbose: method name + first line
                    let first_line = format_description(desc, false, "")
                        .map(|d| format!("  {}", d.dimmed()))
                        .unwrap_or_default();
                    println!("      {}{}", tool.name.bright_cyan(), first_line);
                }
            }
            if idx < tools.len() - 1 {
                println!();
            }
            continue;
        }

        // Determine what to show
        let show_all = !input_only && !output_only;

        if verbose {
            // Verbose: name on its own line, description block below
            println!("      {}", tool.name.bright_cyan());
            if show_all
                && let Some(desc) = &tool.description
                && let Some(formatted) = format_description(desc, true, "        ")
            {
                println!("{}\n", formatted.dimmed());
            }
        } else {
            // Non-verbose: name + first line description inline
            let desc = tool
                .description
                .as_ref()
                .and_then(|d| format_description(d, false, ""))
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            if show_all {
                println!("      {}{}", tool.name.bright_cyan(), desc);
            } else {
                println!("      {}", tool.name.bright_cyan());
            }
        }

        let has_input = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .is_some_and(|p| !p.is_empty());
        let has_output = tool.output_schema.is_some();

        // Input schema
        if (show_all || input_only) && has_input {
            let schema = &tool.input_schema;
            let defs = schema.get("$defs").and_then(|d| d.as_object());
            let props = schema
                .get("properties")
                .and_then(|p| p.as_object())
                .unwrap();
            let required: Vec<&str> = schema
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            let input_branch = if has_output && show_all {
                "├──"
            } else {
                "└──"
            };
            println!("      {} {}", input_branch.dimmed(), "Input".dimmed());

            let prop_count = props.len();
            for (i, (name, prop)) in props.iter().enumerate() {
                let is_last = i == prop_count - 1;
                let prefix = if has_output && show_all { "│" } else { " " };
                let branch = if is_last { "└──" } else { "├──" };
                let type_str = format_schema_type(prop, defs, level);
                let req_marker = if required.contains(&name.as_str()) {
                    "*"
                } else {
                    ""
                };
                let param_desc = prop
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("");

                let param_name = format!("{}{}", name, req_marker);
                println!(
                    "      {}   {} {:<20} {:<10} {}",
                    prefix.dimmed(),
                    branch.dimmed(),
                    param_name,
                    type_str.dimmed(),
                    param_desc.dimmed()
                );
            }
        }

        // Output schema
        if (show_all || output_only)
            && has_output
            && let Some(output_schema) = &tool.output_schema
        {
            println!("      {} {}", "└──".dimmed(), "Output".dimmed());

            let defs = output_schema.get("$defs").and_then(|d| d.as_object());
            if let Some((resolved, required)) = resolve_output_schema(output_schema)
                && let Some(props) = resolved.get("properties").and_then(|p| p.as_object())
            {
                let prop_count = props.len();
                for (i, (name, prop)) in props.iter().enumerate() {
                    let is_last = i == prop_count - 1;
                    let branch = if is_last { "└──" } else { "├──" };
                    let type_str = format_schema_type(prop, defs, level);
                    let req_marker = if required.contains(&name.as_str()) {
                        "*"
                    } else {
                        ""
                    };
                    let param_desc = prop
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");

                    let param_name = format!("{}{}", name, req_marker);
                    println!(
                        "          {} {:<20} {:<10} {}",
                        branch.dimmed(),
                        param_name,
                        type_str.dimmed(),
                        param_desc.dimmed()
                    );
                }
            }
        }

        // Add spacing between methods
        if idx < tools.len() - 1 {
            println!();
        }
    }
    println!();
}
