//! Run command handler for proxy mode.

use colored::Colorize;

use crate::error::{ToolError, ToolResult};
use crate::mcp::connect_with_oauth;
use crate::proxy::{ExposeTransport, HttpExposeConfig, run_proxy};
use crate::resolver::load_tool_from_path;
use crate::system_config::allocate_system_config;

use super::call::{apply_user_config_defaults, parse_user_config, prompt_missing_user_config};
use super::config_cmd::{parse_tool_ref_for_config, save_tool_config};
use super::list::resolve_tool_path;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Run an MCP server in proxy mode.
#[allow(clippy::too_many_arguments)]
pub async fn tool_run(
    tool: String,
    expose: Option<String>,
    port: u16,
    host: String,
    config: Vec<String>,
    config_file: Option<String>,
    no_save: bool,
    yes: bool,
    verbose: bool,
) -> ToolResult<()> {
    // Parse expose transport
    let expose_transport = match expose.as_deref() {
        Some(t) => Some(t.parse::<ExposeTransport>().map_err(ToolError::Generic)?),
        None => None,
    };

    // Resolve tool path
    let tool_path = resolve_tool_path(&tool).await?;

    // Load manifest
    let resolved_plugin = load_tool_from_path(&tool_path)?;
    let manifest_schema = resolved_plugin.template.user_config.as_ref();

    // Parse user config
    let mut user_config =
        parse_user_config(&config, config_file.as_deref(), &tool, &resolved_plugin)?;

    // Prompt for missing required config values, then apply defaults
    prompt_missing_user_config(manifest_schema, &mut user_config, yes)?;
    apply_user_config_defaults(manifest_schema, &mut user_config);

    // Auto-save config for future use (unless --no-save)
    if !no_save
        && !user_config.is_empty()
        && let Ok(plugin_ref) = parse_tool_ref_for_config(&tool, &resolved_plugin)
    {
        let _ = save_tool_config(&plugin_ref, &user_config);
    }

    // Allocate system config
    let system_config = allocate_system_config(resolved_plugin.template.system_config.as_ref())?;

    // Resolve manifest with config
    let resolved = resolved_plugin
        .template
        .resolve(&user_config, &system_config)?;

    let backend_transport = resolved.transport;

    // Get tool name for OAuth
    let tool_name = resolved_plugin.template.name.clone().unwrap_or_else(|| {
        tool_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });

    if verbose {
        eprintln!(
            "  {} Connecting to backend MCP server...",
            "→".bright_blue()
        );
    }

    // Connect to backend
    let backend = connect_with_oauth(&resolved, &tool_name, verbose).await?;

    // Get server info for display
    let server_info = backend
        .peer_info()
        .map(|info| format!("{} v{}", info.server_info.name, info.server_info.version))
        .unwrap_or_else(|| "unknown".to_string());

    // Determine expose info for display
    let expose_str = match expose_transport {
        Some(ExposeTransport::Stdio) => "stdio".to_string(),
        Some(ExposeTransport::Http) => format!("http://{}:{}/mcp", host, port),
        None => match backend_transport {
            crate::mcpb::McpbTransport::Stdio => "stdio (native)".to_string(),
            crate::mcpb::McpbTransport::Http => format!("http://{}:{}/mcp (native)", host, port),
        },
    };

    eprintln!("  {} Proxy server ready\n", "✓".bright_green());
    eprintln!("    {}  {}", "Server".dimmed(), server_info.bright_white());
    eprintln!("    {}  {}", "Expose".dimmed(), expose_str.bright_white());
    eprintln!(
        "    {} {}",
        "Backend".dimmed(),
        match backend_transport {
            crate::mcpb::McpbTransport::Stdio => "stdio".to_string(),
            crate::mcpb::McpbTransport::Http => resolved
                .mcp_config
                .url
                .as_deref()
                .unwrap_or("http")
                .to_string(),
        }
    );
    eprintln!();

    // Run proxy
    let http_config = HttpExposeConfig { port, host };
    run_proxy(
        backend,
        expose_transport,
        http_config,
        backend_transport,
        verbose,
    )
    .await
}
