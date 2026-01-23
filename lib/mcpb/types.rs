//! MCPB type definitions.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Author information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbAuthor {
    /// Author name (required within author object).
    pub name: String,

    /// Author email.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Author URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl McpbAuthor {
    /// Create a new author with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: None,
            url: None,
        }
    }

    /// Create a new author with name and email.
    pub fn with_email(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: Some(email.into()),
            url: None,
        }
    }
}

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbServer {
    /// Server runtime type (optional for HTTP reference mode).
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub server_type: Option<McpbServerType>,

    /// Transport type (stdio or http). Defaults to stdio.
    #[serde(default, skip_serializing_if = "McpbTransport::is_stdio")]
    pub transport: McpbTransport,

    /// Path to entry point file. Absent = reference mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<String>,

    /// MCP execution configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_config: Option<McpbMcpConfig>,
}

/// Transport type for MCP servers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpbTransport {
    /// Standard input/output transport.
    #[default]
    Stdio,
    /// HTTP transport.
    Http,
}

impl McpbTransport {
    /// Check if this is stdio transport (for skip_serializing_if).
    pub fn is_stdio(&self) -> bool {
        matches!(self, McpbTransport::Stdio)
    }
}

impl std::fmt::Display for McpbTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpbTransport::Stdio => write!(f, "stdio"),
            McpbTransport::Http => write!(f, "http"),
        }
    }
}

/// Package manager for Node.js projects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodePackageManager {
    /// npm - Node Package Manager.
    #[default]
    Npm,
    /// pnpm - Fast, disk space efficient package manager.
    Pnpm,
    /// Bun - All-in-one JavaScript runtime and package manager.
    Bun,
    /// Yarn - Alternative package manager.
    Yarn,
}

impl NodePackageManager {
    /// Get the build command for this package manager.
    pub fn build_command(&self) -> &'static str {
        match self {
            Self::Npm => "npm install",
            Self::Pnpm => "pnpm install",
            Self::Bun => "bun install",
            Self::Yarn => "yarn install",
        }
    }
}

impl std::fmt::Display for NodePackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Npm => write!(f, "npm"),
            Self::Pnpm => write!(f, "pnpm"),
            Self::Yarn => write!(f, "yarn"),
            Self::Bun => write!(f, "bun"),
        }
    }
}

/// Package manager for Python projects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PythonPackageManager {
    /// uv - Fast Python package manager.
    #[default]
    Uv,
    /// pip - Standard Python package installer.
    Pip,
    /// Poetry - Dependency management and packaging.
    Poetry,
}

impl PythonPackageManager {
    /// Get the build command for this package manager.
    pub fn build_command(&self) -> &'static str {
        match self {
            Self::Uv => "uv sync",
            Self::Pip => "python3 -m venv .venv && .venv/bin/pip install -r requirements.txt",
            Self::Poetry => "poetry install",
        }
    }

    /// Get the command to run Python scripts.
    pub fn run_command(&self) -> &'static str {
        match self {
            Self::Uv => "uv",
            Self::Pip => ".venv/bin/python",
            Self::Poetry => "poetry",
        }
    }

    /// Get the args prefix for running Python scripts.
    pub fn run_args_prefix(&self) -> Vec<&'static str> {
        match self {
            Self::Uv => vec!["run"],
            Self::Pip => vec![],
            Self::Poetry => vec!["run", "python"],
        }
    }
}

impl std::fmt::Display for PythonPackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uv => write!(f, "uv"),
            Self::Pip => write!(f, "pip"),
            Self::Poetry => write!(f, "poetry"),
        }
    }
}

/// Package manager selection (language-specific).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// Node.js package manager.
    Node(NodePackageManager),
    /// Python package manager.
    Python(PythonPackageManager),
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node(pm) => write!(f, "{}", pm),
            Self::Python(pm) => write!(f, "{}", pm),
        }
    }
}

/// Platform-specific override for mcp_config fields.
///
/// Used in `platform_overrides` to specify different values for different OS/arch combinations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct McpbPlatformOverride {
    /// Override command to execute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Override command arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

    /// Override environment variables (merged with base).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,

    /// Override HTTP endpoint URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Override HTTP headers (merged with base).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
}

/// MCP execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbMcpConfig {
    /// Command to execute (required for stdio transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Command arguments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,

    /// Environment variables.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,

    /// HTTP endpoint URL (required for http transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// HTTP headers for authentication or configuration.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,

    /// OAuth configuration for HTTP servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_config: Option<OAuthConfig>,

    /// Platform-specific overrides keyed by OS (darwin, linux, win32).
    ///
    /// Per MCPB spec, these override base mcp_config fields for specific platforms.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub platform_overrides: BTreeMap<String, McpbPlatformOverride>,
}

/// OAuth configuration for HTTP MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthConfig {
    /// Pre-registered OAuth client ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Custom authorization endpoint URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,

    /// Custom token endpoint URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,

    /// OAuth scopes to request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

