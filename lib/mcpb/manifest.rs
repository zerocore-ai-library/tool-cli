//! MCPB manifest structure and methods.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::vars;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::init_mode::InitMode;
use super::platform::{detect_platform, resolve_platform_overrides};
use super::resolved::{ResolvedMcpConfig, ResolvedMcpbManifest};
use super::types::{
    McpbAuthor, McpbCompatibility, McpbIcon, McpbLocalization, McpbMcpConfig, McpbPlatform,
    McpbPrompt, McpbRepository, McpbServer, McpbServerType, McpbSystemConfigField,
    McpbSystemConfigType, McpbTool, McpbTransport, McpbUserConfigField, McpbUserConfigType,
    NodePackageManager, PackageManager, PythonPackageManager, Scripts, StaticResponses,
};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

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

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

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
            .get("store.tool.mcpb")?
            .get("static_responses")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Get scripts from _meta if present.
    pub fn scripts(&self) -> Option<Scripts> {
        self.meta
            .as_ref()?
            .get("store.tool.mcpb")?
            .get("scripts")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Resolve the manifest with user/system config values.
    ///
    /// This method:
    /// 1. Applies platform-specific overrides to mcp_config
    /// 2. Evaluates template expressions (${__dirname}, ${user_config.*}, etc.)
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
            // Apply platform-specific overrides before template evaluation
            let cfg = resolve_platform_overrides(cfg, self.meta.as_ref());

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
                        platform_overrides: BTreeMap::new(),
                    }),
                    None,
                    None,
                    Some(serde_json::json!({
                        "store.tool.mcpb": {
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
                            platform_overrides: BTreeMap::new(),
                        }),
                        None,
                        None,
                        Some(serde_json::json!({
                            "store.tool.mcpb": {
                                "scripts": {
                                    "build": build_cmd
                                }
                            }
                        })),
                    )
                }
                (McpbServerType::Binary, McpbTransport::Stdio) => (None, None, None, None, None),
                // Bundle HTTP modes - use system_config for port, user_config for host
                (McpbServerType::Node, McpbTransport::Http) => {
                    let sys_cfg = create_http_system_config();
                    let user_cfg = create_http_user_config();
                    (
                        Some("server/index.js".to_string()),
                        Some(McpbMcpConfig {
                            command: Some("node".to_string()),
                            args: vec![
                                "${__dirname}/server/index.js".to_string(),
                                "--port=${system_config.port}".to_string(),
                                "--host=${user_config.host}".to_string(),
                            ],
                            env: BTreeMap::new(),
                            url: Some(
                                "http://${user_config.host}:${system_config.port}/mcp".to_string(),
                            ),
                            headers: BTreeMap::new(),
                            oauth_config: None,
                            platform_overrides: BTreeMap::new(),
                        }),
                        Some(user_cfg),
                        Some(sys_cfg),
                        Some(serde_json::json!({
                            "store.tool.mcpb": {
                                "scripts": {
                                    "build": build_cmd
                                }
                            }
                        })),
                    )
                }
                (McpbServerType::Python, McpbTransport::Http) => {
                    let sys_cfg = create_http_system_config();
                    let user_cfg = create_http_user_config();

                    let mut args: Vec<String> = python_pm
                        .run_args_prefix()
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect();
                    args.push("server/main.py".to_string());
                    args.push("--port".to_string());
                    args.push("${system_config.port}".to_string());
                    args.push("--host".to_string());
                    args.push("${user_config.host}".to_string());

                    (
                        Some("server/main.py".to_string()),
                        Some(McpbMcpConfig {
                            command: Some(python_pm.run_command().to_string()),
                            args,
                            env: BTreeMap::new(),
                            url: Some(
                                "http://${user_config.host}:${system_config.port}/mcp".to_string(),
                            ),
                            headers: BTreeMap::new(),
                            oauth_config: None,
                            platform_overrides: BTreeMap::new(),
                        }),
                        Some(user_cfg),
                        Some(sys_cfg),
                        Some(serde_json::json!({
                            "store.tool.mcpb": {
                                "scripts": {
                                    "build": build_cmd
                                }
                            }
                        })),
                    )
                }
                (McpbServerType::Binary, McpbTransport::Http) => {
                    let sys_cfg = create_http_system_config();
                    let user_cfg = create_http_user_config();
                    (None, None, Some(user_cfg), Some(sys_cfg), None)
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
        let (mcp_config, user_config, system_config) = match transport {
            McpbTransport::Stdio => (
                Some(McpbMcpConfig {
                    command: Some("TODO".to_string()),
                    args: vec![],
                    env: BTreeMap::new(),
                    url: None,
                    headers: BTreeMap::new(),
                    oauth_config: None,
                    platform_overrides: BTreeMap::new(),
                }),
                None,
                None,
            ),
            McpbTransport::Http => {
                let sys_cfg = create_http_system_config();
                let user_cfg = create_http_user_config();
                (
                    Some(McpbMcpConfig {
                        command: None,
                        args: vec![],
                        env: BTreeMap::new(),
                        url: Some(
                            "http://${user_config.host}:${system_config.port}/mcp".to_string(),
                        ),
                        headers: BTreeMap::new(),
                        oauth_config: None,
                        platform_overrides: BTreeMap::new(),
                    }),
                    Some(user_cfg),
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
            user_config,
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
                format!("dist/{}.exe", name),
                format!("${{__dirname}}\\dist\\{}.exe", name),
            ),
            _ => (
                format!("dist/{}", name),
                format!("${{__dirname}}/dist/{}", name),
            ),
        };

        let (mcp_config, user_config, system_config) = match transport {
            McpbTransport::Stdio => (
                Some(McpbMcpConfig {
                    command: Some(command),
                    args: vec![],
                    env: BTreeMap::new(),
                    url: None,
                    headers: BTreeMap::new(),
                    oauth_config: None,
                    platform_overrides: BTreeMap::new(),
                }),
                None,
                None,
            ),
            McpbTransport::Http => {
                let sys_cfg = create_http_system_config();
                let user_cfg = create_http_user_config();
                (
                    Some(McpbMcpConfig {
                        command: Some(command),
                        args: vec![
                            "--port=${system_config.port}".to_string(),
                            "--host=${user_config.host}".to_string(),
                        ],
                        env: BTreeMap::new(),
                        url: Some(
                            "http://${user_config.host}:${system_config.port}/mcp".to_string(),
                        ),
                        headers: BTreeMap::new(),
                        oauth_config: None,
                        platform_overrides: BTreeMap::new(),
                    }),
                    Some(user_cfg),
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
            user_config,
            system_config,
            compatibility: Some(McpbCompatibility {
                claude_desktop: None,
                platforms: Some(vec![platform.clone()]),
                runtimes: None,
            }),
            privacy_policies: None,
            localization: None,
            meta: Some(serde_json::json!({
                "store.tool.mcpb": {
                    "scripts": {
                        "build": match platform {
                            McpbPlatform::Win32 => format!(
                                "cargo build --release && if not exist dist mkdir dist && copy target\\release\\{}.exe dist\\",
                                name
                            ),
                            _ => format!(
                                "cargo build --release && mkdir -p dist && cp target/release/{} dist/",
                                name
                            ),
                        }
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

    /// Returns true if this manifest requires the `.mcpbx` format.
    ///
    /// A manifest requires `.mcpbx` if it uses any feature beyond the base MCPB spec:
    /// - Reference mode (no `entry_point` or no `type`)
    /// - HTTP transport
    /// - `system_config`
    /// - `mcp_config.url`, `mcp_config.headers`, `mcp_config.oauth_config`
    pub fn requires_mcpbx(&self) -> bool {
        // Reference mode: entry_point or type absent
        if self.server.entry_point.is_none() {
            return true;
        }
        if self.server.server_type.is_none() {
            return true;
        }
        // HTTP transport
        if self.server.transport == McpbTransport::Http {
            return true;
        }
        // system_config present
        if self.system_config.is_some() {
            return true;
        }
        // mcp_config extensions
        if let Some(ref cfg) = self.server.mcp_config
            && (cfg.url.is_some() || !cfg.headers.is_empty() || cfg.oauth_config.is_some())
        {
            return true;
        }
        false
    }

    /// Get the appropriate bundle file extension (`"mcpb"` or `"mcpbx"`).
    pub fn bundle_extension(&self) -> &'static str {
        if self.requires_mcpbx() {
            "mcpbx"
        } else {
            "mcpb"
        }
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

/// Create the standard HTTP system config (port only).
fn create_http_system_config() -> BTreeMap<String, McpbSystemConfigField> {
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
    sys_cfg
}

/// Create the standard HTTP user config (host only).
fn create_http_user_config() -> BTreeMap<String, McpbUserConfigField> {
    let mut user_cfg = BTreeMap::new();
    user_cfg.insert(
        "host".to_string(),
        McpbUserConfigField {
            field_type: McpbUserConfigType::String,
            title: "Bind Address".to_string(),
            description: Some("Network interface to bind to".to_string()),
            required: None,
            default: Some(serde_json::json!("127.0.0.1")),
            multiple: None,
            sensitive: None,
            enum_values: None,
            min: None,
            max: None,
        },
    );
    user_cfg
}
