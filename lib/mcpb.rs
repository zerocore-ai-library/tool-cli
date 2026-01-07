//! MCPB (MCP Bundle) manifest types for serialization.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::vars;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Initialization mode for `tool init`.
#[derive(Debug, Clone)]
pub enum InitMode {
    /// Bundled server with scaffolding.
    Bundle {
        server_type: McpbServerType,
        transport: McpbTransport,
        package_manager: Option<PackageManager>,
    },
    /// Reference to external server (no scaffolding).
    Reference { transport: McpbTransport },
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl InitMode {
    /// Create bundle mode with stdio transport.
    pub fn bundle_stdio(server_type: McpbServerType) -> Self {
        Self::Bundle {
            server_type,
            transport: McpbTransport::Stdio,
            package_manager: Self::default_package_manager(server_type),
        }
    }

    /// Create bundle mode with HTTP transport.
    pub fn bundle_http(server_type: McpbServerType) -> Self {
        Self::Bundle {
            server_type,
            transport: McpbTransport::Http,
            package_manager: Self::default_package_manager(server_type),
        }
    }

    /// Create bundle mode with explicit package manager.
    pub fn bundle(
        server_type: McpbServerType,
        transport: McpbTransport,
        package_manager: Option<PackageManager>,
    ) -> Self {
        Self::Bundle {
            server_type,
            transport,
            package_manager: package_manager.or_else(|| Self::default_package_manager(server_type)),
        }
    }

    /// Create reference mode with stdio transport.
    pub fn reference_stdio() -> Self {
        Self::Reference {
            transport: McpbTransport::Stdio,
        }
    }

    /// Create reference mode with HTTP transport.
    pub fn reference_http() -> Self {
        Self::Reference {
            transport: McpbTransport::Http,
        }
    }

    /// Get the default package manager for a server type.
    fn default_package_manager(server_type: McpbServerType) -> Option<PackageManager> {
        match server_type {
            McpbServerType::Node => Some(PackageManager::Node(NodePackageManager::default())),
            McpbServerType::Python => Some(PackageManager::Python(PythonPackageManager::default())),
            McpbServerType::Binary => None,
        }
    }

    /// Check if this is reference mode.
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::Reference { .. })
    }

    /// Check if this uses HTTP transport.
    pub fn is_http(&self) -> bool {
        match self {
            Self::Bundle { transport, .. } | Self::Reference { transport } => {
                *transport == McpbTransport::Http
            }
        }
    }

    /// Get the server type if in bundle mode.
    pub fn server_type(&self) -> Option<McpbServerType> {
        match self {
            Self::Bundle { server_type, .. } => Some(*server_type),
            Self::Reference { .. } => None,
        }
    }

    /// Get the transport type.
    pub fn transport(&self) -> McpbTransport {
        match self {
            Self::Bundle { transport, .. } | Self::Reference { transport } => *transport,
        }
    }

    /// Get the package manager if in bundle mode.
    pub fn package_manager(&self) -> Option<PackageManager> {
        match self {
            Self::Bundle {
                package_manager, ..
            } => *package_manager,
            Self::Reference { .. } => None,
        }
    }

    /// Get the Node.js package manager if applicable.
    pub fn node_package_manager(&self) -> Option<NodePackageManager> {
        match self.package_manager() {
            Some(PackageManager::Node(pm)) => Some(pm),
            _ => None,
        }
    }

    /// Get the Python package manager if applicable.
    pub fn python_package_manager(&self) -> Option<PythonPackageManager> {
        match self.package_manager() {
            Some(PackageManager::Python(pm)) => Some(pm),
            _ => None,
        }
    }
}

/// MCPB manifest structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpbManifest {
    /// Specification version (currently "0.3").
    pub manifest_version: String,

    /// Machine-readable package name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Semantic version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Brief description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Author information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<McpbAuthor>,

    /// Server configuration.
    pub server: McpbServer,

    /// Human-readable display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Extended description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_description: Option<String>,

    /// License identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Path to icon file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Multiple icon sizes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons: Option<Vec<McpbIcon>>,

    /// Project homepage URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Documentation URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,

    /// Support URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support: Option<String>,

    /// Repository information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<McpbRepository>,

    /// Search keywords.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,

    /// Static tool declarations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<McpbTool>>,

    /// Static prompt templates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<McpbPrompt>>,

    /// Server generates dynamic tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools_generated: Option<bool>,

    /// Server generates dynamic prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts_generated: Option<bool>,

    /// User-configurable options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_config: Option<BTreeMap<String, McpbUserConfigField>>,

    /// System configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_config: Option<BTreeMap<String, McpbSystemConfigField>>,

    /// Platform/runtime requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<McpbCompatibility>,

    /// Privacy policy URLs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub privacy_policies: Option<Vec<String>>,

    /// Internationalization configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localization: Option<McpbLocalization>,

    /// Platform-specific metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,

    /// Path to the bundle directory (set after loading, not serialized).
    #[serde(skip)]
    pub bundle_path: Option<PathBuf>,
}

