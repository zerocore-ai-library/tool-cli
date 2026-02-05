//! Common utilities for tool command handlers.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use colored::Colorize;

use crate::error::{ToolError, ToolResult};
use crate::mcpb::{McpbManifest, McpbTransport, ResolvedMcpbManifest};
use crate::resolver::{ResolvedPlugin, load_tool_from_path};
use crate::system_config::allocate_system_config;

use super::call::{apply_user_config_defaults, parse_user_config, prompt_missing_user_config};
use super::config_cmd::{parse_tool_ref_for_config, save_tool_config_with_schema};
use super::list::resolve_tool_path;
use super::registry::{LinkResult, link_local_tool, link_local_tool_force};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A tool that has been resolved and is ready for connection.
pub struct PreparedTool {
    /// The resolved manifest with all variables substituted.
    pub resolved: ResolvedMcpbManifest,
    /// The tool name (from manifest or directory).
    pub tool_name: String,
    /// The path to the tool directory.
    pub tool_path: PathBuf,
    /// The path to the manifest file.
    pub manifest_path: PathBuf,
    /// The transport type (stdio or http).
    pub transport: McpbTransport,
    /// The original resolved plugin (for additional metadata).
    pub plugin: ResolvedPlugin<McpbManifest>,
}

/// Options for preparing a tool.
pub struct PrepareToolOptions<'a> {
    /// Configuration values from -k flags.
    pub config: &'a [String],
    /// Path to config file.
    pub config_file: Option<&'a str>,
    /// Skip auto-saving config.
    pub no_save: bool,
    /// Skip interactive prompts.
    pub yes: bool,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Prepare a tool for connection.
///
/// This consolidates the common setup logic shared by `tool call`, `tool info`, and `tool run`:
/// 1. Resolve tool path
/// 2. Load manifest
/// 3. Parse, prompt, and apply user config
/// 4. Auto-save config (unless disabled)
/// 5. Allocate system config and resolve manifest
/// 6. Extract tool name
pub async fn prepare_tool(tool: &str, options: PrepareToolOptions<'_>) -> ToolResult<PreparedTool> {
    // Resolve tool path
    let resolved_path = resolve_tool_path(tool).await?;
    let tool_path = resolved_path.path;
    let mut is_installed = resolved_path.is_installed;

    // Load manifest
    let resolved_plugin = load_tool_from_path(&tool_path)?;

    // Auto-install local path tools
    if !is_installed {
        is_installed = auto_install_local_tool(&tool_path, &resolved_plugin.template, options.yes)?;
    }

    let manifest_schema = resolved_plugin.template.user_config.as_ref();

    // Parse user config from saved config, config file, and -k flags
    let (mut user_config, has_saved_config) = parse_user_config(
        options.config,
        options.config_file,
        tool,
        &resolved_plugin,
        is_installed,
    )?;

    // Prompt for missing required config values, then apply defaults
    prompt_missing_user_config(
        manifest_schema,
        &mut user_config,
        options.yes,
        has_saved_config,
    )?;
    apply_user_config_defaults(manifest_schema, &mut user_config);

    // Auto-save config for future use (unless --no-save)
    if !options.no_save
        && !user_config.is_empty()
        && let Ok(plugin_ref) = parse_tool_ref_for_config(tool, &resolved_plugin, is_installed)
    {
        let _ = save_tool_config_with_schema(&plugin_ref, &user_config, manifest_schema);
    }

    // Allocate system config and resolve manifest
    let system_config = allocate_system_config(resolved_plugin.template.system_config.as_ref())?;
    let resolved = resolved_plugin
        .template
        .resolve(&user_config, &system_config)?;

    // Get transport type
    let transport = resolved.transport;

    // Get manifest path
    let manifest_path = resolved_plugin.path.clone();

    // Get tool name (from manifest or directory)
    let tool_name = resolved_plugin.template.name.clone().unwrap_or_else(|| {
        tool_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });

    Ok(PreparedTool {
        resolved,
        tool_name,
        tool_path,
        manifest_path,
        transport,
        plugin: resolved_plugin,
    })
}

/// Auto-install a local path tool if not already installed.
///
/// Creates a symlink in the tools directory. On conflict, prompts the user
/// to overwrite (or auto-overwrites if `yes` is true).
///
/// Returns `true` if the tool is now installed, `false` if it has no name
/// (and thus cannot be linked).
pub fn auto_install_local_tool(
    tool_path: &Path,
    manifest: &McpbManifest,
    yes: bool,
) -> ToolResult<bool> {
    let Some(tool_name) = &manifest.name else {
        return Ok(false);
    };

    let version = manifest.version.as_deref();
    let source_path = tool_path
        .canonicalize()
        .map_err(|e| ToolError::Generic(format!("Failed to resolve path: {}", e)))?;

    let display_name = match version {
        Some(v) => format!("{}@{}", tool_name, v),
        None => tool_name.clone(),
    };

    match link_local_tool(&source_path, tool_name, version)? {
        LinkResult::Linked => {
            println!(
                "  {} Linked {} from {}",
                "→".bright_blue(),
                display_name.bright_cyan(),
                source_path.display().to_string().dimmed()
            );
            Ok(true)
        }
        LinkResult::AlreadyLinked => Ok(true),
        LinkResult::Conflict(existing) => {
            println!(
                "  {} {} is linked to {}",
                "!".bright_yellow(),
                display_name,
                existing.display()
            );

            let confirmed = if yes {
                true
            } else {
                print!("  Overwrite? [y/N] ");
                io::stdout().flush().ok();

                let mut input = String::new();
                io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| ToolError::Generic(format!("Failed to read input: {}", e)))?;
                input.trim().eq_ignore_ascii_case("y")
            };

            if confirmed {
                link_local_tool_force(&source_path, tool_name, version)?;
                println!(
                    "  {} Linked {} from {}",
                    "→".bright_blue(),
                    display_name.bright_cyan(),
                    source_path.display().to_string().dimmed()
                );
                Ok(true)
            } else {
                Err(ToolError::Cancelled)
            }
        }
    }
}
