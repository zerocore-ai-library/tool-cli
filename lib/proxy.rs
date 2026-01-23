//! MCP proxy server implementation.
//!
//! Bridges between frontend clients and backend MCP servers with protocol translation.

use std::future::Future;
use std::sync::Arc;

use rmcp::model::*;
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::StreamableHttpService;
use rmcp::{ErrorData as McpError, ServerHandler, serve_server};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::error::{ToolError, ToolResult};
use crate::mcp::McpConnection;
use crate::mcpb::McpbTransport;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Transport type for the proxy expose side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposeTransport {
    /// Standard input/output.
    Stdio,
    /// HTTP server.
    Http,
}

impl std::str::FromStr for ExposeTransport {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stdio" => Ok(ExposeTransport::Stdio),
            "http" => Ok(ExposeTransport::Http),
            _ => Err(format!(
                "Unknown transport: '{}'. Use 'stdio' or 'http'.",
                s
            )),
        }
    }
}

/// HTTP server configuration for expose mode.
#[derive(Debug, Clone)]
pub struct HttpExposeConfig {
    /// Port to listen on.
    pub port: u16,
    /// Host/address to bind to.
    pub host: String,
}

impl Default for HttpExposeConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            host: "127.0.0.1".to_string(),
        }
    }
}

/// Shared state for the proxy - cloneable and thread-safe.
#[derive(Clone)]
struct SharedProxyState {
    /// The backend client connection (shared across sessions).
    backend: Arc<RwLock<McpConnection>>,
    /// Server info from the backend (cached from initialize).
    server_info: ServerInfo,
}

