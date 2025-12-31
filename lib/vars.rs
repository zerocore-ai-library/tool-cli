//! Variable substitution utilities for MCP manifests.
//!
//! Handles `${__dirname}`, `${HOME}`, `${user_config.X}`, `${system_config.X}` and
//! template functions like `${base64(value)}` in mcp_config args, env, and header values.

use crate::error::{ToolError, ToolResult};
use crate::mcpb::{McpbSystemConfigField, McpbSystemConfigType, McpbUserConfigField, McpbUserConfigType};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Regex pattern for user_config variable references.
pub const USER_CONFIG_VAR_PATTERN: &str = r"\$\{user_config\.(\w+)\}";

/// Regex pattern for system_config variable references.
pub const SYSTEM_CONFIG_VAR_PATTERN: &str = r"\$\{system_config\.(\w+)\}";

/// Built-in variables that don't require config definition.
pub const BUILTIN_VARS: &[&str] = &["__dirname", "HOME", "DESKTOP", "DOCUMENTS", "DOWNLOADS"];

/// Compiled regex for user_config variable extraction.
static USER_CONFIG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(USER_CONFIG_VAR_PATTERN).expect("Invalid regex pattern"));

/// Compiled regex for system_config variable extraction.
static SYSTEM_CONFIG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(SYSTEM_CONFIG_VAR_PATTERN).expect("Invalid regex pattern"));

/// Regex for all variable patterns.
static VAR_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{([^}]+)\}").expect("Invalid regex pattern")
});

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Extract all user_config variable names from a string.
pub fn extract_user_config_vars(s: &str) -> Vec<String> {
    USER_CONFIG_REGEX
        .captures_iter(s)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Extract all system_config variable names from a string.
pub fn extract_system_config_vars(s: &str) -> Vec<String> {
    SYSTEM_CONFIG_REGEX
        .captures_iter(s)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Substitute variables in a string.
///
/// Handles:
/// - `${__dirname}` - replaced with the provided dirname
/// - `${HOME}` - user's home directory
/// - `${DESKTOP}` - user's desktop directory
/// - `${DOCUMENTS}` - user's documents directory
/// - `${DOWNLOADS}` - user's downloads directory
/// - `${user_config.X}` - replaced with value from user_config map
/// - `${system_config.X}` - replaced with value from system_config map
/// - Template functions like `${base64(value)}`, `${basicAuth(user, pass)}`
pub fn substitute_vars(
    s: &str,
    dirname: &str,
    user_config: &BTreeMap<String, String>,
    system_config: &BTreeMap<String, String>,
) -> ToolResult<String> {
    let mut result = s.to_string();
    let mut errors = Vec::new();

    // Find all ${...} patterns
    for cap in VAR_REGEX.captures_iter(s) {
        let full_match = &cap[0];
        let inner = &cap[1];

        let replacement = if inner == "__dirname" {
            Some(dirname.to_string())
        } else if inner == "HOME" {
            dirs::home_dir().map(|p| p.to_string_lossy().to_string())
        } else if inner == "DESKTOP" {
            dirs::desktop_dir().map(|p| p.to_string_lossy().to_string())
        } else if inner == "DOCUMENTS" {
            dirs::document_dir().map(|p| p.to_string_lossy().to_string())
        } else if inner == "DOWNLOADS" {
            dirs::download_dir().map(|p| p.to_string_lossy().to_string())
        } else if let Some(key) = inner.strip_prefix("user_config.") {
            user_config.get(key).cloned()
        } else if let Some(key) = inner.strip_prefix("system_config.") {
            system_config.get(key).cloned()
        } else if inner.starts_with("base64(") && inner.ends_with(')') {
            // Handle base64 encoding
            let arg = &inner[7..inner.len() - 1];
            let resolved = substitute_vars(arg, dirname, user_config, system_config)?;
            Some(base64_encode(&resolved))
        } else if inner.starts_with("basicAuth(") && inner.ends_with(')') {
            // Handle basicAuth(user, pass) -> base64(user:pass)
            let args_str = &inner[10..inner.len() - 1];
            let parts: Vec<&str> = args_str.split(',').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                let user = substitute_vars(parts[0], dirname, user_config, system_config)?;
                let pass = substitute_vars(parts[1], dirname, user_config, system_config)?;
                Some(base64_encode(&format!("{}:{}", user, pass)))
            } else {
                None
            }
        } else {
            // Try environment variable
            std::env::var(inner).ok()
        };

        match replacement {
            Some(value) => {
                result = result.replace(full_match, &value);
            }
            None => {
                errors.push(format!("Undefined variable: {}", inner));
            }
        }
    }

    if !errors.is_empty() {
        return Err(ToolError::Generic(errors.join(", ")));
    }

    Ok(result)
}

/// Base64 encode a string.
fn base64_encode(s: &str) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(s.as_bytes())
}

/// Check if a variable name is a built-in variable.
pub fn is_builtin_var(name: &str) -> bool {
    BUILTIN_VARS.contains(&name)
}

/// Validate user_config values against the schema.
pub fn validate_user_config(
    schema: &BTreeMap<String, McpbUserConfigField>,
    values: &BTreeMap<String, String>,
) -> ToolResult<()> {
    for (name, field) in schema {
        let value = values.get(name);

        // Check required fields
        if field.required.unwrap_or(false) && value.is_none() {
            return Err(ToolError::Generic(format!(
                "Required config field '{}' is missing",
                name
            )));
        }

        // Validate type and constraints
        if let Some(v) = value {
            match field.field_type {
                McpbUserConfigType::Number => {
                    let num: f64 = v.parse().map_err(|_| {
                        ToolError::Generic(format!("'{}' must be a number", name))
                    })?;
                    if let Some(min) = field.min
                        && num < min
                    {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be >= {}",
                            name, min
                        )));
                    }
                    if let Some(max) = field.max
                        && num > max
                    {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be <= {}",
                            name, max
                        )));
                    }
                }
                McpbUserConfigType::String => {
                    if let Some(ref enum_values) = field.enum_values
                        && !enum_values.contains(v)
                    {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be one of: {:?}",
                            name, enum_values
                        )));
                    }
                }
                McpbUserConfigType::Boolean => {
                    if v != "true" && v != "false" {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be 'true' or 'false'",
                            name
                        )));
                    }
                }
                McpbUserConfigType::Directory | McpbUserConfigType::File => {}
            }
        }
    }
    Ok(())
}

/// Validate system_config values against the schema.
pub fn validate_system_config(
    schema: &BTreeMap<String, McpbSystemConfigField>,
    values: &BTreeMap<String, String>,
) -> ToolResult<()> {
    for (name, field) in schema {
        let value = values.get(name);

        // Check required fields
        if field.required.unwrap_or(false) && value.is_none() {
            return Err(ToolError::Generic(format!(
                "Required system_config field '{}' is missing",
                name
            )));
        }

        // Validate type and constraints
        if let Some(v) = value {
            match field.field_type {
                McpbSystemConfigType::Port => {
                    let num: f64 = v.parse().map_err(|_| {
                        ToolError::Generic(format!("'{}' must be a number", name))
                    })?;
                    if !(1.0..=65535.0).contains(&num) {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be a valid port (1-65535)",
                            name
                        )));
                    }
                }
                McpbSystemConfigType::Hostname
                | McpbSystemConfigType::TempDirectory
                | McpbSystemConfigType::DataDirectory => {}
            }
        }
    }
    Ok(())
}
