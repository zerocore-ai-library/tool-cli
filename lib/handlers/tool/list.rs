//! Tool list command handlers.

use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::resolver::FilePluginResolver;
use colored::Colorize;
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

    // JSON output
    if json_output {
        let output: Vec<_> = tool_entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "name": e.name,
                    "type": e.tool_type,
                    "description": e.description,
                    "location": e.path.display().to_string(),
                })
            })
            .collect();
        if concise {
            println!(
                "{}",
                serde_json::to_string(&output).expect("Failed to serialize JSON output")
            );
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&output).expect("Failed to serialize JSON output")
            );
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
    let resolver = FilePluginResolver::default();
    if let Some(resolved) = resolver.resolve_tool(tool).await? {
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