/// Server runtime type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpbServerType {
    /// Node.js runtime.
    #[default]
    Node,
    /// Python runtime.
    Python,
    /// Pre-compiled binary.
    Binary,
}

impl fmt::Display for McpbServerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Node => write!(f, "node"),
            Self::Python => write!(f, "python"),
            Self::Binary => write!(f, "binary"),
        }
    }
}

impl FromStr for McpbServerType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "node" | "nodejs" | "js" => Ok(Self::Node),
            "python" | "py" => Ok(Self::Python),
            "binary" | "rust" | "go" => Ok(Self::Binary),
            _ => Err(format!("Unknown server type: {}", s)),
        }
    }
}

/// Icon with size specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbIcon {
    /// Icon size (e.g., "16x16", "32x32").
    pub size: String,
    /// Path to icon file.
    pub path: String,
}

/// Repository information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbRepository {
    /// Repository type (e.g., "git").
    #[serde(rename = "type")]
    pub repo_type: String,
    /// Repository URL.
    pub url: String,
}

/// Static tool declaration (top-level, simple format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbTool {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
}

/// Full MCP tool declaration with input/output schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbToolFull {
    /// Tool name.
    pub name: String,

    /// Tool description.
    pub description: String,

    /// Human-readable title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// JSON Schema for tool input parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,

    /// JSON Schema for tool output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

/// Static responses for MCP protocol methods.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StaticResponses {
    /// Response for `tools/list` method.
    #[serde(rename = "tools/list", skip_serializing_if = "Option::is_none")]
    pub tools_list: Option<ToolsListResponse>,

    /// Response for `prompts/list` method.
    #[serde(rename = "prompts/list", skip_serializing_if = "Option::is_none")]
    pub prompts_list: Option<PromptsListResponse>,

    /// Response for `resources/list` method.
    #[serde(rename = "resources/list", skip_serializing_if = "Option::is_none")]
    pub resources_list: Option<ResourcesListResponse>,
}

/// Response for `tools/list` MCP method.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsListResponse {
    /// Full tool definitions with schemas.
    pub tools: Vec<McpbToolFull>,
}

/// Response for `prompts/list` MCP method.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptsListResponse {
    /// Prompt definitions.
    pub prompts: Vec<McpbPrompt>,
}

/// Response for `resources/list` MCP method.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourcesListResponse {
    /// Resource definitions.
    pub resources: Vec<McpbResource>,
}

/// MCP resource declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpbResource {
    /// Resource URI.
    pub uri: String,

    /// Resource name.
    pub name: String,

    /// Resource description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// MIME type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Static prompt template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbPrompt {
    /// Prompt name.
    pub name: String,
    /// Prompt description.
    pub description: String,
    /// Prompt arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<McpbPromptArgument>>,
    /// Prompt template string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

/// Prompt argument definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbPromptArgument {
    /// Argument name.
    pub name: String,
    /// Argument description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the argument is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// User-configurable field definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbUserConfigField {
    /// Field type.
    #[serde(rename = "type")]
    pub field_type: McpbUserConfigType,
    /// Display title.
    pub title: String,
    /// Field description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the field is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    /// Whether multiple values are allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiple: Option<bool>,
    /// Whether the value is sensitive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitive: Option<bool>,
    /// Allowed values for string fields.
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// Minimum value for numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Maximum value for numbers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// System configuration field definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbSystemConfigField {
    /// Field type.
    #[serde(rename = "type")]
    pub field_type: McpbSystemConfigType,
    /// Display title.
    pub title: String,
    /// Field description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the field is required.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

/// System config field type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpbSystemConfigType {
    /// Network port for binding.
    Port,
    /// Ephemeral directory.
    TempDirectory,
    /// Persistent directory.
    DataDirectory,
}

/// User config field type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpbUserConfigType {
    /// String value.
    String,
    /// Numeric value.
    Number,
    /// Boolean value.
    Boolean,
    /// Directory path.
    Directory,
    /// File path.
    File,
}

/// Platform/runtime compatibility requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbCompatibility {
    /// Claude Desktop version requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_desktop: Option<String>,
    /// Supported platforms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<McpbPlatform>>,
    /// Runtime version requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtimes: Option<McpbRuntimes>,
}

/// Supported platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpbPlatform {
    /// macOS.
    Darwin,
    /// Windows.
    Win32,
    /// Linux.
    Linux,
}

/// Runtime version requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbRuntimes {
    /// Node.js version requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// Python version requirement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
}

/// Localization/i18n configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbLocalization {
    /// Path to localization resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<String>,
    /// Default locale identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_locale: Option<String>,
}

/// Scripts defined in _meta.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scripts {
    /// Build script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,

    /// Test script.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,

    /// Additional custom scripts.
    #[serde(flatten)]
    pub custom: BTreeMap<String, String>,
}
