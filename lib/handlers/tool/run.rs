//! Run command handler for proxy mode.

use colored::Colorize;

use crate::error::{ToolError, ToolResult};
use crate::mcp::connect_with_oauth;
use crate::proxy::{ExposeTransport, HttpExposeConfig, run_proxy};

use super::common::{PrepareToolOptions, prepare_tool};

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

    let backend_transport = prepared.transport;

    if verbose {
        eprintln!(
            "  {} Connecting to backend MCP server...",
            "→".bright_blue()
        );
    }

    // Connect to backend
    // Never pass verbose to connection - verbose only affects output formatting
    let backend = connect_with_oauth(&prepared.resolved, &prepared.tool_name, false).await?;

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
            crate::mcpb::McpbTransport::Http => prepared
                .resolved
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
