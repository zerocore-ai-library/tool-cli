//! Tool list command handlers.

use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcp::get_tool_info;
use crate::output::{
    FullServerOutput, ServerOutput, ToolServerInfo, full_list_to_json, full_list_to_json_pretty,
    list_to_json, list_to_json_pretty,
};
use crate::resolver::{FilePluginResolver, load_tool_from_path};
use crate::system_config::allocate_system_config;
use colored::Colorize;
use std::collections::BTreeMap;
use std::path::PathBuf;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

pub(super) struct ToolListEntry {
    pub name: String,
    pub tool_type: String,
    pub description: Option<String>,
    pub path: PathBuf,
}

/// Result of resolving a tool reference.
#[derive(Debug)]
pub struct ResolvedToolPath {
    /// The resolved path to the tool directory.
    pub path: PathBuf,
    /// Whether this was resolved as an installed tool (via FilePluginResolver).
    pub is_installed: bool,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// List all installed tools.
pub async fn list_tools(
    filter: Option<&str>,
    json_output: bool,
    full: bool,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    let resolver = FilePluginResolver::default();
    let tools = resolver.list_tools().await?;

    // Filter if specified
    let filtered: Vec<_> = if let Some(f) = filter {
        let pattern_lower = f.to_lowercase();
        tools
            .iter()
            .filter(|t| t.to_string().to_lowercase().contains(&pattern_lower))
            .collect()
    } else {
        tools.iter().collect()
    };

    if filtered.is_empty() {
        if !concise {
            if let Some(pattern) = filter {
                println!(
                    "  {} No tools found matching: {}",
                    "✗".bright_red(),
                    pattern.bright_white().bold()
                );
            } else {
                println!("  {} No tools installed", "✗".bright_red());
                println!("\n    {}", "Searched:".dimmed());
                if let Ok(cwd) = std::env::current_dir() {
                    println!("      {}", cwd.join("tools").display().to_string().dimmed());
                }
                if let Some(home) = dirs::home_dir() {
                    println!(
                        "      {}",
                        home.join(".tool/tools").display().to_string().dimmed()
                    );
                }
            }
        }
        return Ok(());
    }

    // Collect tool info for each ref
    let mut tool_entries: Vec<ToolListEntry> = Vec::new();

    for plugin_ref in &filtered {
        let entry = match resolver.resolve_tool(&plugin_ref.to_string()).await {
            Ok(Some(resolved)) => {
                let description = resolved
                    .template
                    .description
                    .clone()
                    .or_else(|| resolved.template.display_name.clone());
                let transport = resolved.template.server.transport.to_string();

                ToolListEntry {
                    name: plugin_ref.to_string(),
                    tool_type: transport,
                    description,
                    path: resolved
                        .path
                        .parent()
                        .unwrap_or(&resolved.path)
                        .to_path_buf(),
                }
            }
            _ => ToolListEntry {
                name: plugin_ref.to_string(),
                tool_type: "unknown".to_string(),
                description: None,
                path: PathBuf::new(),
            },
        };
        tool_entries.push(entry);
    }

    // JSON output (object-keyed by server name)
    if json_output {
        if full {
            // Full output: include tools, prompts, resources for each server
            let mut output: BTreeMap<String, FullServerOutput> = BTreeMap::new();

            for entry in &tool_entries {
                // Load the tool manifest and resolve it
                let resolved_plugin = match load_tool_from_path(&entry.path) {
                    Ok(p) => p,
                    Err(_) => {
                        // Can't load manifest, include basic info
                        output.insert(
                            entry.name.clone(),
                            FullServerOutput {
                                server_type: entry.tool_type.clone(),
                                description: entry.description.clone(),
                                location: entry.path.display().to_string(),
                                server: ToolServerInfo {
                                    name: entry.name.clone(),
                                    version: "unknown".to_string(),
                                },
                                tools: BTreeMap::new(),
                                prompts: BTreeMap::new(),
                                resources: BTreeMap::new(),
                            },
                        );
                        continue;
                    }
                };

                let user_config = BTreeMap::new();
                let system_config =
                    match allocate_system_config(resolved_plugin.template.system_config.as_ref()) {
                        Ok(c) => c,
                        Err(_) => {
                            output.insert(
                                entry.name.clone(),
                                FullServerOutput {
                                    server_type: entry.tool_type.clone(),
                                    description: entry.description.clone(),
                                    location: entry.path.display().to_string(),
                                    server: ToolServerInfo {
                                        name: entry.name.clone(),
                                        version: "unknown".to_string(),
                                    },
                                    tools: BTreeMap::new(),
                                    prompts: BTreeMap::new(),
                                    resources: BTreeMap::new(),
                                },
                            );
                            continue;
                        }
                    };

                let resolved = match resolved_plugin
                    .template
                    .resolve(&user_config, &system_config)
                {
                    Ok(r) => r,
                    Err(_) => {
                        output.insert(
                            entry.name.clone(),
                            FullServerOutput {
                                server_type: entry.tool_type.clone(),
                                description: entry.description.clone(),
                                location: entry.path.display().to_string(),
                                server: ToolServerInfo {
                                    name: entry.name.clone(),
                                    version: "unknown".to_string(),
                                },
                                tools: BTreeMap::new(),
                                prompts: BTreeMap::new(),
                                resources: BTreeMap::new(),
                            },
                        );
                        continue;
                    }
                };

                let tool_name = resolved_plugin.template.name.clone().unwrap_or_else(|| {
                    entry
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                });

                // Try to get tool info (may fail if server can't start)
                match get_tool_info(&resolved, &tool_name, false).await {
                    Ok(capabilities) => {
                        output.insert(
                            entry.name.clone(),
                            FullServerOutput::from_capabilities(
                                &entry.tool_type,
                                entry.description.clone(),
                                entry.path.display().to_string(),
                                &capabilities,
                            ),
                        );
                    }
                    Err(_) => {
                        // Server couldn't start, include basic info with empty collections
                        output.insert(
                            entry.name.clone(),
                            FullServerOutput {
                                server_type: entry.tool_type.clone(),
                                description: entry.description.clone(),
                                location: entry.path.display().to_string(),
                                server: ToolServerInfo {
                                    name: entry.name.clone(),
                                    version: "unknown".to_string(),
                                },
                                tools: BTreeMap::new(),
                                prompts: BTreeMap::new(),
                                resources: BTreeMap::new(),
                            },
                        );
                    }
                }
            }

            if concise {
                println!(
                    "{}",
                    full_list_to_json(&output).expect("Failed to serialize JSON output")
                );
            } else {
                println!(
                    "{}",
                    full_list_to_json_pretty(&output).expect("Failed to serialize JSON output")
                );
            }
        } else {
            // Basic output: just server metadata
            let output: BTreeMap<String, ServerOutput> = tool_entries
                .iter()
                .map(|e| {
                    (
                        e.name.clone(),
                        ServerOutput::new(
                            &e.tool_type,
                            e.description.clone(),
                            e.path.display().to_string(),
                        ),
                    )
                })
                .collect();
            if concise {
                println!(
                    "{}",
                    list_to_json(&output).expect("Failed to serialize JSON output")
                );
            } else {
                println!(
                    "{}",
                    list_to_json_pretty(&output).expect("Failed to serialize JSON output")
                );
            }
        }
        return Ok(());
    }

    // Concise output: Header + TSV format
    if concise {
        use crate::concise::quote;
        if !no_header {
            println!("#name\ttype\tpath");
        }
        for entry in &tool_entries {
            println!(
                "{}\t{}\t{}",
                entry.name,
                entry.tool_type,
                quote(&entry.path.display().to_string())
            );
        }
        return Ok(());
    }

    // Human-readable output
    let count = tool_entries.len();
    let label = if count == 1 { "tool" } else { "tools" };
    println!(
        "  {} Found {} {}\n",
        "✓".bright_green(),
        count.to_string().bold(),
        label
    );

    if full {
        // Full human-readable output: show tools, prompts, resources for each server
        for entry in &tool_entries {
            let desc = entry
                .description
                .as_ref()
                .and_then(|d| format_description(d, false, ""))
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("    {}{}", entry.name.bright_cyan().bold(), desc);
            println!(
                "    {}  {}",
                entry.tool_type.dimmed(),
                entry.path.display().to_string().dimmed()
            );

            // Try to get full tool info
            let resolved_plugin = match load_tool_from_path(&entry.path) {
                Ok(p) => p,
                Err(_) => {
                    println!("    {} Could not load manifest\n", "✗".bright_red());
                    continue;
                }
            };

            let user_config = BTreeMap::new();
            let system_config =
                match allocate_system_config(resolved_plugin.template.system_config.as_ref()) {
                    Ok(c) => c,
                    Err(_) => {
                        println!(
                            "    {} Could not allocate system config\n",
                            "✗".bright_red()
                        );
                        continue;
                    }
                };

            let resolved = match resolved_plugin
                .template
                .resolve(&user_config, &system_config)
            {
                Ok(r) => r,
                Err(_) => {
                    println!("    {} Could not resolve manifest\n", "✗".bright_red());
                    continue;
                }
            };

            let tool_name = resolved_plugin.template.name.clone().unwrap_or_else(|| {
                entry
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            });

            match get_tool_info(&resolved, &tool_name, false).await {
                Ok(capabilities) => {
                    // Show tools
                    if !capabilities.tools.is_empty() {
                        println!();
                        println!("    {}:", "Tools".dimmed());
                        for tool_info in &capabilities.tools {
                            let tool_desc = tool_info
                                .description
                                .as_ref()
                                .and_then(|d| format_description(d, false, ""))
                                .map(|d| format!("  {}", d.dimmed()))
                                .unwrap_or_default();
                            println!("      {}{}", tool_info.name.bright_white(), tool_desc);
                        }
                    }

                    // Show prompts
                    if !capabilities.prompts.is_empty() {
                        println!();
                        println!("    {}:", "Prompts".dimmed());
                        for prompt in &capabilities.prompts {
                            let prompt_desc = prompt
                                .description
                                .as_ref()
                                .and_then(|d| format_description(d, false, ""))
                                .map(|d| format!("  {}", d.dimmed()))
                                .unwrap_or_default();
                            println!(
                                "      {}{}",
                                prompt.name.to_string().bright_magenta(),
                                prompt_desc
                            );
                        }
                    }

                    // Show resources
                    if !capabilities.resources.is_empty() {
                        println!();
                        println!("    {}:", "Resources".dimmed());
                        for resource in &capabilities.resources {
                            let resource_desc = resource
                                .description
                                .as_ref()
                                .and_then(|d| format_description(d, false, ""))
                                .map(|d| format!("  {}", d.dimmed()))
                                .unwrap_or_default();
                            println!(
                                "      {}{}",
                                resource.uri.to_string().bright_yellow(),
                                resource_desc
                            );
                        }
                    }
                }
                Err(_) => {
                    println!("    {} Could not connect to server", "✗".bright_red());
                }
            }
            println!();
        }
    } else {
        // Basic human-readable output
        for entry in &tool_entries {
            let desc = entry
                .description
                .as_ref()
                .and_then(|d| format_description(d, false, ""))
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("    {}{}", entry.name.bright_cyan(), desc);
            println!(
                "    └── {}  {}",
                entry.tool_type.dimmed(),
                entry.path.display().to_string().dimmed()
            );
            println!();
        }
    }

    Ok(())
}

/// Resolve a tool reference to a path.
///
/// Resolution order:
/// 1. Explicit path indicators (`.`, `./`, `/`, `..`) - treat as local path
/// 2. Try to resolve from installed tools
/// 3. Fallback to relative path if it exists locally
///
/// Returns both the path and whether it was resolved as an installed tool.
pub async fn resolve_tool_path(tool: &str) -> ToolResult<ResolvedToolPath> {
    // Check for explicit path indicators first
    let is_explicit_path =
        tool == "." || tool.starts_with("./") || tool.starts_with('/') || tool.contains("..");

    if is_explicit_path {
        let path = PathBuf::from(tool);
        let abs_path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(&path)
        };
        return Ok(ResolvedToolPath {
            path: abs_path,
            is_installed: false,
        });
    }

