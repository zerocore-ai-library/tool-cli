//! Reusable output types for CLI commands.
//!
//! These types provide a consistent structure for JSON output across commands
//! like `list`, `info`, and `grep`. All collections use object-keyed structures
//! (BTreeMap) instead of arrays for self-describing paths.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::mcp::ToolCapabilities;

//--------------------------------------------------------------------------------------------------
// Types: List Output
//--------------------------------------------------------------------------------------------------

/// Server entry for `tool list --json` (object-keyed by server name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerOutput {
    #[serde(rename = "type")]
    pub server_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub location: String,
}

//--------------------------------------------------------------------------------------------------
// Types: Info Output
//--------------------------------------------------------------------------------------------------

/// Output for `tool info --json` command - detailed tool information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfoOutput {
    pub server: ToolServerInfo,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub manifest_path: String,
    pub tools: BTreeMap<String, ToolOutput>,
    pub prompts: BTreeMap<String, PromptOutput>,
    pub resources: BTreeMap<String, ResourceOutput>,
}

/// Server information from MCP handshake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolServerInfo {
    pub name: String,
    pub version: String,
}

/// Individual tool information (keyed by tool name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Arc<serde_json::Map<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Arc<serde_json::Map<String, serde_json::Value>>>,
}

/// Prompt information (keyed by prompt name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Resource information (keyed by resource name).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub uri: String,
}

//--------------------------------------------------------------------------------------------------
// Types: Full Output (list --full)
//--------------------------------------------------------------------------------------------------

/// Full server info for `tool list --json --full` (object-keyed by server name).
/// Combines list metadata with info details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullServerOutput {
    #[serde(rename = "type")]
    pub server_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub location: String,
    pub server: ToolServerInfo,
    pub tools: BTreeMap<String, ToolOutput>,
    pub prompts: BTreeMap<String, PromptOutput>,
    pub resources: BTreeMap<String, ResourceOutput>,
}

//--------------------------------------------------------------------------------------------------
// Types: Grep Output
//--------------------------------------------------------------------------------------------------

/// Grep match result with JavaScript accessor path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepMatch {
    pub path: String,
    pub value: String,
}

/// Grep output containing pattern and all matches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepOutput {
    pub pattern: String,
    pub matches: Vec<GrepMatch>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl ServerOutput {
    /// Create a new server output entry.
    pub fn new(
        server_type: impl Into<String>,
        description: Option<String>,
        location: impl Into<String>,
    ) -> Self {
        Self {
            server_type: server_type.into(),
            description,
            location: location.into(),
        }
    }
}

