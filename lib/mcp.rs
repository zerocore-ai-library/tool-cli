//! MCP client for tool connections.
//!
//! This module provides a simple MCP client that connects to tools via stdio or HTTP.

use colored::Colorize;

use crate::error::{ToolError, ToolResult};
use crate::mcpb::{McpbManifest, McpbTransport, ResolvedMcpbManifest};
use rmcp::model::{CallToolRequestParam, CallToolResult, ClientInfo, Tool};
use rmcp::service::RunningService;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::TokioChildProcess;
use rmcp::transport::auth::AuthClient;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{RoleClient, serve_client};
use std::collections::BTreeMap;
use std::process::{Child, Stdio};
use std::time::Duration;
use tokio::process::Command;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Server information from MCP initialize response.
#[derive(Debug, Clone)]
pub struct ServerInfo {
    /// Server name.
    pub name: String,
    /// Server version.
    pub version: String,
}

/// Result of listing tool capabilities.
#[derive(Debug, Clone)]
pub struct ToolCapabilities {
    /// Server info from initialize.
    pub server_info: ServerInfo,
    /// Available tools.
    pub tools: Vec<Tool>,
    /// Available prompts.
    pub prompts: Vec<rmcp::model::Prompt>,
    /// Available resources.
    pub resources: Vec<rmcp::model::Resource>,
}

/// Result of calling a tool method.
#[derive(Debug)]
pub struct ToolCallResult {
    /// Raw result from MCP.
    pub result: CallToolResult,
}

/// Tool type for display purposes.
#[derive(Debug, Clone, Copy)]
pub enum ToolType {
    /// Stdio transport (local process).
    Stdio,
    /// HTTP transport (remote server).
    Http,
}

impl std::fmt::Display for ToolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolType::Stdio => write!(f, "manifest.json (stdio)"),
            ToolType::Http => write!(f, "manifest.json (http)"),
        }
    }
}

/// Result of attempting to connect to an MCP server.
#[allow(clippy::large_enum_variant)]
pub enum ConnectResult {
    /// Connected successfully.
    Connected(McpConnection),
    /// OAuth required - use AuthSession to complete authentication.
    AuthRequired {
        /// The auth session for completing OAuth.
        session: crate::oauth::AuthSession,
        /// Spawned server process (if any) - kept alive during OAuth.
        spawned_server: Option<Child>,
    },
}

/// Active MCP connection that manages child process lifecycle.
pub struct McpConnection {
    /// The MCP client for making requests.
    pub client: RunningService<RoleClient, ClientInfo>,
    /// Spawned child process (killed on drop).
    child: Option<Child>,
    /// Process group ID (Unix only) - used to kill entire process group.
    #[cfg(unix)]
    pgid: Option<i32>,
}

impl Drop for McpConnection {
    fn drop(&mut self) {
        // On Unix, kill the entire process group to ensure all child processes die
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            // Send SIGTERM to entire process group (negative PID = process group)
            unsafe {
                libc::kill(-pgid, libc::SIGTERM);
            }
        }

