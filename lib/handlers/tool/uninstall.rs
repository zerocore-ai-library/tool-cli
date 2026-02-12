//! Tool uninstallation command handlers.

use crate::error::{ToolError, ToolResult};
use crate::resolver::FilePluginResolver;
use colored::Colorize;
use std::io::{self, Write};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Result of a single tool uninstallation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum UninstallResult {
    /// Successfully removed
    Removed,
    /// Tool not found
    NotFound,
    /// Removal failed
    Failed(String),
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Remove a single tool and return its result.
async fn remove_tool(name: &str) -> (String, UninstallResult) {
    use crate::constants::DEFAULT_TOOLS_PATH;
    use tokio::fs;

    let resolver = FilePluginResolver::default();

    // First, find the tool
    let resolved = match resolver.resolve_tool(name).await {
        Ok(Some(r)) => r,
        Ok(None) => return (name.to_string(), UninstallResult::NotFound),
        Err(e) => return (name.to_string(), UninstallResult::Failed(e.to_string())),
    };

    // Get the directory containing the tool
    let tool_dir = match resolved.path.parent() {
        Some(d) => d,
        None => {
            return (
                name.to_string(),
                UninstallResult::Failed("Failed to get tool directory".into()),
            );
        }
    };

    // Remove the directory
    if let Err(e) = fs::remove_dir_all(tool_dir).await {
        return (
            resolved.plugin_ref.to_string(),
            UninstallResult::Failed(format!("Failed to remove: {}", e)),
        );
    }

    // Clean up empty parent namespace directory if applicable
    if let Some(parent_dir) = tool_dir.parent() {
        // Only clean up if the parent is not the root tools directory
        if parent_dir != DEFAULT_TOOLS_PATH.as_path() {
            // Check if the parent directory is now empty
            let is_empty = std::fs::read_dir(parent_dir)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);

            if is_empty {
                // Remove the empty namespace directory
                let _ = std::fs::remove_dir(parent_dir);
            }
        }
    }

    (resolved.plugin_ref.to_string(), UninstallResult::Removed)
}

/// Remove multiple installed tools.
pub async fn remove_tools(names: &[String], all: bool, yes: bool) -> ToolResult<()> {
    use futures_util::future::join_all;

    let resolver = FilePluginResolver::default();

    // Get list of tools to remove and orphaned entries
    let (tools_to_remove, orphans) = if all {
        if !names.is_empty() {
            return Err(ToolError::Generic(
                "Cannot specify tool names with --all".into(),
            ));
        }
        let installed = resolver.list_tools().await?;
        let orphans = resolver.list_orphaned_entries()?;

        if installed.is_empty() && orphans.is_empty() {
            println!("\n  No tools installed.\n");
            return Ok(());
        }
        (
            installed.into_iter().map(|t| t.to_string()).collect(),
            orphans,
        )
    } else {
        if names.is_empty() {
            return Err(ToolError::Generic(
                "No tools specified. Use --all to remove all tools.".into(),
            ));
        }
        (names.to_vec(), Vec::new())
    };

    let total_items = tools_to_remove.len() + orphans.len();

    // Confirm if --all and not --yes
    if all && !yes && total_items > 0 {
        println!();
        if !tools_to_remove.is_empty() {
            println!(
                "  {} This will uninstall {} tool(s)",
                "!".bright_yellow(),
                tools_to_remove.len()
            );
        }
        if !orphans.is_empty() {
            println!(
                "  {} This will clean up {} orphaned {}",
                "!".bright_yellow(),
                orphans.len(),
                if orphans.len() == 1 {
                    "entry"
                } else {
                    "entries"
                }
            );
        }
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

    let mut removed_count = 0usize;
    let mut not_found_count = 0usize;
    let mut failed_count = 0usize;
    let mut orphans_cleaned = 0usize;

    // Remove tools
    if !tools_to_remove.is_empty() {
        let futures: Vec<_> = tools_to_remove
            .iter()
            .map(|name| remove_tool(name))
            .collect();
        let results = join_all(futures).await;

        // Print results
        for (tool_name, result) in &results {
            match result {
                UninstallResult::Removed => {
                    println!(
                        "  {} Removed {}",
                        "✓".bright_green(),
                        tool_name.bright_cyan()
                    );
                    removed_count += 1;
                }
                UninstallResult::NotFound => {
                    println!(
                        "  {} Tool {} not found",
                        "✗".bright_red(),
                        tool_name.bright_white().bold()
                    );
                    not_found_count += 1;
                }
                UninstallResult::Failed(msg) => {
                    println!("  {} {}: {}", "✗".bright_red(), tool_name, msg);
                    failed_count += 1;
                }
            }
        }
    }

    // Clean up orphaned entries
    for orphan_path in &orphans {
        let display_name = orphan_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| orphan_path.display().to_string());

        let result = if orphan_path.is_symlink() {
            // Remove broken symlink
            std::fs::remove_file(orphan_path)
        } else {
            // Remove directory
            std::fs::remove_dir_all(orphan_path)
        };

        match result {
            Ok(()) => {
                println!(
                    "  {} Cleaned up {}",
                    "✓".bright_green(),
                    display_name.bright_yellow()
                );
                orphans_cleaned += 1;
            }
            Err(e) => {
                println!(
                    "  {} Failed to clean up {}: {}",
                    "✗".bright_red(),
                    display_name,
                    e
                );
                failed_count += 1;
            }
        }
    }

    // Print summary if multiple items were processed
    if total_items > 1 {
        println!();
        if removed_count > 0 {
            println!(
                "  Removed {} {}",
                removed_count.to_string().bright_green(),
                if removed_count == 1 {
                    "package"
                } else {
                    "packages"
                }
            );
        }
        if orphans_cleaned > 0 {
            println!(
                "  Cleaned up {} orphaned {}",
                orphans_cleaned.to_string().bright_green(),
                if orphans_cleaned == 1 {
                    "entry"
                } else {
                    "entries"
                }
            );
        }
        if not_found_count > 0 {
            println!(
                "  Not found: {}",
                not_found_count.to_string().bright_yellow()
            );
        }
        if failed_count > 0 {
            println!("  Failed: {}", failed_count.to_string().bright_red());
        }
    }

    Ok(())
}
