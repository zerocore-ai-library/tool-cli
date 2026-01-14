//! MCPB (MCP Bundle) manifest types for serialization.

mod init_mode;
mod manifest;
mod platform;
mod resolved;
mod types;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// tool.store's MCPB namespace identifier for vendor extensions.
pub const TOOL_STORE_NAMESPACE: &str = "store.tool.mcpb";

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use init_mode::InitMode;
pub use manifest::McpbManifest;
pub use platform::{
    detect_platform, get_current_arch, get_current_os, get_current_platform,
    resolve_platform_overrides,
};
pub use resolved::{ResolvedMcpConfig, ResolvedMcpbManifest};
pub use types::{
    McpbAuthor, McpbCompatibility, McpbIcon, McpbLocalization, McpbMcpConfig, McpbPlatform,
    McpbPlatformOverride, McpbPrompt, McpbPromptArgument, McpbRepository, McpbResource,
    McpbRuntimes, McpbServer, McpbServerType, McpbSystemConfigField, McpbSystemConfigType,
    McpbTool, McpbToolFull, McpbTransport, McpbUserConfigField, McpbUserConfigType,
    NodePackageManager, OAuthConfig, PackageManager, PromptsListResponse, PythonPackageManager,
    ResourcesListResponse, Scripts, StaticResponses, ToolsListResponse,
};
