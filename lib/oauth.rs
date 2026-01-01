//! Interactive OAuth flow for CLI.
//!
//! Handles browser-based OAuth authentication for MCP tools.

use colored::Colorize;

use crate::mcpb::OAuthConfig;
use crate::security::{CredentialCrypto, FileCredentialStore};
use crate::{ToolError, ToolResult};
use axum::{
    Router,
    extract::{Query, State},
    response::Html,
    routing::get,
};
use rmcp::transport::auth::{AuthorizationManager, AuthorizationMetadata, OAuthClientConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Default port for OAuth callback server.
pub const DEFAULT_CALLBACK_PORT: u16 = 8080;

/// HTML page shown after successful authorization.
const CALLBACK_SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Authorized</title>
    <style>
        @import url('https://fonts.googleapis.com/css2?family=Geist+Mono:wght@400;500&display=swap');
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Geist Mono', ui-monospace, SFMono-Regular, monospace;
            min-height: 100vh;
            display: flex;
            align-items: center;
            justify-content: center;
            background: #1a1a1a;
            color: #fafafa;
        }
        .container {
            background: #303030;
            border: 1px solid #525252;
            padding: 40px 48px;
            max-width: 360px;
            text-align: center;
        }
        .icon {
            width: 24px;
            height: 24px;
            border: 2px solid #10b981;
            display: inline-flex;
            align-items: center;
            justify-content: center;
            margin-bottom: 20px;
        }
        .icon svg {
            width: 12px;
            height: 12px;
            stroke: #10b981;
            stroke-width: 2.5;
            fill: none;
        }
        .title {
            font-size: 14px;
            font-weight: 500;
            margin-bottom: 8px;
            letter-spacing: -0.01em;
        }
        .hint {
            font-size: 13px;
            color: #a3a3a3;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="icon">
            <svg viewBox="0 0 24 24"><polyline points="20 6 9 17 4 12"></polyline></svg>
        </div>
        <p class="title">Authorization complete</p>
        <p class="hint">You can close this window.</p>
    </div>
</body>
</html>"#;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// OAuth credentials for authenticated MCP connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredentials {
    /// OAuth client ID.
    pub client_id: String,
    /// Access token for API requests.
    pub access_token: String,
    /// Refresh token for obtaining new access tokens.
    pub refresh_token: Option<String>,
    /// Unix timestamp when the access token expires.
    pub expires_at: Option<i64>,
}

/// Session for completing OAuth authentication.
pub struct AuthSession {
    /// The rmcp authorization manager.
    manager: AuthorizationManager,
    /// The MCP server URL.
    url: String,
    /// OAuth config from manifest (if any).
    pub oauth_config: Option<OAuthConfig>,
}

/// Options for the interactive OAuth flow.
#[derive(Debug, Clone)]
pub struct OAuthFlowOptions {
    /// Port for the callback server.
    pub callback_port: u16,
    /// Client name for dynamic registration.
    pub client_name: String,
}

impl Default for OAuthFlowOptions {
    fn default() -> Self {
        Self {
            callback_port: DEFAULT_CALLBACK_PORT,
            client_name: "Tool CLI".to_string(),
        }
    }
}

/// Parameters received from the OAuth callback.
#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

/// State shared with the callback server.
#[derive(Clone)]
struct CallbackState {
    code_sender: Arc<Mutex<Option<oneshot::Sender<CallbackParams>>>>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl OAuthCredentials {
    /// Create OAuthCredentials from rmcp's token response.
    pub fn from_token_response(
        token_response: &rmcp::transport::auth::OAuthTokenResponse,
        client_id: &str,
    ) -> Self {
        use oauth2::TokenResponse;

        let expires_at = token_response
            .expires_in()
            .map(|duration| chrono::Utc::now().timestamp() + duration.as_secs() as i64);

        Self {
            client_id: client_id.to_string(),
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().to_string()),
            expires_at,
        }
    }

    /// Check if the access token has expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(expires_at) => chrono::Utc::now().timestamp() >= expires_at,
            None => false,
        }
    }
}

impl AuthSession {
    /// Create a new auth session.
    pub(crate) fn new(
        manager: AuthorizationManager,
        url: String,
        oauth_config: Option<OAuthConfig>,
    ) -> Self {
        Self {
            manager,
            url,
            oauth_config,
        }
    }

