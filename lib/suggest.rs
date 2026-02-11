//! Fuzzy matching utilities for tool suggestions.

use rmcp::service::ServiceError;
use strsim::jaro_winkler;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Information about a tool parameter.
#[derive(Debug, Clone)]
pub struct ParamInfo {
    /// Parameter name.
    pub name: String,
    /// Parameter type (e.g., "string", "number").
    pub param_type: String,
    /// Parameter description.
    pub description: Option<String>,
    /// Whether the parameter is required.
    pub required: bool,
}

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Minimum similarity threshold for suggestions (0.0 to 1.0).
const MIN_SIMILARITY: f64 = 0.6;

/// Maximum number of suggestions to show.
const MAX_SUGGESTIONS: usize = 3;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Find similar tool names using fuzzy matching.
///
/// Returns a list of suggestions sorted by similarity (best match first).
pub fn find_similar_tools(query: &str, available_tools: &[String]) -> Vec<String> {
    let mut scored: Vec<(String, f64)> = available_tools
        .iter()
        .map(|tool| {
            let score = jaro_winkler(query, tool);
            (tool.clone(), score)
        })
        .filter(|(_, score)| *score >= MIN_SIMILARITY)
        .collect();

    // Sort by score descending
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Take top suggestions
    scored
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(name, _)| name)
        .collect()
}

/// Format suggestions in cargo-style.
///
/// Returns None if no suggestions are available.
pub fn format_suggestions(suggestions: &[String]) -> Option<String> {
    match suggestions.len() {
        0 => None,
        1 => Some(format!("Did you mean `{}`?", suggestions[0])),
        _ => {
            let formatted: Vec<String> = suggestions.iter().map(|s| format!("`{}`", s)).collect();
            Some(format!("Did you mean one of: {}?", formatted.join(", ")))
        }
    }
}

/// Extract the unknown tool name from an MCP error message.
///
/// Parses error messages like: "Tool call failed: Mcp error: -32602: unknown tool \"me\""
pub fn extract_unknown_tool_from_error(error_msg: &str) -> Option<String> {
    // Pattern: unknown tool "xxx" or unknown tool 'xxx'
    if let Some(start) = error_msg.find("unknown tool") {
        let rest = &error_msg[start + 12..]; // skip "unknown tool"
        // Find the quoted tool name
        if let Some(quote_start) = rest.find('"')
            && let Some(quote_end) = rest[quote_start + 1..].find('"')
        {
            return Some(rest[quote_start + 1..quote_start + 1 + quote_end].to_string());
        }
        // Try single quotes
        if let Some(quote_start) = rest.find('\'')
            && let Some(quote_end) = rest[quote_start + 1..].find('\'')
        {
            return Some(rest[quote_start + 1..quote_start + 1 + quote_end].to_string());
        }
    }
    None
}

/// Extract the missing parameter name from an MCP error message.
///
/// Parses error messages like: "missing required parameter: owner"
pub fn extract_missing_param_from_error(error_msg: &str) -> Option<String> {
    // Pattern: "missing required parameter: xxx"
    if let Some(start) = error_msg.find("missing required parameter:") {
        let rest = error_msg[start + 27..].trim(); // skip "missing required parameter:"
        // Take until end of line or end of string
        let param = rest.split_whitespace().next()?;
        return Some(param.to_string());
    }
    None
}

/// Check if an error is a missing parameter error.
pub fn is_missing_param_error(error_msg: &str) -> bool {
    error_msg.contains("missing required parameter")
        || error_msg.contains("missing field")
        || error_msg.contains("failed to deserialize parameters")
}

//--------------------------------------------------------------------------------------------------
// Functions: Typed Error Handling
//--------------------------------------------------------------------------------------------------

/// JSON-RPC error code for invalid params.
const INVALID_PARAMS: i32 = -32602;

/// JSON-RPC error code for method not found (unknown tool).
const METHOD_NOT_FOUND: i32 = -32601;

/// Information extracted from an MCP error.
#[derive(Debug, Clone)]
pub enum McpErrorKind {
    /// Missing required parameter with the parameter name.
    MissingParam(String),
    /// Unknown tool/method with the tool name.
    UnknownTool(String),
    /// Other error with code and message.
    Other { code: i32, message: String },
}

