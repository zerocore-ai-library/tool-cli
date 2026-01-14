//! Tool and prompt validation.

use crate::mcpb::McpbManifest;
use std::collections::HashSet;

use super::super::codes::{ErrorCode, WarningCode};
use super::super::result::{ValidationIssue, ValidationResult};
use super::standard::check_extra_fields;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Allowed fields in top-level tools per MCPB spec.
const ALLOWED_TOOL_FIELDS: &[&str] = &["name", "description"];

/// Allowed fields in top-level prompts per MCPB spec.
const ALLOWED_PROMPT_FIELDS: &[&str] = &["name", "description", "arguments", "text"];

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate tool and prompt declarations (top-level and static_responses).
pub fn validate_tools(
    manifest: &McpbManifest,
    raw_json: &serde_json::Value,
    result: &mut ValidationResult,
) {
    let mut top_level_names: HashSet<String> = HashSet::new();

    // 1. Validate top-level tools array
    if let Some(tools) = &manifest.tools {
        let raw_tools = raw_json
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);

        for (i, tool) in tools.iter().enumerate() {
            let location = format!("manifest.json:tools[{}]", i);

            // Check for extra fields
            if let Some(obj) = raw_tools.get(i).and_then(|t| t.as_object()) {
                check_extra_fields(obj, ALLOWED_TOOL_FIELDS, &location, "tool", result);
            }

            // Check name is non-empty
            if tool.name.is_empty() {
                result.errors.push(ValidationIssue {
                    code: ErrorCode::ToolMissingName.into(),
                    message: "tool missing name".into(),
                    location: location.clone(),
                    details: "tool `name` field is required and cannot be empty".into(),
                    help: Some("add a unique name for this tool".into()),
                });
            } else {
                // Check for duplicate names
                if !top_level_names.insert(tool.name.clone()) {
                    result.errors.push(ValidationIssue {
                        code: ErrorCode::DuplicateToolName.into(),
                        message: "duplicate tool name".into(),
                        location: location.clone(),
                        details: format!("tool name `{}` is already declared", tool.name),
                        help: Some("use unique names for each tool".into()),
                    });
                }
            }

            // Check description is non-empty
            if tool.description.is_empty() {
                result.errors.push(ValidationIssue {
                    code: ErrorCode::ToolMissingDescription.into(),
                    message: "tool missing description".into(),
                    location,
                    details: format!(
                        "tool `{}` is missing a description",
                        if tool.name.is_empty() {
                            format!("tools[{}]", i)
                        } else {
                            tool.name.clone()
                        }
                    ),
                    help: Some("add a description explaining what the tool does".into()),
                });
            }
        }
    }

    // 2. Validate top-level prompts array for extra fields
    if let Some(raw_prompts) = raw_json.get("prompts").and_then(|p| p.as_array()) {
        for (i, raw_prompt) in raw_prompts.iter().enumerate() {
            if let Some(obj) = raw_prompt.as_object() {
                check_extra_fields(
                    obj,
                    ALLOWED_PROMPT_FIELDS,
                    &format!("manifest.json:prompts[{}]", i),
                    "prompt",
                    result,
                );
            }
        }
    }

    // 3. Validate static_responses tools/list if present
    if let Some(static_responses) = manifest.static_responses()
        && let Some(tools_list) = &static_responses.tools_list
    {
        let mut static_names: HashSet<String> = HashSet::new();

        for (i, tool) in tools_list.tools.iter().enumerate() {
            let location = format!(
                "manifest.json:_meta[\"store.tool.mcpb\"][\"static_responses\"][\"tools/list\"].tools[{}]",
                i
            );

            // Check name is non-empty
            if tool.name.is_empty() {
                result.errors.push(ValidationIssue {
                    code: ErrorCode::ToolMissingName.into(),
                    message: "static tool missing name".into(),
                    location: location.clone(),
                    details: "tool `name` field is required and cannot be empty".into(),
                    help: Some("add a unique name for this tool".into()),
                });
            } else {
                static_names.insert(tool.name.clone());

                // Warn if static tool is not in top-level tools
                if !top_level_names.contains(&tool.name) {
                    result.warnings.push(ValidationIssue {
                        code: WarningCode::StaticToolNotInTopLevel.into(),
                        message: "static tool not in top-level".into(),
                        location: location.clone(),
                        details: format!(
                            "tool `{}` in static_responses is not declared in top-level `tools`",
                            tool.name
                        ),
                        help: Some("add this tool to the top-level `tools` array".into()),
                    });
                }
            }

            // Check description is non-empty
            if tool.description.is_empty() {
                result.errors.push(ValidationIssue {
                    code: ErrorCode::ToolMissingDescription.into(),
                    message: "static tool missing description".into(),
                    location: location.clone(),
                    details: format!(
                        "tool `{}` is missing a description",
                        if tool.name.is_empty() {
                            format!("tools[{}]", i)
                        } else {
                            tool.name.clone()
                        }
                    ),
                    help: Some("add a description explaining what the tool does".into()),
                });
            }

            // Validate inputSchema if present
            if let Some(input_schema) = &tool.input_schema {
                validate_json_schema(input_schema, &tool.name, "inputSchema", result);
            }

            // Validate outputSchema if present
            if let Some(output_schema) = &tool.output_schema {
                validate_json_schema(output_schema, &tool.name, "outputSchema", result);
            }
        }

        // Warn about top-level tools missing from static_responses
        for name in &top_level_names {
            if !static_names.contains(name) {
                result.warnings.push(ValidationIssue {
                    code: WarningCode::TopLevelToolMissingSchema.into(),
                    message: "tool missing schema".into(),
                    location: format!("manifest.json:tools[name=\"{}\"]", name),
                    details: format!(
                        "tool `{}` is declared in top-level but has no schema in static_responses",
                        name
                    ),
                    help: Some(
                        "add this tool to static_responses[\"tools/list\"] with inputSchema".into(),
                    ),
                });
            }
        }
    }
}