impl ToolInfoOutput {
    /// Create from ToolCapabilities and metadata.
    pub fn from_capabilities(
        capabilities: &ToolCapabilities,
        tool_type: impl Into<String>,
        manifest_path: &Path,
    ) -> Self {
        Self {
            server: ToolServerInfo {
                name: capabilities.server_info.name.clone(),
                version: capabilities.server_info.version.clone(),
            },
            tool_type: tool_type.into(),
            manifest_path: manifest_path.display().to_string(),
            tools: capabilities
                .tools
                .iter()
                .map(|t| {
                    (
                        t.name.to_string(),
                        ToolOutput {
                            description: t.description.as_ref().map(|d| d.to_string()),
                            input_schema: t.input_schema.clone(),
                            output_schema: t.output_schema.clone(),
                        },
                    )
                })
                .collect(),
            prompts: capabilities
                .prompts
                .iter()
                .map(|p| {
                    (
                        p.name.to_string(),
                        PromptOutput {
                            description: p.description.as_ref().map(|d| d.to_string()),
                        },
                    )
                })
                .collect(),
            resources: capabilities
                .resources
                .iter()
                .map(|r| {
                    (
                        r.name.to_string(),
                        ResourceOutput {
                            description: r.description.as_ref().map(|d| d.to_string()),
                            uri: r.uri.to_string(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Serialize to pretty JSON string.
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

impl FullServerOutput {
    /// Create from server metadata and ToolCapabilities.
    pub fn from_capabilities(
        server_type: impl Into<String>,
        description: Option<String>,
        location: impl Into<String>,
        capabilities: &ToolCapabilities,
    ) -> Self {
        Self {
            server_type: server_type.into(),
            description,
            location: location.into(),
            server: ToolServerInfo {
                name: capabilities.server_info.name.clone(),
                version: capabilities.server_info.version.clone(),
            },
            tools: capabilities
                .tools
                .iter()
                .map(|t| {
                    (
                        t.name.to_string(),
                        ToolOutput {
                            description: t.description.as_ref().map(|d| d.to_string()),
                            input_schema: t.input_schema.clone(),
                            output_schema: t.output_schema.clone(),
                        },
                    )
                })
                .collect(),
            prompts: capabilities
                .prompts
                .iter()
                .map(|p| {
                    (
                        p.name.to_string(),
                        PromptOutput {
                            description: p.description.as_ref().map(|d| d.to_string()),
                        },
                    )
                })
                .collect(),
            resources: capabilities
                .resources
                .iter()
                .map(|r| {
                    (
                        r.name.to_string(),
                        ResourceOutput {
                            description: r.description.as_ref().map(|d| d.to_string()),
                            uri: r.uri.to_string(),
                        },
                    )
                })
                .collect(),
        }
    }
}

impl GrepOutput {
    /// Create a new grep output.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            matches: Vec::new(),
        }
    }

    /// Add a match.
    pub fn add_match(&mut self, path: impl Into<String>, value: impl Into<String>) {
        self.matches.push(GrepMatch {
            path: path.into(),
            value: value.into(),
        });
    }

    /// Serialize to JSON string.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// Serialize to pretty JSON string.
    pub fn to_json_pretty(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Serialize a BTreeMap of ServerOutput to JSON.
pub fn list_to_json(items: &BTreeMap<String, ServerOutput>) -> serde_json::Result<String> {
    serde_json::to_string(items)
}

/// Serialize a BTreeMap of ServerOutput to pretty JSON.
pub fn list_to_json_pretty(items: &BTreeMap<String, ServerOutput>) -> serde_json::Result<String> {
    serde_json::to_string_pretty(items)
}

/// Serialize a BTreeMap of FullServerOutput to JSON.
pub fn full_list_to_json(items: &BTreeMap<String, FullServerOutput>) -> serde_json::Result<String> {
    serde_json::to_string(items)
}

/// Serialize a BTreeMap of FullServerOutput to pretty JSON.
pub fn full_list_to_json_pretty(
    items: &BTreeMap<String, FullServerOutput>,
) -> serde_json::Result<String> {
    serde_json::to_string_pretty(items)
}

//--------------------------------------------------------------------------------------------------
// Functions: Path Building
//--------------------------------------------------------------------------------------------------

/// Build a JavaScript accessor path segment for a key.
/// Uses bracket notation with single quotes for keys with special characters.
pub fn js_path_key(key: &str) -> String {
    // Check if key needs bracket notation (contains special chars)
    if key.contains('/') || key.contains('-') || key.contains('.') || key.contains(' ') {
        format!("['{}']", key)
    } else {
        format!(".{}", key)
    }
}

/// Build a JavaScript accessor path for a server.
pub fn js_path_server(server_name: &str) -> String {
    format!("['{}']", server_name)
}

/// Build a JavaScript accessor path for a server property.
pub fn js_path_server_prop(server_name: &str, prop: &str) -> String {
    format!("['{}'].{}", server_name, prop)
}

/// Build a JavaScript accessor path for a tool.
pub fn js_path_tool(server_name: &str, tool_name: &str) -> String {
    format!("['{}'].tools{}", server_name, js_path_key(tool_name))
}

/// Build a JavaScript accessor path for a tool property.
pub fn js_path_tool_prop(server_name: &str, tool_name: &str, prop: &str) -> String {
    format!(
        "['{}'].tools{}.{}",
        server_name,
        js_path_key(tool_name),
        prop
    )
}

/// Build a JavaScript accessor path for a schema field.
pub fn js_path_schema_field(
    server_name: &str,
    tool_name: &str,
    schema_type: &str, // "input_schema" or "output_schema"
    field_path: &str,
) -> String {
    format!(
        "['{}'].tools{}.{}.properties{}",
        server_name,
        js_path_key(tool_name),
        schema_type,
        js_path_key(field_path)
    )
}

/// Build a JavaScript accessor path for a schema field property.
pub fn js_path_schema_field_prop(
    server_name: &str,
    tool_name: &str,
    schema_type: &str,
    field_path: &str,
    prop: &str,
) -> String {
    format!(
        "['{}'].tools{}.{}.properties{}.{}",
        server_name,
        js_path_key(tool_name),
        schema_type,
        js_path_key(field_path),
        prop
    )
}
