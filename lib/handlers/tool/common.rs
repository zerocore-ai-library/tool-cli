//! Common utilities for tool command handlers.

use std::path::PathBuf;

use crate::error::ToolResult;
use crate::mcpb::{McpbManifest, McpbTransport, ResolvedMcpbManifest};
use crate::resolver::{ResolvedPlugin, load_tool_from_path};
use crate::system_config::allocate_system_config;

use super::call::{apply_user_config_defaults, parse_user_config, prompt_missing_user_config};
use super::config_cmd::{parse_tool_ref_for_config, save_tool_config_with_schema};
use super::list::resolve_tool_path;

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
    let tool_path = resolve_tool_path(tool).await?;

    // Load manifest
    let resolved_plugin = load_tool_from_path(&tool_path)?;
    let manifest_schema = resolved_plugin.template.user_config.as_ref();

    // Parse user config from saved config, config file, and -k flags
    let mut user_config =
        parse_user_config(options.config, options.config_file, tool, &resolved_plugin)?;

    // Prompt for missing required config values, then apply defaults
    prompt_missing_user_config(manifest_schema, &mut user_config, options.yes)?;
    apply_user_config_defaults(manifest_schema, &mut user_config);

    // Auto-save config for future use (unless --no-save)
    if !options.no_save
        && !user_config.is_empty()
        && let Ok(plugin_ref) = parse_tool_ref_for_config(tool, &resolved_plugin)
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