/// Analyze a ServiceError and extract structured information.
pub fn analyze_mcp_error(error: &ServiceError) -> Option<McpErrorKind> {
    let ServiceError::McpError(mcp_error) = error else {
        return None;
    };

    let code = mcp_error.code.0;
    let message = &mcp_error.message;

    match code {
        INVALID_PARAMS => {
            // Try to extract missing parameter name from message
            // Formats: "missing field `name`" or "missing required parameter: name"
            if let Some(param) = extract_missing_field_from_message(message) {
                return Some(McpErrorKind::MissingParam(param));
            }
            Some(McpErrorKind::Other {
                code,
                message: message.to_string(),
            })
        }
        METHOD_NOT_FOUND => {
            // Try to extract unknown tool name
            if let Some(tool) = extract_unknown_tool_from_error(message) {
                return Some(McpErrorKind::UnknownTool(tool));
            }
            Some(McpErrorKind::Other {
                code,
                message: message.to_string(),
            })
        }
        _ => Some(McpErrorKind::Other {
            code,
            message: message.to_string(),
        }),
    }
}

/// Extract missing field name from various error message formats.
///
/// Handles:
/// - "missing field `name`" (serde)
/// - "missing required parameter: name"
pub fn extract_missing_field_from_message(message: &str) -> Option<String> {
    // Pattern: "missing field `xxx`" (serde format with backticks)
    if let Some(start) = message.find("missing field `") {
        let rest = &message[start + 15..]; // skip "missing field `"
        if let Some(end) = rest.find('`') {
            return Some(rest[..end].to_string());
        }
    }

    // Pattern: "missing required parameter: xxx"
    if let Some(start) = message.find("missing required parameter:") {
        let rest = message[start + 27..].trim(); // skip "missing required parameter:"
        let param = rest.split_whitespace().next()?;
        return Some(param.to_string());
    }

    None
}

/// Extract parameter info from a JSON Schema input_schema.
///
/// Returns a list of all parameters with their types, descriptions, and required status.
pub fn extract_params_from_schema(input_schema: &serde_json::Value) -> Vec<ParamInfo> {
    let mut params = Vec::new();

    let properties = match input_schema.get("properties") {
        Some(serde_json::Value::Object(props)) => props,
        _ => return params,
    };

    let required: Vec<String> = input_schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    for (name, prop) in properties {
        let param_type = prop
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("any")
            .to_string();

        let description = prop
            .get("description")
            .and_then(|d| d.as_str())
            .map(String::from);

        params.push(ParamInfo {
            name: name.clone(),
            param_type,
            description,
            required: required.contains(name),
        });
    }

    // Sort: required first, then alphabetically
    params.sort_by(|a, b| match (a.required, b.required) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    params
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_similar_tools() {
        let tools = vec![
            "me".to_string(),
            "get_user".to_string(),
            "get_users".to_string(),
            "create_user".to_string(),
            "delete_user".to_string(),
        ];

        // Exact match should come first
        let suggestions = find_similar_tools("get_user", &tools);
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0], "get_user");

        // Close match
        let suggestions = find_similar_tools("get_usr", &tools);
        assert!(suggestions.contains(&"get_user".to_string()));

        // No match for very different string
        let suggestions = find_similar_tools("xyz123", &tools);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_format_suggestions() {
        assert_eq!(format_suggestions(&[]), None);
        assert_eq!(
            format_suggestions(&["foo".to_string()]),
            Some("Did you mean `foo`?".to_string())
        );
        assert_eq!(
            format_suggestions(&["foo".to_string(), "bar".to_string()]),
            Some("Did you mean one of: `foo`, `bar`?".to_string())
        );
    }

    #[test]
    fn test_extract_unknown_tool_from_error() {
        assert_eq!(
            extract_unknown_tool_from_error(
                "Tool call failed: Mcp error: -32602: unknown tool \"me\""
            ),
            Some("me".to_string())
        );
        assert_eq!(
            extract_unknown_tool_from_error("unknown tool \"get_user\""),
            Some("get_user".to_string())
        );
        assert_eq!(extract_unknown_tool_from_error("some other error"), None);
    }

    #[test]
    fn test_extract_missing_param_from_error() {
        assert_eq!(
            extract_missing_param_from_error("missing required parameter: owner"),
            Some("owner".to_string())
        );
        assert_eq!(
            extract_missing_param_from_error("Tool call failed: missing required parameter: repo"),
            Some("repo".to_string())
        );
        assert_eq!(extract_missing_param_from_error("some other error"), None);
    }

    #[test]
    fn test_extract_params_from_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "owner": {
                    "type": "string",
                    "description": "Repository owner"
                },
                "repo": {
                    "type": "string",
                    "description": "Repository name"
                },
                "limit": {
                    "type": "number",
                    "description": "Max results"
                }
            },
            "required": ["owner", "repo"]
        });

        let params = extract_params_from_schema(&schema);
        assert_eq!(params.len(), 3);

        // Required params come first
        assert_eq!(params[0].name, "owner");
        assert!(params[0].required);
        assert_eq!(params[1].name, "repo");
        assert!(params[1].required);

        // Optional param last
        assert_eq!(params[2].name, "limit");
        assert!(!params[2].required);
    }
}