    /// Get the MCP server URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Configure pre-registered OAuth client.
    pub fn configure_client(
        &mut self,
        client_id: &str,
        redirect_uri: &str,
        scopes: &[String],
    ) -> ToolResult<()> {
        let config = OAuthClientConfig {
            client_id: client_id.to_string(),
            client_secret: None,
            scopes: scopes.to_vec(),
            redirect_uri: redirect_uri.to_string(),
        };
        self.manager
            .configure_client(config)
            .map_err(|e| ToolError::Generic(format!("Failed to configure OAuth client: {}", e)))
    }

    /// Register client via Dynamic Client Registration (RFC 7591).
    pub async fn register_client(
        &mut self,
        client_name: &str,
        redirect_uri: &str,
    ) -> ToolResult<()> {
        let config = self
            .manager
            .register_client(client_name, redirect_uri)
            .await
            .map_err(|e| {
                ToolError::Generic(format!("Dynamic Client Registration failed: {}", e))
            })?;

        self.manager.configure_client(config).map_err(|e| {
            ToolError::Generic(format!("Failed to configure registered client: {}", e))
        })
    }

    /// Generate authorization URL.
    pub async fn authorization_url(&self, scopes: &[&str]) -> ToolResult<String> {
        self.manager
            .get_authorization_url(scopes)
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to get authorization URL: {}", e)))
    }

    /// Exchange authorization code for credentials.
    pub async fn exchange(&mut self, code: &str, state: &str) -> ToolResult<OAuthCredentials> {
        let token_response = self
            .manager
            .exchange_code_for_token(code, state)
            .await
            .map_err(|e| {
                ToolError::Generic(format!("Failed to exchange code for tokens: {}", e))
            })?;

        let (client_id, _) =
            self.manager.get_credentials().await.map_err(|e| {
                ToolError::Generic(format!("Failed to get client credentials: {}", e))
            })?;

        Ok(OAuthCredentials::from_token_response(
            &token_response,
            &client_id,
        ))
    }

