//! Variable reference validation.

use crate::mcpb::McpbManifest;
use crate::vars::extract_user_config_vars;

use super::super::codes::{ErrorCode, WarningCode};
use super::super::result::{ValidationIssue, ValidationResult};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate variable references in mcp_config.
pub fn validate_variable_references(manifest: &McpbManifest, result: &mut ValidationResult) {
    if let Some(mcp_config) = &manifest.server.mcp_config {
        let user_config_keys: Vec<&str> = manifest
            .user_config
            .as_ref()
            .map(|uc| uc.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();

        // Collect all referenced variables for warning check
        let mut all_referenced: Vec<String> = Vec::new();

        // Check command
        if let Some(command) = &mcp_config.command {
            let vars =
                check_variable_references(command, &user_config_keys, "mcp_config.command", result);
            all_referenced.extend(vars);
        }

        // Check args
        for arg in &mcp_config.args {
            let vars = check_variable_references(arg, &user_config_keys, "mcp_config.args", result);
            all_referenced.extend(vars);
        }

        // Check env values
        for (env_key, value) in &mcp_config.env {
            let vars = check_variable_references(
                value,
                &user_config_keys,
                &format!("mcp_config.env.{}", env_key),
                result,
            );
            all_referenced.extend(vars);
        }

        // Check url
        if let Some(url) = &mcp_config.url {
            let vars = check_variable_references(url, &user_config_keys, "mcp_config.url", result);
            all_referenced.extend(vars);
        }

        // Check headers
        for (header_key, value) in &mcp_config.headers {
            let vars = check_variable_references(
                value,
                &user_config_keys,
                &format!("mcp_config.headers.{}", header_key),
                result,
            );
            all_referenced.extend(vars);
        }

        // Warn about referenced fields without defaults and not required (deduplicated)
        if let Some(user_config) = &manifest.user_config {
            let mut warned: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for var_name in &all_referenced {
                if warned.contains(var_name.as_str()) {
                    continue;
                }
                if let Some(field) = user_config.get(var_name) {
                    let has_default = field.default.is_some();
                    let is_required = field.required.unwrap_or(false);

                    if !has_default && !is_required {
                        result.warnings.push(ValidationIssue {
                            code: WarningCode::ReferencedFieldNoDefault.into(),
                            message: "referenced field has no default".into(),
                            location: format!("manifest.json:user_config.{}", var_name),
                            details: format!(
                                "`{}` is used in mcp_config but has no default and isn't required",
                                var_name
                            ),
                            help: Some("add a `default` value or set `required: true`".into()),
                        });
                        warned.insert(var_name.as_str());
                    }
                }
            }
        }
    }
}

/// Check for invalid ${user_config.X} references.
/// Returns the list of valid variable names found.
fn check_variable_references(
    s: &str,
    user_config_keys: &[&str],
    field: &str,
    result: &mut ValidationResult,
) -> Vec<String> {
    let mut valid_vars = Vec::new();
    for key in extract_user_config_vars(s) {
        if user_config_keys.contains(&key.as_str()) {
            valid_vars.push(key);
        } else {
            result.errors.push(ValidationIssue {
                code: ErrorCode::InvalidVariableReference.into(),
                message: "invalid variable reference".into(),
                location: format!("manifest.json:server.{}", field),
                details: format!("`${{user_config.{}}}` references undefined key", key),
                help: Some(format!("add `{}` to user_config or fix the reference", key)),
            });
        }
    }
    valid_vars
}
