//! Resolved manifest types with template expressions evaluated.

use std::collections::BTreeMap;

use super::manifest::McpbManifest;
use super::types::{McpbTransport, OAuthConfig};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Resolved MCP config with all template expressions evaluated.
#[derive(Debug, Clone)]
pub struct ResolvedMcpConfig {
    /// Resolved command.
    pub command: Option<String>,
    /// Resolved arguments.
    pub args: Vec<String>,
    /// Resolved environment variables.
    pub env: BTreeMap<String, String>,
    /// Resolved URL (for HTTP).
    pub url: Option<String>,
    /// Resolved headers.
    pub headers: BTreeMap<String, String>,
    /// OAuth config (passed through).
    pub oauth_config: Option<OAuthConfig>,
}

/// Resolved MCPB manifest with all template expressions evaluated.
#[derive(Debug, Clone)]
pub struct ResolvedMcpbManifest {
    /// Original manifest (for metadata access).
    pub manifest: McpbManifest,
    /// Resolved MCP configuration.
    pub mcp_config: ResolvedMcpConfig,
    /// Transport type.
    pub transport: McpbTransport,
    /// Whether this is reference mode (no entry_point).
    pub is_reference: bool,
}