    /// Set credential store on the authorization manager.
    pub fn set_credential_store<S: rmcp::transport::auth::CredentialStore + 'static>(
        &mut self,
        store: S,
    ) {
        self.manager.set_credential_store(store);
    }

    /// Consume this session and return the configured AuthorizationManager.
    pub fn into_manager(self) -> AuthorizationManager {
        self.manager
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Prepare an auth session for OAuth authentication.
pub async fn prepare_auth_session(
    url: &str,
    oauth_config: Option<OAuthConfig>,
) -> ToolResult<AuthSession> {
    let mut manager = AuthorizationManager::new(url)
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to initialize OAuth: {}", e)))?;

    // Configure metadata (discovery or custom)
    let metadata = if let Some(ref config) = oauth_config {
        if config.authorization_url.is_some() && config.token_url.is_some() {
            // Custom metadata from manifest
            AuthorizationMetadata {
                authorization_endpoint: config.authorization_url.clone().unwrap(),
                token_endpoint: config.token_url.clone().unwrap(),
                registration_endpoint: None,
                issuer: None,
                jwks_uri: None,
                scopes_supported: config.scopes.clone(),
                additional_fields: HashMap::new(),
            }
        } else {
            // Partial config - still need discovery
            manager.discover_metadata().await.map_err(|e| {
                ToolError::Generic(format!("OAuth metadata discovery failed: {}", e))
            })?
        }
    } else {
        // No config - RFC 8414 discovery
        manager
            .discover_metadata()
            .await
            .map_err(|e| ToolError::Generic(format!("OAuth metadata discovery failed: {}", e)))?
    };

    manager.set_metadata(metadata);

    Ok(AuthSession::new(manager, url.to_string(), oauth_config))
}

/// Run interactive OAuth flow.
pub async fn run_interactive_oauth(
    mut session: AuthSession,
    tool_ref: &str,
    crypto: CredentialCrypto,
    options: OAuthFlowOptions,
) -> ToolResult<OAuthCredentials> {
    let redirect_uri = format!("http://127.0.0.1:{}/callback", options.callback_port);

    // Get scopes and client_id from oauth_config
    let scopes: Vec<String> = session
        .oauth_config
        .as_ref()
        .and_then(|c| c.scopes.clone())
        .unwrap_or_else(|| vec!["mcp".to_string()]);

    let client_id = session
        .oauth_config
        .as_ref()
        .and_then(|c| c.client_id.clone());

    // Configure client (pre-registered or DCR)
    if let Some(ref id) = client_id {
        tracing::debug!("Using pre-registered client ID: {}", id);
        session.configure_client(id, &redirect_uri, &scopes)?;
    } else {
        tracing::debug!(
            "No client_id in manifest, attempting DCR with client_name={}, redirect_uri={}",
            options.client_name,
            redirect_uri
        );
        session
            .register_client(&options.client_name, &redirect_uri)
            .await?;
        tracing::debug!("DCR completed successfully");
    }

    // Set up credential store - the store will save credentials after exchange
    let store = FileCredentialStore::new(tool_ref, crypto);
    session.set_credential_store(store);

    // Generate authorization URL
    let scopes_refs: Vec<&str> = scopes.iter().map(String::as_str).collect();
    let auth_url = session.authorization_url(&scopes_refs).await?;

    // Create channel for receiving the callback
    let (code_tx, code_rx) = oneshot::channel::<CallbackParams>();

    // Start callback server
    let callback_state = CallbackState {
        code_sender: Arc::new(Mutex::new(Some(code_tx))),
    };

    let server_handle = start_callback_server(options.callback_port, callback_state);

    // Open browser
    eprintln!(
        "    {} Opening browser for authorization...",
        "→".bright_blue()
    );
    eprintln!(
        "    {} If browser doesn't open, visit:\n      {}\n",
        "?".bright_yellow(),
        auth_url.dimmed()
    );

    if let Err(e) = open::that(auth_url.as_str()) {
        eprintln!("    {} Failed to open browser: {}", "!".bright_yellow(), e);
        eprintln!("      Please manually open the URL above.");
    }

    // Wait for callback with timeout
    let params = tokio::time::timeout(std::time::Duration::from_secs(300), code_rx)
        .await
        .map_err(|_| ToolError::Generic("OAuth callback timeout (5 minutes)".into()))?
        .map_err(|_| ToolError::Generic("OAuth callback channel closed".into()))?;

    // Abort the callback server
    server_handle.abort();

    // Exchange code for tokens (credentials are saved automatically via the store)
    let credentials = session.exchange(&params.code, &params.state).await?;

    eprintln!("  {} Authorization successful\n", "✓".bright_green());

    Ok(credentials)
}

/// Load stored OAuth credentials for a tool.
pub async fn load_credentials(tool_ref: &str) -> ToolResult<Option<OAuthCredentials>> {
    let crypto = match crate::security::get_credential_crypto() {
        Some(c) => c,
        None => return Ok(None),
    };

    let store = FileCredentialStore::new(tool_ref, crypto);

    use rmcp::transport::auth::CredentialStore;
    match store.load().await {
        Ok(Some(stored)) => {
            // Convert rmcp StoredCredentials to OAuthCredentials
            Ok(stored
                .token_response
                .map(|t| OAuthCredentials::from_token_response(&t, &stored.client_id)))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            tracing::debug!("Failed to load credentials for '{}': {}", tool_ref, e);
            Ok(None)
        }
    }
}

/// Start the OAuth callback server.
fn start_callback_server(port: u16, state: CallbackState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let app = Router::new()
            .route("/callback", get(callback_handler))
            .with_state(state);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));

        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                tracing::debug!("OAuth callback server listening on {}", addr);
                if let Err(e) = axum::serve(listener, app).await {
                    tracing::error!("Callback server error: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("Failed to bind callback server to {}: {}", addr, e);
            }
        }
    })
}

/// Handle the OAuth callback request.
async fn callback_handler(
    Query(params): Query<CallbackParams>,
    State(state): State<CallbackState>,
) -> Html<&'static str> {
    tracing::debug!("Received OAuth callback with code");

    if let Some(sender) = state.code_sender.lock().await.take() {
        let _ = sender.send(params);
    }

    Html(CALLBACK_SUCCESS_HTML)
}
