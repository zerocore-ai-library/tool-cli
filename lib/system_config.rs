//! System configuration allocation for MCPB tools.
//!
//! Provides helpers for allocating system resources (ports, directories)
//! based on a tool's `system_config` schema.

use crate::constants::{DEFAULT_DATA_PATH, DEFAULT_TMP_PATH};
use crate::error::{ToolError, ToolResult};
use crate::mcpb::{McpbSystemConfigField, McpbSystemConfigType};
use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::PathBuf;
use uuid::Uuid;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Allocate system_config values based on a manifest schema.
///
/// Reads the schema from `system_config` in the manifest and allocates
/// real values for each declared resource (ports, directories, etc.).
pub fn allocate_system_config(
    schema: Option<&BTreeMap<String, McpbSystemConfigField>>,
) -> ToolResult<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();

    let Some(schema) = schema else {
        return Ok(result);
    };

    for (name, field) in schema {
        let value = allocate_field(field)?;
        result.insert(name.clone(), value);
    }

    Ok(result)
}

/// Allocate a single system_config field based on its type.
fn allocate_field(field: &McpbSystemConfigField) -> ToolResult<String> {
    match field.field_type {
        McpbSystemConfigType::Port => {
            let default = field
                .default
                .as_ref()
                .and_then(|v| v.as_u64())
                .map(|n| n as u16);
            let port = reserve_port(default)?;
            Ok(port.to_string())
        }
        McpbSystemConfigType::DataDirectory => {
            let dir = allocate_data_dir()?;
            Ok(dir.to_string_lossy().to_string())
        }
        McpbSystemConfigType::TempDirectory => {
            let dir = allocate_temp_dir()?;
            Ok(dir.to_string_lossy().to_string())
        }
    }
}

/// Reserve an available port.
///
/// Tries to allocate an available port first, falling back to the default
/// if OS allocation fails.
pub fn reserve_port(default: Option<u16>) -> ToolResult<u16> {
    // First try to let OS assign an available port
    if let Ok(listener) = TcpListener::bind("127.0.0.1:0")
        && let Ok(addr) = listener.local_addr()
    {
        return Ok(addr.port());
    }

    // Fall back to default port if allocation failed
    if let Some(port) = default
        && TcpListener::bind(("127.0.0.1", port)).is_ok()
    {
        return Ok(port);
    }

    Err(ToolError::Generic(
        "Failed to allocate port: no available ports".to_string(),
    ))
}

/// Check if a port is currently available.
pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Allocate a persistent data directory for a tool.
pub fn allocate_data_dir() -> ToolResult<PathBuf> {
    let id = Uuid::new_v4();
    let dir = DEFAULT_DATA_PATH.join(id.to_string());

    std::fs::create_dir_all(&dir)
        .map_err(|e| ToolError::Generic(format!("Failed to create data directory: {}", e)))?;

    Ok(dir)
}

/// Allocate an ephemeral temp directory for a tool.
pub fn allocate_temp_dir() -> ToolResult<PathBuf> {
    let id = Uuid::new_v4();
    let dir = DEFAULT_TMP_PATH.join(id.to_string());

    std::fs::create_dir_all(&dir)
        .map_err(|e| ToolError::Generic(format!("Failed to create temp directory: {}", e)))?;

    Ok(dir)
}