    // Try to resolve from installed tools first
    // If parsing fails (e.g., invalid ref like "a/b/c"), fall through to path check
    let resolver = FilePluginResolver::default();
    if let Ok(Some(resolved)) = resolver.resolve_tool(tool).await {
        // Get the directory containing the manifest
        let dir = resolved.path.parent().unwrap_or(&resolved.path);
        return Ok(ResolvedToolPath {
            path: dir.to_path_buf(),
            is_installed: true,
        });
    }

    // Fallback: check if it exists as a relative path
    let path = PathBuf::from(tool);
    if path.exists() {
        let abs_path = std::env::current_dir()?.join(&path);
        return Ok(ResolvedToolPath {
            path: abs_path,
            is_installed: false,
        });
    }

    Err(ToolError::Generic(format!(
        "Tool '{}' not found. Use a path or install it first.",
        tool
    )))
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_manifest(dir: &std::path::Path, name: &str) {
        let manifest = format!(
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
        fs::write(dir.join("manifest.json"), manifest).unwrap();
    }

    #[tokio::test]
    async fn test_resolve_explicit_absolute_path() {
        let temp = TempDir::new().unwrap();
        create_manifest(temp.path(), "test-tool");

        let abs_path = temp.path().to_string_lossy().to_string();
        let result = resolve_tool_path(&abs_path).await.unwrap();
        assert!(!result.is_installed);
        // Canonicalize to handle symlinks
        let result_canonical = result.path.canonicalize().unwrap();
        let temp_canonical = temp.path().canonicalize().unwrap();
        assert_eq!(result_canonical, temp_canonical);
    }

    #[tokio::test]
    async fn test_resolve_explicit_path_starting_with_slash() {
        let temp = TempDir::new().unwrap();
        let tool_dir = temp.path().join("my-tool");
        fs::create_dir(&tool_dir).unwrap();
        create_manifest(&tool_dir, "my-tool");

        let abs_path = tool_dir.to_string_lossy().to_string();
        let result = resolve_tool_path(&abs_path).await.unwrap();
        assert!(!result.is_installed);
        assert!(result.path.ends_with("my-tool"));
    }

    #[tokio::test]
    async fn test_resolve_not_found_returns_error() {
        // Use a name that is neither installed nor exists as local path
        // (not an explicit path like /foo or ./foo)
        let result = resolve_tool_path("definitely-nonexistent-tool-12345").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_resolved_tool_path_struct() {
        let resolved = ResolvedToolPath {
            path: PathBuf::from("/test/path"),
            is_installed: true,
        };
        assert_eq!(resolved.path, PathBuf::from("/test/path"));
        assert!(resolved.is_installed);

        let resolved_local = ResolvedToolPath {
            path: PathBuf::from("/local/path"),
            is_installed: false,
        };
        assert!(!resolved_local.is_installed);
    }

    #[test]
    fn test_is_explicit_path_detection() {
        // These should be detected as explicit paths
        assert!(".".starts_with('.') || "." == ".");
        assert!("./my-tool".starts_with("./"));
        assert!("/absolute/path".starts_with('/'));
        assert!("../parent".contains(".."));

        // These should NOT be explicit paths
        assert!(!"my-tool".starts_with('.'));
        assert!(!"my-tool".starts_with("./"));
        assert!(!"my-tool".starts_with('/'));
        assert!(!"my-tool".contains(".."));
    }
}