impl McpbManifest {
    /// Load manifest from a directory.
    pub fn load(dir: &Path) -> ToolResult<Self> {
        let manifest_path = dir.join(MCPB_MANIFEST_FILE);
        let content = std::fs::read_to_string(&manifest_path)?;
        let mut manifest: McpbManifest = serde_json::from_str(&content)?;
        manifest.bundle_path = Some(dir.to_path_buf());
        Ok(manifest)
    }

    /// Get the transport type from server config.
    pub fn transport(&self) -> McpbTransport {
        self.server.transport
    }

    /// Check if this is reference mode (no entry_point).
    pub fn is_reference(&self) -> bool {
        self.server.entry_point.is_none()
    }

    /// Get static_responses from _meta if present.
    pub fn static_responses(&self) -> Option<StaticResponses> {
        self.meta
            .as_ref()?
            .get("company.superrad.mcpb")?
            .get("static_responses")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Get scripts from _meta if present.
    pub fn scripts(&self) -> Option<Scripts> {
        self.meta
            .as_ref()?
            .get("company.superrad.mcpb")?
            .get("scripts")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Resolve the manifest with user/system config values.
    pub fn resolve(
        &self,
        user_config: &BTreeMap<String, String>,
        system_config: &BTreeMap<String, String>,
    ) -> ToolResult<ResolvedMcpbManifest> {
        let dirname = self
            .bundle_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let mcp_config = if let Some(ref cfg) = self.server.mcp_config {
            ResolvedMcpConfig {
                command: cfg
                    .command
                    .as_ref()
                    .map(|c| vars::substitute_vars(c, &dirname, user_config, system_config))
                    .transpose()?,
                args: cfg
                    .args
                    .iter()
                    .map(|a| vars::substitute_vars(a, &dirname, user_config, system_config))
                    .collect::<Result<Vec<_>, _>>()?,
                env: cfg
                    .env
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            k.clone(),
                            vars::substitute_vars(v, &dirname, user_config, system_config)?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>, ToolError>>()?,
                url: cfg
                    .url
                    .as_ref()
                    .map(|u| vars::substitute_vars(u, &dirname, user_config, system_config))
                    .transpose()?,
                headers: cfg
                    .headers
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            k.clone(),
                            vars::substitute_vars(v, &dirname, user_config, system_config)?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>, ToolError>>()?,
                oauth_config: cfg.oauth_config.clone(),
            }
        } else {
            ResolvedMcpConfig {
                command: None,
                args: Vec::new(),
                env: BTreeMap::new(),
                url: None,
                headers: BTreeMap::new(),
                oauth_config: None,
            }
        };

        Ok(ResolvedMcpbManifest {
            manifest: self.clone(),
            mcp_config,
            transport: self.transport(),
            is_reference: self.is_reference(),
        })
    }

    /// Create a manifest from an initialization mode.
    pub fn from_mode(mode: &InitMode) -> Self {
        match mode {
            InitMode::Bundle {
                server_type,
                transport,
                package_manager,
            } => Self::new_bundle(*server_type, *transport, *package_manager),
            InitMode::Reference { transport } => Self::new_reference(*transport),
        }
    }

    /// Create a manifest for bundle mode.
    fn new_bundle(
        server_type: McpbServerType,
        transport: McpbTransport,
        package_manager: Option<PackageManager>,
    ) -> Self {
        // Get the build command based on package manager
        let build_cmd = match package_manager {
            Some(PackageManager::Node(pm)) => pm.build_command().to_string(),
            Some(PackageManager::Python(pm)) => pm.build_command().to_string(),
            None => match server_type {
                McpbServerType::Node => NodePackageManager::default().build_command().to_string(),
                McpbServerType::Python => {
                    PythonPackageManager::default().build_command().to_string()
                }
                McpbServerType::Binary => String::new(),
            },
        };

        // Get Python package manager for mcp_config
        let python_pm = match package_manager {
            Some(PackageManager::Python(pm)) => pm,
            _ => PythonPackageManager::default(),
        };

        let (entry_point, mcp_config, user_config, system_config, meta) =
            match (server_type, transport) {
                // Bundle Stdio modes
                (McpbServerType::Node, McpbTransport::Stdio) => (
                    Some("server/index.js".to_string()),
                    Some(McpbMcpConfig {
                        command: Some("node".to_string()),
                        args: vec!["${__dirname}/server/index.js".to_string()],
                        env: BTreeMap::new(),
                        url: None,
                        headers: BTreeMap::new(),
                        oauth_config: None,
                    }),
                    None,
                    None,
                    Some(serde_json::json!({
                        "company.superrad.mcpb": {
                            "scripts": {
                                "build": build_cmd
                            }
                        }
                    })),
                ),
                (McpbServerType::Python, McpbTransport::Stdio) => {
                    let mut args: Vec<String> = python_pm
                        .run_args_prefix()
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();
                    args.push("server/main.py".to_string());

                    (
                        Some("server/main.py".to_string()),
                        Some(McpbMcpConfig {
                            command: Some(python_pm.run_command().to_string()),
                            args,
                            env: BTreeMap::new(),
                            url: None,
                            headers: BTreeMap::new(),
                            oauth_config: None,
                        }),
                        None,
                        None,
                        Some(serde_json::json!({
                            "company.superrad.mcpb": {
                                "scripts": {
                                    "build": build_cmd
                                }
                            }
                        })),
                    )
                }
                (McpbServerType::Binary, McpbTransport::Stdio) => (None, None, None, None, None),
                // Bundle HTTP modes - use system_config for ports/hostname
                (McpbServerType::Node, McpbTransport::Http) => {
                    let mut sys_cfg = BTreeMap::new();
                    sys_cfg.insert(
                        "port".to_string(),
                        McpbSystemConfigField {
                            field_type: McpbSystemConfigType::Port,
                            title: "Server Port".to_string(),
                            description: Some("Port for the MCP HTTP endpoint".to_string()),
                            required: None,
                            default: Some(serde_json::json!(3000)),
                        },
                    );
                    sys_cfg.insert(
                        "hostname".to_string(),
                        McpbSystemConfigField {
                            field_type: McpbSystemConfigType::Hostname,
                            title: "Bind Address".to_string(),
                            description: Some("Network interface to bind to".to_string()),
                            required: None,
                            default: Some(serde_json::json!("127.0.0.1")),
                        },
                    );
                    (
                        Some("server/index.js".to_string()),
                        Some(McpbMcpConfig {
                            command: Some("node".to_string()),
                            args: vec![
                                "${__dirname}/server/index.js".to_string(),
                                "--port=${system_config.port}".to_string(),
                                "--host=${system_config.hostname}".to_string(),
                            ],
                            env: BTreeMap::new(),
                            url: Some(
                                "http://${system_config.hostname}:${system_config.port}/mcp"
                                    .to_string(),
                            ),
                            headers: BTreeMap::new(),
                            oauth_config: None,
                        }),
                        None,
                        Some(sys_cfg),
                        Some(serde_json::json!({
                            "company.superrad.mcpb": {
                                "scripts": {
                                    "build": build_cmd
                                }
                            }
                        })),
                    )
                }
                (McpbServerType::Python, McpbTransport::Http) => {
                    let mut sys_cfg = BTreeMap::new();
                    sys_cfg.insert(
                        "port".to_string(),
                        McpbSystemConfigField {
                            field_type: McpbSystemConfigType::Port,
                            title: "Server Port".to_string(),
                            description: Some("Port for the MCP HTTP endpoint".to_string()),
                            required: None,
                            default: Some(serde_json::json!(3000)),
                        },
                    );
                    sys_cfg.insert(
                        "hostname".to_string(),
                        McpbSystemConfigField {
                            field_type: McpbSystemConfigType::Hostname,
                            title: "Bind Address".to_string(),
                            description: Some("Network interface to bind to".to_string()),
                            required: None,
                            default: Some(serde_json::json!("127.0.0.1")),
                        },
                    );

                    let mut args: Vec<String> = python_pm
                        .run_args_prefix()
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();
                    args.push("server/main.py".to_string());
                    args.push("--port".to_string());
                    args.push("${system_config.port}".to_string());
                    args.push("--host".to_string());
                    args.push("${system_config.hostname}".to_string());

                    (
                        Some("server/main.py".to_string()),
                        Some(McpbMcpConfig {
                            command: Some(python_pm.run_command().to_string()),
                            args,
                            env: BTreeMap::new(),
                            url: Some(
                                "http://${system_config.hostname}:${system_config.port}/mcp"
                                    .to_string(),
                            ),
                            headers: BTreeMap::new(),
                            oauth_config: None,
                        }),
                        None,
                        Some(sys_cfg),
                        Some(serde_json::json!({
                            "company.superrad.mcpb": {
                                "scripts": {
                                    "build": build_cmd
                                }
                            }
                        })),
                    )
                }
                (McpbServerType::Binary, McpbTransport::Http) => {
                    let mut sys_cfg = BTreeMap::new();
                    sys_cfg.insert(
                        "port".to_string(),
                        McpbSystemConfigField {
                            field_type: McpbSystemConfigType::Port,
                            title: "Server Port".to_string(),
                            description: Some("Port for the MCP HTTP endpoint".to_string()),
                            required: None,
                            default: Some(serde_json::json!(3000)),
                        },
                    );
                    sys_cfg.insert(
                        "hostname".to_string(),
                        McpbSystemConfigField {
                            field_type: McpbSystemConfigType::Hostname,
                            title: "Bind Address".to_string(),
                            description: Some("Network interface to bind to".to_string()),
                            required: None,
                            default: Some(serde_json::json!("127.0.0.1")),
                        },
                    );
                    (None, None, None, Some(sys_cfg), None)
                }
            };

        Self {
            manifest_version: "0.3".to_string(),
            name: None,
            version: Some("0.1.0".to_string()),
            description: Some("An MCP server".to_string()),
            author: None,
            server: McpbServer {
                server_type: Some(server_type),
                transport,
                entry_point,
                mcp_config,
            },
            display_name: None,
            long_description: None,
            license: None,
            icon: None,
            icons: None,
            homepage: None,
            documentation: None,
            support: None,
            repository: None,
            keywords: None,
            tools: None,
            prompts: None,
            tools_generated: None,
            prompts_generated: None,
            user_config,
            system_config,
            compatibility: None,
            privacy_policies: None,
            localization: None,
            meta,
            bundle_path: None,
        }
    }

    /// Create a manifest for reference mode (no scaffolding).
    fn new_reference(transport: McpbTransport) -> Self {
        let (mcp_config, system_config) = match transport {
            McpbTransport::Stdio => (
                Some(McpbMcpConfig {
                    command: Some("TODO".to_string()),
                    args: vec![],
                    env: BTreeMap::new(),
                    url: None,
                    headers: BTreeMap::new(),
                    oauth_config: None,
                }),
                None,
            ),
            McpbTransport::Http => {
                let mut sys_cfg = BTreeMap::new();
                sys_cfg.insert(
                    "port".to_string(),
                    McpbSystemConfigField {
                        field_type: McpbSystemConfigType::Port,
                        title: "Server Port".to_string(),
                        description: Some("Port for the MCP HTTP endpoint".to_string()),
                        required: None,
                        default: Some(serde_json::json!(3000)),
                    },
                );
                sys_cfg.insert(
                    "hostname".to_string(),
                    McpbSystemConfigField {
                        field_type: McpbSystemConfigType::Hostname,
                        title: "Bind Address".to_string(),
                        description: Some("Network interface to bind to".to_string()),
                        required: None,
                        default: Some(serde_json::json!("127.0.0.1")),
                    },
                );
                (
                    Some(McpbMcpConfig {
                        command: None,
                        args: vec![],
                        env: BTreeMap::new(),
                        url: Some(
                            "http://${system_config.hostname}:${system_config.port}/mcp"
                                .to_string(),
                        ),
                        headers: BTreeMap::new(),
                        oauth_config: None,
                    }),
                    Some(sys_cfg),
                )
            }
        };

        Self {
            manifest_version: "0.3".to_string(),
            name: None,
            version: Some("0.1.0".to_string()),
            description: Some("An MCP server".to_string()),
            author: None,
            server: McpbServer {
                server_type: None,
                transport,
                entry_point: None,
                mcp_config,
            },
            display_name: None,
            long_description: None,
            license: None,
            icon: None,
            icons: None,
            homepage: None,
            documentation: None,
            support: None,
            repository: None,
            keywords: None,
            tools: None,
            prompts: None,
            tools_generated: None,
            prompts_generated: None,
            user_config: None,
            system_config,
            compatibility: None,
            privacy_policies: None,
            localization: None,
            meta: None,
            bundle_path: None,
        }
    }

    /// Create a new manifest for a Rust binary project.
    pub fn new_rust(name: &str) -> Self {
        Self::new_rust_with_transport(name, McpbTransport::Stdio)
    }

    /// Create a new manifest for a Rust binary project with specified transport.
    pub fn new_rust_with_transport(name: &str, transport: McpbTransport) -> Self {
        let platform = detect_platform();

        // Determine binary name and command based on platform
        let (binary_name, command) = match platform {
            McpbPlatform::Win32 => (
                format!("target/release/{}.exe", name),
                format!("${{__dirname}}\\target\\release\\{}.exe", name),
            ),
            _ => (
                format!("target/release/{}", name),
                format!("${{__dirname}}/target/release/{}", name),
            ),
        };

        let (mcp_config, system_config) = match transport {
            McpbTransport::Stdio => (
                Some(McpbMcpConfig {
                    command: Some(command),
                    args: vec![],
                    env: BTreeMap::new(),
                    url: None,
                    headers: BTreeMap::new(),
                    oauth_config: None,
                }),
                None,
            ),
            McpbTransport::Http => {
                let mut sys_cfg = BTreeMap::new();
                sys_cfg.insert(
                    "port".to_string(),
                    McpbSystemConfigField {
                        field_type: McpbSystemConfigType::Port,
                        title: "Server Port".to_string(),
                        description: Some("Port for the MCP HTTP endpoint".to_string()),
                        required: None,
                        default: Some(serde_json::json!(3000)),
                    },
                );
                sys_cfg.insert(
                    "hostname".to_string(),
                    McpbSystemConfigField {
                        field_type: McpbSystemConfigType::Hostname,
                        title: "Bind Address".to_string(),
                        description: Some("Network interface to bind to".to_string()),
                        required: None,
                        default: Some(serde_json::json!("127.0.0.1")),
                    },
                );
                (
                    Some(McpbMcpConfig {
                        command: Some(command),
                        args: vec![
                            "--port=${system_config.port}".to_string(),
                            "--host=${system_config.hostname}".to_string(),
                        ],
                        env: BTreeMap::new(),
                        url: Some(
                            "http://${system_config.hostname}:${system_config.port}/mcp"
                                .to_string(),
                        ),
                        headers: BTreeMap::new(),
                        oauth_config: None,
                    }),
                    Some(sys_cfg),
                )
            }
        };

        Self {
            manifest_version: "0.3".to_string(),
            name: Some(name.to_string()),
            version: Some("0.1.0".to_string()),
            description: Some("An MCP server".to_string()),
            author: None,
            server: McpbServer {
                server_type: Some(McpbServerType::Binary),
                transport,
                entry_point: Some(binary_name),
                mcp_config,
            },
            display_name: None,
            long_description: None,
            license: None,
            icon: None,
            icons: None,
            homepage: None,
            documentation: None,
            support: None,
            repository: None,
            keywords: None,
            tools: None,
            prompts: None,
            tools_generated: None,
            prompts_generated: None,
            user_config: None,
            system_config,
            compatibility: Some(McpbCompatibility {
                claude_desktop: None,
                platforms: Some(vec![platform]),
                runtimes: None,
            }),
            privacy_policies: None,
            localization: None,
            meta: Some(serde_json::json!({
                "company.superrad.mcpb": {
                    "scripts": {
                        "build": "cargo build --release"
                    }
                }
            })),
            bundle_path: None,
        }
    }

    /// Set the package name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the author.
    pub fn with_author(mut self, author: McpbAuthor) -> Self {
        self.author = Some(author);
        self
    }

    /// Set the license.
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    /// Serialize to pretty-printed JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Get user_config schema if present.
    pub fn user_config_schema(&self) -> Option<&BTreeMap<String, McpbUserConfigField>> {
        self.user_config.as_ref()
    }

    /// Get system_config schema if present.
    pub fn system_config_schema(&self) -> Option<&BTreeMap<String, McpbSystemConfigField>> {
        self.system_config.as_ref()
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Detect the current platform.
pub fn detect_platform() -> McpbPlatform {
    if cfg!(target_os = "windows") {
        McpbPlatform::Win32
    } else if cfg!(target_os = "macos") {
        McpbPlatform::Darwin
    } else {
        McpbPlatform::Linux
    }
}

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

/// Package manager selection (language-specific).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// Node.js package manager.
    Node(NodePackageManager),
    /// Python package manager.
    Python(PythonPackageManager),
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

impl std::fmt::Display for PythonPackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uv => write!(f, "uv"),
            Self::Pip => write!(f, "pip"),
            Self::Poetry => write!(f, "poetry"),
        }
    }
}

impl std::fmt::Display for PackageManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Node(pm) => write!(f, "{}", pm),
            Self::Python(pm) => write!(f, "{}", pm),
        }
    }
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
    /// Bind address/hostname.
    Hostname,
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