/// Validate a JSON Schema object.
fn validate_json_schema(
    schema: &serde_json::Value,
    tool_name: &str,
    field_name: &str,
    result: &mut ValidationResult,
) {
    // Schema must be an object
    if !schema.is_object() {
        result.errors.push(ValidationIssue {
            code: ErrorCode::InvalidInputSchema.into(),
            message: format!("invalid {}", field_name),
            location: format!(
                "manifest.json:_meta[\"store.tool.mcpb\"][\"static_responses\"][\"tools/list\"].tools[name=\"{}\"].{}",
                tool_name, field_name
            ),
            details: format!(
                "`{}` must be a JSON Schema object, got {}",
                field_name,
                schema_type_name(schema)
            ),
            help: Some("use a valid JSON Schema object with `type`, `properties`, etc.".into()),
        });
        return;
    }

    // If it's an object type schema, validate structure
    let schema_obj = schema.as_object().unwrap();

    // Validate properties if present
    if let Some(properties) = schema_obj.get("properties")
        && !properties.is_object()
    {
        result.errors.push(ValidationIssue {
            code: ErrorCode::InvalidInputSchema.into(),
            message: format!("invalid {} properties", field_name),
            location: format!(
                "manifest.json:_meta[\"store.tool.mcpb\"][\"static_responses\"][\"tools/list\"].tools[name=\"{}\"].{}.properties",
                tool_name, field_name
            ),
            details: "`properties` must be an object".into(),
            help: Some("define properties as key-value pairs of property schemas".into()),
        });
    }

    // Validate required if present
    if let Some(required) = schema_obj.get("required")
        && !required.is_array()
    {
        result.errors.push(ValidationIssue {
            code: ErrorCode::InvalidInputSchema.into(),
            message: format!("invalid {} required", field_name),
            location: format!(
                "manifest.json:_meta[\"store.tool.mcpb\"][\"static_responses\"][\"tools/list\"].tools[name=\"{}\"].{}.required",
                tool_name, field_name
            ),
            details: "`required` must be an array of property names".into(),
            help: Some("use an array of strings, e.g., [\"param1\", \"param2\"]".into()),
        });
    }
}

/// Get a human-readable name for a JSON value type.
fn schema_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
