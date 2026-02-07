//! Tool preview command handler - preview tools from the registry without installing.

use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcpb::{McpbPrompt, McpbTool, McpbToolFull, StaticResponses};
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use crate::styles::Spinner;
use colored::Colorize;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Preview a tool from the registry without installing.
#[allow(clippy::too_many_arguments)]
pub async fn tool_preview(
    tool: String,
    methods: Vec<String>,
    input_only: bool,
    output_only: bool,
    description_only: bool,
    show_tools: bool,
    show_prompts: bool,
    show_all: bool,
    json_output: bool,
    concise: bool,
    no_header: bool,
    level: usize,
) -> ToolResult<()> {
    // Parse tool reference
    let plugin_ref = PluginRef::parse(&tool)?;

    let namespace = plugin_ref.namespace().ok_or_else(|| {
        ToolError::InvalidReference(
            "Preview requires a registry reference (namespace/name). Use 'tool info' for local tools.".into(),
        )
    })?;
    let name = plugin_ref.name();

    // Determine version to fetch
    let version_str = plugin_ref.version().map(|v| v.to_string());

    // Show spinner while fetching (human-readable mode only)
    let show_spinner = !json_output && !concise;
    let spinner = show_spinner.then(|| {
        Spinner::new(format!(
            "Fetching {}/{}{}",
            namespace,
            name,
            version_str
                .as_ref()
                .map(|v| format!("@{}", v))
                .unwrap_or_default()
        ))
    });

    // Fetch version info with manifest from registry
    let client = RegistryClient::new();
    let version_info = if let Some(ref v) = version_str {
        client.get_version(namespace, name, v).await?
    } else {
        // Get latest version
        let artifact = client.get_artifact(namespace, name).await?;
        artifact.latest_version.ok_or_else(|| ToolError::NotFound {
            kind: "version".to_string(),
            reference: format!("{}/{}", namespace, name),
        })?
    };

    if let Some(s) = spinner {
        s.done();
    }

    // Extract manifest
    let manifest = version_info.manifest.ok_or_else(|| {
        ToolError::Generic("Registry did not return manifest data for this version".into())
    })?;

    // Extract tools from manifest
    // Priority: _meta.store.tool.mcpb.static_responses.tools/list.tools > top-level tools
    let tools = extract_tools_from_manifest(&manifest);
    let prompts = extract_prompts_from_manifest(&manifest);

    let tool_ref = format!("{}/{}", namespace, name);
    let version = version_info.version;

    // If -m is specified, filter to specific methods
    if !methods.is_empty() {
        let matching_tools: Vec<&McpbToolFull> = tools
            .iter()
            .filter(|t| methods.iter().any(|m| m == &t.name))
            .collect();

        // Validate all requested methods exist
        for method_name in &methods {
            if !matching_tools.iter().any(|t| &t.name == method_name) {
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

        if json_output {
            output_methods_json(&matching_tools, concise)?;
        } else if concise {
            output_methods_concise(
                &tool_ref,
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
                level,
            );
        }
        return Ok(());
    }

    if json_output {
        output_preview_json(&manifest, &tools, &prompts, concise)?;
        return Ok(());
    }

    // Concise output
    if concise {
        output_preview_concise(
            &tool_ref,
            &tools,
            &prompts,
            show_tools,
            show_prompts,
            show_all,
            no_header,
            level,
        );
        return Ok(());
    }

    // Human-readable output
    let show_all = show_all || (!show_tools && !show_prompts);

    // Header
    println!(
        "  {} {}/{} v{}\n",
        "✓".bright_green(),
        namespace.bold(),
        name.bold(),
        version
    );

    // Show metadata
    if let Some(desc) = manifest.get("description").and_then(|d| d.as_str()) {
        println!("  · {}", desc.dimmed());
        println!();
    }

    // Tools section
    if (show_all || show_tools) && !tools.is_empty() {
        output_tools_section(&tools, level);
    }

    // Prompts section
    if (show_all || show_prompts) && !prompts.is_empty() {
        output_prompts_section(&prompts);
    }

    if tools.is_empty() && prompts.is_empty() {
        println!(
            "    {}",
            "No tools or prompts declared in manifest".dimmed()
        );
        println!();
    }

    Ok(())
}

/// Extract tools from manifest, preferring static_responses over top-level tools.
fn extract_tools_from_manifest(manifest: &serde_json::Value) -> Vec<McpbToolFull> {
    // Try _meta.store.tool.mcpb.static_responses.tools/list.tools first
    if let Some(store_meta) = manifest.get("_meta").and_then(|m| m.get("store.tool.mcpb")) {
        if let Some(tools_list) = store_meta
            .get("static_responses")
            .and_then(|sr| serde_json::from_value::<StaticResponses>(sr.clone()).ok())
            .and_then(|sr| sr.tools_list)
        {
            return tools_list.tools;
        }
        // Also check legacy _meta.store.tool.mcpb.tools
        if let Some(t) = store_meta
            .get("tools")
            .and_then(|tools| serde_json::from_value::<Vec<McpbToolFull>>(tools.clone()).ok())
        {
            return t;
        }
    }

    // Fallback to top-level tools (simple format, need to convert)
    if let Some(tools) = manifest.get("tools") {
        // Try full format first
        if let Ok(t) = serde_json::from_value::<Vec<McpbToolFull>>(tools.clone()) {
            return t;
        }
        // Try simple format and convert
        if let Ok(simple) = serde_json::from_value::<Vec<McpbTool>>(tools.clone()) {
            return simple
                .into_iter()
                .map(|t| McpbToolFull {
                    name: t.name,
                    description: t.description,
                    title: None,
                    input_schema: None,
                    output_schema: None,
                })
                .collect();
        }
    }

    Vec::new()
}

/// Extract prompts from manifest.
fn extract_prompts_from_manifest(manifest: &serde_json::Value) -> Vec<McpbPrompt> {
    // Try _meta.store.tool.mcpb.static_responses.prompts/list.prompts first
    if let Some(prompts_list) = manifest
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|sm| sm.get("static_responses"))
        .and_then(|sr| serde_json::from_value::<StaticResponses>(sr.clone()).ok())
        .and_then(|sr| sr.prompts_list)
    {
        return prompts_list.prompts;
    }

    // Fallback to top-level prompts
    if let Some(p) = manifest
        .get("prompts")
        .and_then(|prompts| serde_json::from_value::<Vec<McpbPrompt>>(prompts.clone()).ok())
    {
        return p;
    }

    Vec::new()
}

/// Output tools section in human-readable format.
fn output_tools_section(tools: &[McpbToolFull], level: usize) {
    println!("    {}:", "Tools".dimmed());
    for (idx, tool) in tools.iter().enumerate() {
        // Name + first line description inline
        let desc = format_description(&tool.description, false, "")
            .map(|d: String| format!("  {}", d.dimmed()))
            .unwrap_or_default();
        println!("      {}{}", tool.name.bright_cyan(), desc);

        let has_input = tool
            .input_schema
            .as_ref()
            .and_then(|s: &serde_json::Value| s.get("properties"))
            .and_then(|p: &serde_json::Value| p.as_object())
            .is_some_and(|p: &serde_json::Map<String, serde_json::Value>| !p.is_empty());
        let has_output = tool
            .output_schema
            .as_ref()
            .and_then(|s: &serde_json::Value| s.get("properties"))
            .and_then(|p: &serde_json::Value| p.as_object())
            .is_some_and(|p: &serde_json::Map<String, serde_json::Value>| !p.is_empty());

        // Show input parameters with tree structure
        if has_input {
            let schema = tool.input_schema.as_ref().unwrap();
            let defs = schema.get("$defs").and_then(|d| d.as_object());
            let props = schema
                .get("properties")
                .and_then(|p| p.as_object())
                .unwrap();
            let required: Vec<&str> = schema
                .get("required")
                .and_then(|r: &serde_json::Value| r.as_array())
                .map(|arr: &Vec<serde_json::Value>| {
                    arr.iter()
                        .filter_map(|v: &serde_json::Value| v.as_str())
                        .collect()
                })
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
        if has_output {
            let output_schema = tool.output_schema.as_ref().unwrap();
            println!("      {} {}", "└──".dimmed(), "Output".dimmed());

            let defs = output_schema.get("$defs").and_then(|d| d.as_object());
            if let Some(props) = output_schema.get("properties").and_then(|p| p.as_object()) {
                let required: Vec<&str> = output_schema
                    .get("required")
                    .and_then(|r: &serde_json::Value| r.as_array())
                    .map(|arr: &Vec<serde_json::Value>| {
                        arr.iter()
                            .filter_map(|v: &serde_json::Value| v.as_str())
                            .collect()
                    })
                    .unwrap_or_default();

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
        if idx < tools.len() - 1 {
            println!();
        }
    }
    println!();
}

/// Output prompts section in human-readable format.
fn output_prompts_section(prompts: &[McpbPrompt]) {
    println!("    {}:", "Prompts".dimmed());
    for (idx, prompt) in prompts.iter().enumerate() {
        // Name + first line description inline
        let desc = format_description(&prompt.description, false, "")
            .map(|d: String| format!("  {}", d.dimmed()))
            .unwrap_or_default();
        println!("      {}{}", prompt.name.bright_magenta(), desc);

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

        if idx < prompts.len() - 1 {
            println!();
        }
    }
    println!();
}

/// Get the type string for a schema property.
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
    // Handle $ref
    if let Some(ref_path) = prop.get("$ref").and_then(|r| r.as_str()) {
        let type_name = ref_path.rsplit('/').next().unwrap_or("object");

        if seen.contains(&type_name.to_string()) {
            return type_name.to_string();
        }

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
        if type_str == "array" {
            if let Some(items) = prop.get("items") {
                let item_type = format_schema_type_recursive(items, defs, depth, max_depth, seen);
                return format!("{}[]", item_type);
            }
            return "array".to_string();
        }

        if type_str == "object" {
            if depth >= max_depth {
                return "object".to_string();
            }
            if let Some(props) = prop.get("properties").and_then(|p| p.as_object()) {
                let required: Vec<&str> = prop
                    .get("required")
                    .and_then(|r: &serde_json::Value| r.as_array())
                    .map(|arr: &Vec<serde_json::Value>| {
                        arr.iter()
                            .filter_map(|v: &serde_json::Value| v.as_str())
                            .collect()
                    })
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
fn format_schema_params_concise(
    schema: &serde_json::Value,
    is_input: bool,
    level: usize,
) -> String {
    let defs = schema.get("$defs").and_then(|d| d.as_object());

    if !is_input {
        return format_schema_type(schema, defs, level);
    }

    let props = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return String::new(),
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r: &serde_json::Value| r.as_array())
        .map(|arr: &Vec<serde_json::Value>| {
            arr.iter()
                .filter_map(|v: &serde_json::Value| v.as_str())
                .collect()
        })
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

/// Output preview in concise TSV format.
#[allow(clippy::too_many_arguments)]
fn output_preview_concise(
    tool_ref: &str,
    tools: &[McpbToolFull],
    prompts: &[McpbPrompt],
    show_tools: bool,
    show_prompts: bool,
    show_all: bool,
    no_header: bool,
    level: usize,
) {
    use crate::concise::quote;

    let show_all = show_all || (!show_tools && !show_prompts);

    // Tools section
    if (show_all || show_tools) && !tools.is_empty() {
        if !no_header {
            println!("#tool");
        }
        for tool_item in tools {
            let params = tool_item
                .input_schema
                .as_ref()
                .map(|s| format_schema_params_concise(s, true, level))
                .unwrap_or_default();
            let outputs = tool_item
                .output_schema
                .as_ref()
                .map(|s| format_schema_params_concise(s, false, level))
                .unwrap_or_default();

            if outputs.is_empty() {
                println!("{}:{}({})", tool_ref, tool_item.name, params);
            } else {
                println!("{}:{}({}) -> {}", tool_ref, tool_item.name, params, outputs);
            }
        }
    }

    // Prompts section
    if (show_all || show_prompts) && !prompts.is_empty() {
        if !no_header {
            println!("#prompt\targs");
        }
        for prompt in prompts {
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
            println!("{}:{}\t{}", tool_ref, prompt.name, quote(&args));
        }
    }
}

/// Output preview as JSON (matching tool info --json format).
fn output_preview_json(
    manifest: &serde_json::Value,
    tools: &[McpbToolFull],
    prompts: &[McpbPrompt],
    concise: bool,
) -> ToolResult<()> {
    // Build tools map
    let mut tools_map = serde_json::Map::new();
    for tool in tools {
        let value = serde_json::json!({
            "description": tool.description,
            "input_schema": tool.input_schema,
            "output_schema": tool.output_schema,
        });
        tools_map.insert(tool.name.clone(), value);
    }

    // Build prompts map
    let mut prompts_map = serde_json::Map::new();
    for prompt in prompts {
        let value = serde_json::json!({
            "description": prompt.description,
            "arguments": prompt.arguments,
        });
        prompts_map.insert(prompt.name.clone(), value);
    }

    // Extract server info from manifest
    let server_type = manifest
        .get("server")
        .and_then(|s| s.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("stdio");
    let transport = manifest
        .get("server")
        .and_then(|s| s.get("transport"))
        .and_then(|t| t.as_str())
        .unwrap_or("stdio");

    let output = serde_json::json!({
        "server": {
            "type": server_type,
        },
        "type": transport,
        "tools": tools_map,
        "prompts": prompts_map,
        "resources": {},
    });

    if concise {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }
    Ok(())
}

/// Output methods as JSON.
fn output_methods_json(tools: &[&McpbToolFull], concise: bool) -> ToolResult<()> {
    let mut map = serde_json::Map::new();
    for tool in tools {
        let value = serde_json::json!({
            "description": tool.description,
            "input_schema": tool.input_schema,
            "output_schema": tool.output_schema,
        });
        map.insert(tool.name.clone(), value);
    }
    let json = serde_json::Value::Object(map);
    if concise {
        println!("{}", serde_json::to_string(&json)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&json)?);
    }
    Ok(())
}

/// Output methods in concise format.
#[allow(clippy::too_many_arguments)]
fn output_methods_concise(
    tool_ref: &str,
    tools: &[&McpbToolFull],
    input_only: bool,
    output_only: bool,
    description_only: bool,
    no_header: bool,
    level: usize,
) {
    use crate::concise::quote;

    if description_only {
        if !no_header {
            println!("#method\tdescription");
        }
        for tool in tools {
            println!("{}\t{}", tool.name, quote(&tool.description));
        }
        return;
    }

    if input_only {
        if !no_header {
            println!("#method\tparam\ttype\trequired\tdescription");
        }
        for tool in tools {
            if let Some(schema) = &tool.input_schema
                && let Some(props) = schema.get("properties").and_then(|p| p.as_object())
            {
                let required: Vec<&str> = schema
                    .get("required")
                    .and_then(|r: &serde_json::Value| r.as_array())
                    .map(|arr: &Vec<serde_json::Value>| {
                        arr.iter()
                            .filter_map(|v: &serde_json::Value| v.as_str())
                            .collect()
                    })
                    .unwrap_or_default();
                let defs = schema.get("$defs").and_then(|d| d.as_object());

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

    if output_only {
        if !no_header {
            println!("#method\tparam\ttype\trequired\tdescription");
        }
        for tool in tools {
            if let Some(schema) = &tool.output_schema {
                let defs = schema.get("$defs").and_then(|d| d.as_object());
                if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
                    let required: Vec<&str> = schema
                        .get("required")
                        .and_then(|r: &serde_json::Value| r.as_array())
                        .map(|arr: &Vec<serde_json::Value>| {
                            arr.iter()
                                .filter_map(|v: &serde_json::Value| v.as_str())
                                .collect()
                        })
                        .unwrap_or_default();

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
        let params = tool
            .input_schema
            .as_ref()
            .map(|s| format_schema_params_concise(s, true, level))
            .unwrap_or_default();
        let outputs = tool
            .output_schema
            .as_ref()
            .map(|s| format_schema_params_concise(s, false, level))
            .unwrap_or_default();

        if outputs.is_empty() {
            println!("{}:{}({})", tool_ref, tool.name, params);
        } else {
            println!("{}:{}({}) -> {}", tool_ref, tool.name, params, outputs);
        }
    }
}

/// Output methods in human-readable format.
fn output_methods_normal(
    tools: &[&McpbToolFull],
    input_only: bool,
    output_only: bool,
    description_only: bool,
    level: usize,
) {
    for (idx, tool) in tools.iter().enumerate() {
        if description_only {
            let first_line = format_description(&tool.description, false, "")
                .map(|d: String| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("      {}{}", tool.name.bright_cyan(), first_line);
            if idx < tools.len() - 1 {
                println!();
            }
            continue;
        }

        let show_all = !input_only && !output_only;

        // Name + first line description inline
        let desc = format_description(&tool.description, false, "")
            .map(|d: String| format!("  {}", d.dimmed()))
            .unwrap_or_default();
        if show_all {
            println!("      {}{}", tool.name.bright_cyan(), desc);
        } else {
            println!("      {}", tool.name.bright_cyan());
        }

        let has_input = tool
            .input_schema
            .as_ref()
            .and_then(|s: &serde_json::Value| s.get("properties"))
            .and_then(|p: &serde_json::Value| p.as_object())
            .is_some_and(|p: &serde_json::Map<String, serde_json::Value>| !p.is_empty());
        let has_output = tool
            .output_schema
            .as_ref()
            .and_then(|s: &serde_json::Value| s.get("properties"))
            .and_then(|p: &serde_json::Value| p.as_object())
            .is_some_and(|p: &serde_json::Map<String, serde_json::Value>| !p.is_empty());

        // Input schema
        if (show_all || input_only) && has_input {
            let schema = tool.input_schema.as_ref().unwrap();
            let defs = schema.get("$defs").and_then(|d| d.as_object());
            let props = schema
                .get("properties")
                .and_then(|p| p.as_object())
                .unwrap();
            let required: Vec<&str> = schema
                .get("required")
                .and_then(|r: &serde_json::Value| r.as_array())
                .map(|arr: &Vec<serde_json::Value>| {
                    arr.iter()
                        .filter_map(|v: &serde_json::Value| v.as_str())
                        .collect()
                })
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
        if (show_all || output_only) && has_output {
            let output_schema = tool.output_schema.as_ref().unwrap();
            println!("      {} {}", "└──".dimmed(), "Output".dimmed());

            let defs = output_schema.get("$defs").and_then(|d| d.as_object());
            if let Some(props) = output_schema.get("properties").and_then(|p| p.as_object()) {
                let required: Vec<&str> = output_schema
                    .get("required")
                    .and_then(|r: &serde_json::Value| r.as_array())
                    .map(|arr: &Vec<serde_json::Value>| {
                        arr.iter()
                            .filter_map(|v: &serde_json::Value| v.as_str())
                            .collect()
                    })
                    .unwrap_or_default();

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

        if idx < tools.len() - 1 {
            println!();
        }
    }
    println!();
}
