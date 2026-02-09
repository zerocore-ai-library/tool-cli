//! Standard field validation for MCPB spec compliance.

use super::super::codes::ErrorCode;
use super::super::result::{ValidationIssue, ValidationResult};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Allowed fields in author per MCPB spec.
const ALLOWED_AUTHOR_FIELDS: &[&str] = &["name", "email", "url"];

/// Allowed fields in repository per MCPB spec.
const ALLOWED_REPOSITORY_FIELDS: &[&str] = &["type", "url"];

/// Allowed fields in server per MCPB spec + tool.store extensions.
const ALLOWED_SERVER_FIELDS: &[&str] = &[
    // MCPB standard
    "type",
    "entry_point",
    "mcp_config",
    // tool.store extension
    "transport",
];

/// Allowed fields in mcp_config per MCPB spec + tool.store extensions.
const ALLOWED_MCP_CONFIG_FIELDS: &[&str] = &[
    // MCPB standard
    "command",
    "args",
    "env",
    "platform_overrides",
    // tool.store extensions
    "url",
    "headers",
    "oauth_config",
];

/// Allowed fields in compatibility per MCPB spec.
const ALLOWED_COMPATIBILITY_FIELDS: &[&str] = &["claude_desktop", "platforms", "runtimes"];

/// Allowed fields in icons array items per MCPB spec.
const ALLOWED_ICON_FIELDS: &[&str] = &["src", "size", "theme"];

/// Allowed fields in user_config entries per MCPB spec.
const ALLOWED_USER_CONFIG_FIELDS: &[&str] = &[
    "type",
    "title",
    "description",
    "required",
    "default",
    "sensitive",
    "min",
    "max",
    "multiple",
    "enum",
];

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Helper to check for extra fields in a JSON object.
pub fn check_extra_fields(
    obj: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
    location: &str,
    field_type: &str,
    result: &mut ValidationResult,
) {
    let extra_fields: Vec<&String> = obj
        .keys()
        .filter(|k| !allowed.contains(&k.as_str()))
        .collect();

    if !extra_fields.is_empty() {
        let fields_str = extra_fields
            .iter()
            .map(|s| format!("`{}`", s))
            .collect::<Vec<_>>()
            .join(", ");

        result.errors.push(ValidationIssue {
            code: ErrorCode::ExtraFieldsInStandardField.into(),
            message: format!("extra fields in {}", field_type),
            location: location.to_string(),
            details: format!(
                "{} has fields {} which are not allowed in MCPB spec",
                field_type, fields_str
            ),
            help: Some(format!(
                "{} only allows: {}",
                field_type,
                allowed.join(", ")
            )),
        });
    }
}

/// Validate all standard-defined fields for extra fields.
pub fn validate_standard_fields(raw_json: &serde_json::Value, result: &mut ValidationResult) {
    // Validate author
    if let Some(author) = raw_json.get("author").and_then(|a| a.as_object()) {
        check_extra_fields(
            author,
            ALLOWED_AUTHOR_FIELDS,
            "manifest.json:author",
            "author",
            result,
        );
    }

    // Validate repository
    if let Some(repo) = raw_json.get("repository").and_then(|r| r.as_object()) {
        check_extra_fields(
            repo,
            ALLOWED_REPOSITORY_FIELDS,
            "manifest.json:repository",
            "repository",
            result,
        );
    }

    // Validate server
    if let Some(server) = raw_json.get("server").and_then(|s| s.as_object()) {
        check_extra_fields(
            server,
            ALLOWED_SERVER_FIELDS,
            "manifest.json:server",
            "server",
            result,
        );

        // Validate mcp_config within server
        if let Some(mcp_config) = server.get("mcp_config").and_then(|m| m.as_object()) {
            check_extra_fields(
                mcp_config,
                ALLOWED_MCP_CONFIG_FIELDS,
                "manifest.json:server.mcp_config",
                "mcp_config",
                result,
            );
        }
    }

    // Validate compatibility
    if let Some(compat) = raw_json.get("compatibility").and_then(|c| c.as_object()) {
        check_extra_fields(
            compat,
            ALLOWED_COMPATIBILITY_FIELDS,
            "manifest.json:compatibility",
            "compatibility",
            result,
        );
    }

    // Validate icons array
    if let Some(icons) = raw_json.get("icons").and_then(|i| i.as_array()) {
        for (i, icon) in icons.iter().enumerate() {
            if let Some(obj) = icon.as_object() {
                check_extra_fields(
                    obj,
                    ALLOWED_ICON_FIELDS,
                    &format!("manifest.json:icons[{}]", i),
                    "icon",
                    result,
                );
            }
        }
    }

    // Validate user_config entries
    if let Some(user_config) = raw_json.get("user_config").and_then(|u| u.as_object()) {
        for (key, value) in user_config {
            if let Some(obj) = value.as_object() {
                check_extra_fields(
                    obj,
                    ALLOWED_USER_CONFIG_FIELDS,
                    &format!("manifest.json:user_config.{}", key),
                    &format!("user_config.{}", key),
                    result,
                );
            }
        }
    }
}