/// Proxy server that forwards requests to a backend MCP server.
pub struct ProxyHandler {
    /// Shared state with backend connection and server info.
    state: SharedProxyState,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl ProxyHandler {
    /// Create a new proxy handler with an established backend connection.
    pub fn new(backend: McpConnection) -> Self {
        let state = Self::create_shared_state(backend);
        Self { state }
    }

    /// Create shared state from a backend connection.
    fn create_shared_state(backend: McpConnection) -> SharedProxyState {
        // Clone the entire server info from backend (capabilities, instructions, etc.)
        let server_info = backend.peer_info().cloned().unwrap_or_else(|| {
            let mut si = ServerInfo::default();
            si.server_info.name = "proxy".to_string();
            si.server_info.version = "1.0.0".to_string();
            // Advertise all capabilities by default
            si.capabilities = ServerCapabilities {
                tools: Some(ToolsCapability::default()),
                prompts: Some(PromptsCapability::default()),
                resources: Some(ResourcesCapability::default()),
                ..Default::default()
            };
            si
        });

        SharedProxyState {
            backend: Arc::new(RwLock::new(backend)),
            server_info,
        }
    }

    /// Create a handler from shared state (for HTTP session factory).
    fn from_shared(state: SharedProxyState) -> Self {
        Self { state }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

#[allow(clippy::manual_async_fn)]
impl ServerHandler for ProxyHandler {
    fn list_tools(
        &self,
        request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        async move {
            let backend = self.state.backend.read().await;
            backend
                .peer()
                .list_tools(request)
                .await
                .map_err(|e| McpError::internal_error(format!("Backend error: {}", e), None))
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, McpError>> + Send + '_ {
        async move {
            let backend = self.state.backend.read().await;
            backend
                .peer()
                .call_tool(request)
                .await
                .map_err(|e| McpError::internal_error(format!("Backend error: {}", e), None))
        }
    }

    fn list_prompts(
        &self,
        request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, McpError>> + Send + '_ {
        async move {
            let backend = self.state.backend.read().await;
            match backend.peer().list_prompts(request).await {
                Ok(result) => Ok(result),
                Err(_) => Ok(ListPromptsResult::default()),
            }
        }
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetPromptResult, McpError>> + Send + '_ {
        async move {
            let backend = self.state.backend.read().await;
            backend
                .peer()
                .get_prompt(request)
                .await
                .map_err(|e| McpError::internal_error(format!("Backend error: {}", e), None))
        }
    }

    fn list_resources(
        &self,
        request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, McpError>> + Send + '_ {
        async move {
            let backend = self.state.backend.read().await;
            match backend.peer().list_resources(request).await {
                Ok(result) => Ok(result),
                Err(_) => Ok(ListResourcesResult::default()),
            }
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, McpError>> + Send + '_ {
        async move {
            let backend = self.state.backend.read().await;
            backend
                .peer()
                .read_resource(request)
                .await
                .map_err(|e| McpError::internal_error(format!("Backend error: {}", e), None))
        }
    }

    fn get_info(&self) -> ServerInfo {
        self.state.server_info.clone()
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Run the proxy server with the specified expose transport.
pub async fn run_proxy(
    backend: McpConnection,
    expose: Option<ExposeTransport>,
    http_config: HttpExposeConfig,
    backend_transport: McpbTransport,
    verbose: bool,
) -> ToolResult<()> {
    let handler = ProxyHandler::new(backend);

    // Determine expose transport (native if not specified)
    let expose_transport = expose.unwrap_or(match backend_transport {
        McpbTransport::Stdio => ExposeTransport::Stdio,
        McpbTransport::Http => ExposeTransport::Http,
    });

    match expose_transport {
        ExposeTransport::Stdio => run_stdio_server(handler, verbose).await,
        ExposeTransport::Http => run_http_server(handler, http_config, verbose).await,
    }
}

/// Run the proxy as a stdio server.
async fn run_stdio_server(handler: ProxyHandler, verbose: bool) -> ToolResult<()> {
    use tokio::io::{stdin, stdout};

    if verbose {
        eprintln!("Starting stdio proxy server...");
    }

    let transport = (stdin(), stdout());
    let server = serve_server(handler, transport)
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to start stdio server: {}", e)))?;

    if verbose {
        eprintln!("Stdio server running. Waiting for client...");
    }

    // Wait until the server is cancelled or client disconnects
    server
        .waiting()
        .await
        .map_err(|e| ToolError::Generic(format!("Server error: {}", e)))?;

    Ok(())
}

/// Run the proxy as an HTTP server.
async fn run_http_server(
    handler: ProxyHandler,
    config: HttpExposeConfig,
    verbose: bool,
) -> ToolResult<()> {
    if verbose {
        eprintln!(
            "Starting HTTP proxy server on {}:{}...",
            config.host, config.port
        );
    }

    // Create cancellation token for graceful shutdown
    let ct = CancellationToken::new();

    // Clone the shared state for the factory function
    let shared_state = handler.state.clone();

    // Create the HTTP service with a factory that creates handlers per session
    let service: StreamableHttpService<ProxyHandler, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(ProxyHandler::from_shared(shared_state.clone())),
            Default::default(),
            StreamableHttpServerConfig {
                stateful_mode: true,
                sse_keep_alive: None,
                cancellation_token: ct.child_token(),
            },
        );

    // Create axum router with the MCP service
    let router = axum::Router::new().nest_service("/mcp", service);

    // Bind to the configured address
    let bind_addr = format!("{}:{}", config.host, config.port);
    let tcp_listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to bind to {}: {}", bind_addr, e)))?;

    eprintln!(
        "HTTP server listening on http://{}:{}/mcp",
        config.host, config.port
    );
    eprintln!("Press Ctrl+C to stop");

    // Spawn the server with graceful shutdown
    let server_handle = tokio::spawn({
        let ct = ct.clone();
        async move {
            let _ = axum::serve(tcp_listener, router)
                .with_graceful_shutdown(async move { ct.cancelled_owned().await })
                .await;
        }
    });

    // Wait for Ctrl+C
    tokio::signal::ctrl_c()
        .await
        .map_err(|e| ToolError::Generic(format!("Signal error: {}", e)))?;

    eprintln!("\nShutting down...");
    ct.cancel();

    // Wait for server to finish
    let _ = server_handle.await;

    Ok(())
}
