//! Script name validation.

use super::super::codes::WarningCode;
use super::super::result::{ValidationIssue, ValidationResult};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Built-in tool-cli subcommands that script names cannot use.
const RESERVED_SUBCOMMANDS: &[&str] = &[
    "init",
    "detect",
    "search",
    "install",
    "uninstall",
    "list",
    "grep",
    "info",
    "call",
    "download",
    "validate",
    "pack",
    "run",
    "publish",
    "login",
    "logout",
    "whoami",
    "self",
    "config",
    "host",
];

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate that script names don't conflict with built-in subcommands.
pub fn validate_script_names(raw_json: &serde_json::Value, result: &mut ValidationResult) {
    let scripts = raw_json
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|r| r.get("scripts"))
        .and_then(|s| s.as_object());

    if let Some(scripts) = scripts {
        for script_name in scripts.keys() {
            if RESERVED_SUBCOMMANDS.contains(&script_name.as_str()) {
                result.warnings.push(ValidationIssue {
                    code: WarningCode::ReservedScriptName.into(),
                    message: "reserved script name".into(),
                    location: format!("_meta.store.tool.mcpb.scripts.{}", script_name),
                    details: format!(
                        "script `{}` is shadowed by built-in `tool {}` and will never run",
                        script_name, script_name
                    ),
                    help: Some("rename the script to avoid the conflict".into()),
                });
            }
        }
    }
}