        // Also kill the child directly and reap it
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl McpConnection {
    /// Get the peer for making MCP requests.
    pub fn peer(&self) -> &rmcp::service::Peer<RoleClient> {
        self.client.peer()
    }

    /// Get peer info from initialize response.
    pub fn peer_info(&self) -> Option<&rmcp::model::InitializeResult> {
        self.client.peer_info()
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Check if the entry point exists and return a helpful error if not.
fn check_entry_point_exists(resolved: &ResolvedMcpbManifest) -> ToolResult<()> {
    // Skip check for reference mode (no entry_point)
    if resolved.is_reference {
        return Ok(());
    }

    // Get the bundle path
    let Some(bundle_path) = resolved.manifest.bundle_path.as_ref() else {
        return Ok(()); // Can't check without bundle path
    };

    // Get entry point and check if it exists
    let Some(entry_point) = &resolved.manifest.server.entry_point else {
        return Ok(()); // No entry point defined
    };

    let entry_path = bundle_path.join(entry_point);
    if entry_path.exists() {
        return Ok(());
    }

    // Entry point doesn't exist - return structured error
    let build_script = resolved.manifest.scripts().and_then(|s| s.build.clone());

    Err(ToolError::EntryPointNotFound {
        entry_point: entry_point.clone(),
        full_path: entry_path.display().to_string(),
        build_script,
        bundle_path: bundle_path.display().to_string(),
    })
}

/// Connect to an MCP server based on resolved manifest configuration.
///
/// Returns `ConnectResult::Connected` on success, or `ConnectResult::AuthRequired`
/// if OAuth authentication is needed.
pub async fn connect(resolved: &ResolvedMcpbManifest, verbose: bool) -> ToolResult<ConnectResult> {
    // Check entry point exists before attempting to connect
    check_entry_point_exists(resolved)?;

    match resolved.transport {
        McpbTransport::Stdio => {
            // Stdio connections don't require OAuth
            let conn = connect_stdio(resolved, verbose).await?;
            Ok(ConnectResult::Connected(conn))
        }
        McpbTransport::Http => {
            // For HTTP, we need to spawn the server first if it's a bundle
            if resolved.is_reference {
                connect_http_direct(resolved, verbose).await
            } else {
                connect_http_spawned(resolved, verbose).await
            }
        }
    }
}

/// Connect via stdio transport.
async fn connect_stdio(
    resolved: &ResolvedMcpbManifest,
    verbose: bool,
) -> ToolResult<McpConnection> {
    let command = resolved.mcp_config.command.as_ref().ok_or_else(|| {
        ToolError::Generic("stdio transport requires 'command' in mcp_config".into())
    })?;

    let args = &resolved.mcp_config.args;
    let env = &resolved.mcp_config.env;

    if verbose {
        eprintln!("Spawning: {} {:?}", command, args);
    }

    // Build the command
    let mut cmd = Command::new(command);
    cmd.args(args)
        .envs(env.iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());

    // Suppress child process tracing output unless verbose
    if !verbose {
        cmd.env("RUST_LOG", "off");
    }

    // Set working directory if bundle_path is available
    if let Some(ref bundle_path) = resolved.manifest.bundle_path {
        cmd.current_dir(bundle_path);
    }

    // Use builder to control stderr - TokioChildProcess::new() ignores Command's stderr setting
    let mut builder = TokioChildProcess::builder(cmd);
    if verbose {
        builder = builder.stderr(Stdio::inherit());
    } else {
        builder = builder.stderr(Stdio::null());
    }
    let (transport, _stderr) = builder
        .spawn()
        .map_err(|e| ToolError::Generic(format!("Failed to create transport: {}", e)))?;

    let client_info = ClientInfo::default();
    let client = serve_client(client_info, transport)
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to connect to MCP server: {}", e)))?;

    if verbose && let Some(info) = client.peer_info() {
        eprintln!(
            "Connected: {} v{}",
            info.server_info.name, info.server_info.version
        );
    }

    Ok(McpConnection {
        client,
        child: None,
        #[cfg(unix)]
        pgid: None,
    })
}

/// Check if an error indicates OAuth authentication is required.
fn is_auth_error(err: &(impl std::fmt::Debug + std::fmt::Display)) -> bool {
    let err_debug = format!("{:?}", err);
    let err_display = err.to_string();
    let combined = format!("{} {}", err_debug, err_display).to_lowercase();
    combined.contains("auth required") || combined.contains("401")
}

/// Connect directly to an HTTP MCP server (reference mode).
async fn connect_http_direct(
    resolved: &ResolvedMcpbManifest,
    verbose: bool,
) -> ToolResult<ConnectResult> {
    let url =
        resolved.mcp_config.url.as_ref().ok_or_else(|| {
            ToolError::Generic("HTTP transport requires 'url' in mcp_config".into())
        })?;

    if verbose {
        eprintln!("Connecting to: {}", url);
    }

    // Use rmcp's HTTP transport
    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client_info = ClientInfo::default();

    match serve_client(client_info, transport).await {
        Ok(client) => {
            if verbose && let Some(info) = client.peer_info() {
                eprintln!(
                    "Connected: {} v{}",
                    info.server_info.name, info.server_info.version
                );
            }
            Ok(ConnectResult::Connected(McpConnection {
                client,
                child: None,
                #[cfg(unix)]
                pgid: None,
            }))
        }
        Err(e) => {
            if is_auth_error(&e) {
                // Check if we have crypto configured before preparing auth session
                if crate::security::get_credential_crypto().is_none() {
                    return Err(ToolError::OAuthNotConfigured);
                }

                if verbose {
                    eprintln!("Server requires OAuth, preparing auth session...");
                }

                // Prepare auth session
                let session = crate::oauth::prepare_auth_session(
                    url,
                    resolved.mcp_config.oauth_config.clone(),
                )
                .await?;
                Ok(ConnectResult::AuthRequired {
                    session,
                    spawned_server: None,
                })
            } else {
                Err(ToolError::Generic(format!(
                    "Failed to connect to HTTP MCP server: {}",
                    e
                )))
            }
        }
    }
}

/// Spawn HTTP server and connect (bundle mode).
async fn connect_http_spawned(
    resolved: &ResolvedMcpbManifest,
    verbose: bool,
) -> ToolResult<ConnectResult> {
    let command =
        resolved.mcp_config.command.as_ref().ok_or_else(|| {
            ToolError::Generic("Bundle HTTP requires 'command' in mcp_config".into())
        })?;

    let url = resolved
        .mcp_config
        .url
        .as_ref()
        .ok_or_else(|| ToolError::Generic("Bundle HTTP requires 'url' in mcp_config".into()))?;

    let args = &resolved.mcp_config.args;
    let env = &resolved.mcp_config.env;

    if verbose {
        eprintln!("Spawning: {} {:?}", command, args);
    }

    // Build and spawn the command in its own process group
    let mut cmd = std::process::Command::new(command);
    cmd.args(args)
        .envs(env.iter())
        .stdin(Stdio::null())
        .stdout(if verbose {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stderr(if verbose {
            Stdio::inherit()
        } else {
            Stdio::piped()
        });

    // Suppress child process tracing output unless verbose
    if !verbose {
        cmd.env("RUST_LOG", "off");
    }

    // Set working directory if bundle_path is available
    if let Some(ref bundle_path) = resolved.manifest.bundle_path {
        cmd.current_dir(bundle_path);
    }

    // On Unix, spawn in its own process group so we can kill the entire tree
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd
        .spawn()
        .map_err(|e| ToolError::Generic(format!("Failed to spawn HTTP server: {}", e)))?;

    // On Unix, the pgid equals the child's pid when process_group(0) is used
    #[cfg(unix)]
    let pgid = Some(child.id() as i32);

    if verbose {
        eprintln!("Spawned process PID: {}", child.id());
    }

    // Wait for server to be ready
    if verbose {
        eprintln!("Waiting for server at {}...", url);
    }

    wait_for_server_ready(url, &mut child, Duration::from_secs(30), verbose).await?;

    if verbose {
        eprintln!("Server ready at {}", url);
    }

    // Connect via HTTP
    let transport = StreamableHttpClientTransport::from_uri(url.as_str());
    let client_info = ClientInfo::default();

    match serve_client(client_info, transport).await {
        Ok(client) => {
            if verbose && let Some(info) = client.peer_info() {
                eprintln!(
                    "Connected: {} v{}",
                    info.server_info.name, info.server_info.version
                );
            }
            Ok(ConnectResult::Connected(McpConnection {
                client,
                child: Some(child),
                #[cfg(unix)]
                pgid,
            }))
        }
        Err(e) => {
            if is_auth_error(&e) {
                // Check if we have crypto configured before preparing auth session
                if crate::security::get_credential_crypto().is_none() {
                    // Kill child process before returning
                    let _ = child.kill();
                    return Err(ToolError::OAuthNotConfigured);
                }

                if verbose {
                    eprintln!("Server requires OAuth, preparing auth session...");
                }

                // Prepare auth session - keep child alive for OAuth
                let session = crate::oauth::prepare_auth_session(
                    url,
                    resolved.mcp_config.oauth_config.clone(),
                )
                .await?;
                Ok(ConnectResult::AuthRequired {
                    session,
                    spawned_server: Some(child),
                })
            } else {
                Err(ToolError::Generic(format!(
                    "Failed to connect to HTTP MCP server: {}",
                    e
                )))
            }
        }
    }
}

/// Wait for HTTP server to be ready by polling the URL.
/// Also monitors the child process to detect early crashes.
async fn wait_for_server_ready(
    url: &str,
    child: &mut Child,
    timeout: Duration,
    verbose: bool,
) -> ToolResult<()> {
    let start = std::time::Instant::now();
    let client = reqwest::Client::new();
    let mut attempts: u32 = 0;

    // Extract base URL (remove /mcp path for health check)
    let health_url = url.trim_end_matches('/').trim_end_matches("/mcp");

    loop {
        if start.elapsed() > timeout {
            return Err(ToolError::Generic(format!(
                "Server failed to start within {} seconds",
                timeout.as_secs()
            )));
        }

        // Check if the child process has crashed
        match child.try_wait() {
            Ok(Some(status)) => {
                // Read stderr to get the actual error message
                let stderr_output = read_child_stderr(child);
                let error_detail = if let Some(output) = stderr_output {
                    format!("\n{}", output.trim())
                } else {
                    String::new()
                };
                return Err(ToolError::Generic(format!(
                    "Server process exited unexpectedly with {}{}",
                    status, error_detail
                )));
            }
            Ok(None) => {
                // Process still running, continue polling
            }
            Err(e) => {
                return Err(ToolError::Generic(format!(
                    "Failed to check server process status: {}",
                    e
                )));
            }
        }

        // Try to connect - any response (even 404) means server is up
        match client.get(health_url).send().await {
            Ok(_) => {
                // Final check: ensure our spawned process is still running
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Read stderr to get the actual error message
                        let stderr_output = read_child_stderr(child);
                        let error_detail = if let Some(output) = stderr_output {
                            format!("\n{}", output.trim())
                        } else {
                            String::new()
                        };
                        return Err(ToolError::Generic(format!(
                            "Server process exited with {} but another server is running on the port.{}\n\
                             Kill the existing process or use a different port.",
                            status, error_detail
                        )));
                    }
                    Ok(None) => return Ok(()), // Process alive, response is from our server
                    Err(e) => {
                        return Err(ToolError::Generic(format!(
                            "Failed to check server process status: {}",
                            e
                        )));
                    }
                }
            }
            Err(e) => {
                if verbose {
                    eprintln!("Waiting for server... ({})", e);
                }
                // Exponential backoff: 10ms, 20ms, 40ms, 80ms, 160ms, 320ms, 500ms (capped)
                let delay_ms = (10 * 2u64.pow(attempts.min(5))).min(500);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                attempts += 1;
            }
        }
    }
}

/// Read stderr from a child process (if available and piped).
fn read_child_stderr(child: &mut Child) -> Option<String> {
    use std::io::Read;
    child.stderr.take().and_then(|mut stderr| {
        let mut output = String::new();
        stderr.read_to_string(&mut output).ok()?;
        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    })
}

/// Get tool type from manifest.
pub fn get_tool_type(manifest: &McpbManifest) -> ToolType {
    match manifest.server.transport {
        McpbTransport::Stdio => ToolType::Stdio,
        McpbTransport::Http => ToolType::Http,
    }
}

/// Connect with OAuth support.
///
/// This function handles the OAuth flow if authentication is required.
/// It will open a browser for the user to authenticate and then retry the connection.
pub async fn connect_with_oauth(
    resolved: &ResolvedMcpbManifest,
    tool_ref: &str,
    verbose: bool,
) -> ToolResult<McpConnection> {
    use crate::oauth::{OAuthFlowOptions, run_interactive_oauth};
    use crate::security::{CredentialCrypto, EnvSecretProvider};

    match connect(resolved, verbose).await? {
        ConnectResult::Connected(conn) => Ok(conn),
        ConnectResult::AuthRequired {
            session,
            spawned_server,
        } => {
            // Check if CREDENTIALS_SECRET_KEY is set
            let provider = EnvSecretProvider::new().map_err(|_| ToolError::AuthRequired {
                tool_ref: tool_ref.to_string(),
            })?;

            let key = provider
                .get_encryption_key("default")
                .await
                .map_err(|e| ToolError::Generic(format!("Failed to get encryption key: {}", e)))?;

            let crypto = CredentialCrypto::new(&key);

            eprintln!(
                "  {} Authenticating with {}...\n",
                "→".bright_blue(),
                tool_ref.bold()
            );

            // Store the server URL before OAuth - we'll need it for the retry
            let server_url = session.url().to_string();

            let options = OAuthFlowOptions::default();
            let _credentials = run_interactive_oauth(session, tool_ref, crypto, options).await?;

            if verbose {
                eprintln!("OAuth completed, credentials saved");
            }

            // Retry with new credentials - use reconnect_http with stored credentials
            let new_credentials = crate::oauth::load_credentials(tool_ref)
                .await
                .ok()
                .flatten();
            reconnect_http(
                &server_url,
                tool_ref,
                new_credentials.as_ref(),
                spawned_server,
                verbose,
            )
            .await
        }
    }
}

/// Reconnect to an HTTP server with stored credentials.
pub async fn reconnect_http(
    url: &str,
    tool_ref: &str,
    credentials: Option<&crate::oauth::OAuthCredentials>,
    spawned_server: Option<Child>,
    verbose: bool,
) -> ToolResult<McpConnection> {
    // Extract child and pgid from spawned server
    #[cfg(unix)]
    let (child, pgid) = if let Some(server) = spawned_server {
        let pid = server.id() as i32;
        let pgid = unsafe { libc::getpgid(pid) };
        let pgid = if pgid > 0 { Some(pgid) } else { None };
        (Some(server), pgid)
    } else {
        (None, None)
    };

    #[cfg(not(unix))]
    let child = spawned_server;

    // If we have credentials, connect with AuthClient
    if credentials.is_some() {
        if verbose {
            eprintln!("Attempting connection with stored credentials...");
        }

        // Create authenticated transport using stored credentials
        let crypto = crate::security::get_credential_crypto()
            .ok_or_else(|| ToolError::Generic("CREDENTIALS_SECRET_KEY not set".to_string()))?;
        let store = crate::security::FileCredentialStore::new(tool_ref, crypto);

        // Create a new AuthorizationManager and initialize from store
        let mut manager = rmcp::transport::auth::AuthorizationManager::new(url)
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to initialize OAuth: {}", e)))?;

        manager.set_credential_store(store);

        let has_creds = manager
            .initialize_from_store()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to load credentials: {}", e)))?;

        if has_creds {
            let auth_client = AuthClient::new(reqwest::Client::default(), manager);
            let config = StreamableHttpClientTransportConfig::with_uri(url);
            let transport = StreamableHttpClientTransport::with_client(auth_client, config);
            let client_info = ClientInfo::default();

            match serve_client(client_info, transport).await {
                Ok(client) => {
                    if verbose {
                        eprintln!("Connected with stored credentials");
                    }
                    return Ok(McpConnection {
                        client,
                        child,
                        #[cfg(unix)]
                        pgid,
                    });
                }
                Err(e) => {
                    return Err(ToolError::Generic(format!(
                        "OAuth completed but connection failed: {}",
                        e
                    )));
                }
            }
        }
    }

    Err(ToolError::Generic(
        "OAuth completed but still not authenticated".to_string(),
    ))
}

/// Get tool capabilities from a resolved manifest.
pub async fn get_tool_info(
    resolved: &ResolvedMcpbManifest,
    tool_name: &str,
    verbose: bool,
) -> ToolResult<ToolCapabilities> {
    let connection = connect_with_oauth(resolved, tool_name, verbose).await?;

    // Get server info
    let server_info = connection
        .peer_info()
        .map(|info| ServerInfo {
            name: info.server_info.name.clone(),
            version: info.server_info.version.clone(),
        })
        .unwrap_or_else(|| ServerInfo {
            name: "unknown".to_string(),
            version: "0.0.0".to_string(),
        });

    if verbose {
        eprintln!("Connected: {} v{}", server_info.name, server_info.version);
    }

    // List tools
    if verbose {
        eprintln!("-> tools/list");
    }
    let tools_response = connection
        .peer()
        .list_tools(None)
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to list tools: {}", e)))?;
    if verbose {
        eprintln!("<- {} tool(s)", tools_response.tools.len());
    }

    // List prompts
    if verbose {
        eprintln!("-> prompts/list");
    }
    let prompts = match connection.peer().list_prompts(None).await {
        Ok(response) => {
            if verbose {
                eprintln!("<- {} prompt(s)", response.prompts.len());
            }
            response.prompts
        }
        Err(_) => {
            if verbose {
                eprintln!("<- prompts not supported");
            }
            Vec::new()
        }
    };

    // List resources
    if verbose {
        eprintln!("-> resources/list");
    }
    let resources = match connection.peer().list_resources(None).await {
        Ok(response) => {
            if verbose {
                eprintln!("<- {} resource(s)", response.resources.len());
            }
            response.resources
        }
        Err(_) => {
            if verbose {
                eprintln!("<- resources not supported");
            }
            Vec::new()
        }
    };

    Ok(ToolCapabilities {
        server_info,
        tools: tools_response.tools,
        prompts,
        resources,
    })
}

/// Call a tool method using a resolved manifest.
pub async fn call_tool(
    resolved: &ResolvedMcpbManifest,
    tool_name: &str,
    method: &str,
    arguments: BTreeMap<String, serde_json::Value>,
    verbose: bool,
) -> ToolResult<ToolCallResult> {
    let connection = connect_with_oauth(resolved, tool_name, verbose).await?;

    if verbose && let Some(info) = connection.peer_info() {
        eprintln!(
            "Connected: {} v{}",
            info.server_info.name, info.server_info.version
        );
    }

    // Call the tool
    let params = CallToolRequestParam {
        name: method.to_string().into(),
        arguments: if arguments.is_empty() {
            None
        } else {
            Some(arguments.into_iter().collect())
        },
    };

    if verbose {
        eprintln!("-> tools/call: {}", method);
    }

    let result = connection
        .peer()
        .call_tool(params)
        .await
        .map_err(|e| ToolError::Generic(format!("Tool call failed: {}", e)))?;

    if verbose {
        eprintln!("<- {} content block(s)", result.content.len());
    }

    Ok(ToolCallResult { result })
}
