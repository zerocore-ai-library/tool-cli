//! Core validation entry point and helpers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::mcpb::McpbManifest;
use std::path::Path;

use super::super::codes::ErrorCode;
use super::super::result::{ValidationIssue, ValidationResult};
use super::fields::{validate_file_references, validate_formats, validate_required_fields};
use super::platforms::{
    validate_binary_override_paths, validate_compatibility_platforms, validate_platform_alignment,
    validate_platform_override_keys,
};
use super::recommended::validate_recommended_fields;
use super::scripts::validate_script_names;
use super::standard::validate_standard_fields;
use super::tools::validate_tools;
use super::variables::validate_variable_references;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate a manifest directory.
pub fn validate_manifest(dir: &Path) -> ValidationResult {
    let mut result = ValidationResult::default();

    // 1. Check directory exists
    if !dir.exists() {
        result.errors.push(ValidationIssue {
            code: ErrorCode::ManifestNotFound.into(),
            message: "directory not found".into(),
            location: dir.display().to_string(),
            details: "directory does not exist".into(),
            help: None,
        });
        return result;
    }

    // 2. Check manifest.json exists
    let manifest_path = dir.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        result.errors.push(ValidationIssue {
            code: ErrorCode::ManifestNotFound.into(),
            message: "manifest not found".into(),
            location: dir.display().to_string(),
            details: "manifest.json does not exist".into(),
            help: Some("run `tool init` to create one".into()),
        });
        return result;
    }

    // 3. Read file
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(e) => {
            result.errors.push(ValidationIssue {
                code: ErrorCode::InvalidJson.into(),
                message: "cannot read manifest".into(),
                location: "manifest.json".into(),
                details: format!("failed to read file: {}", e),
                help: None,
            });
            return result;
        }
    };

    // 4. Parse JSON (both as typed and raw for field validation)
    let manifest: McpbManifest = match serde_json::from_str(&content) {
        Ok(m) => m,
        Err(e) => {
            result.errors.push(ValidationIssue {
                code: ErrorCode::InvalidJson.into(),
                message: "invalid JSON".into(),
                location: "manifest.json".into(),
                details: format!("parse error: {}", e),
                help: Some("check JSON syntax".into()),
            });
            return result;
        }
    };

    // Parse as raw JSON for extra field detection
    let raw_json: serde_json::Value = serde_json::from_str(&content).unwrap();

    // 5. Validate required fields
    validate_required_fields(&manifest, &mut result);

    // 6. Validate field formats
    validate_formats(&manifest, &mut result);

    // 7. Validate file references
    validate_file_references(dir, &manifest, &mut result);

    // 8. Validate variable references
    validate_variable_references(&manifest, &mut result);

    // 9. Check for recommended fields (warnings)
    validate_recommended_fields(dir, &manifest, &mut result);

    // 10. Validate tools declarations (with raw JSON for extra field detection)
    validate_tools(&manifest, &raw_json, &mut result);

    // 11. Validate all standard-defined fields for extra fields
    validate_standard_fields(&raw_json, &mut result);

    // 12. Validate platform override keys
    validate_platform_override_keys(&manifest, &mut result);

    // 13. Validate platform override alignment (tool.store namespace covers spec-level)
    validate_platform_alignment(&raw_json, &mut result);

    // 14. Validate binary paths in platform_overrides exist
    validate_binary_override_paths(dir, &manifest, &raw_json, &mut result);

    // 15. Validate compatibility.platforms matches platform_overrides
    validate_compatibility_platforms(&raw_json, &mut result);

    // 16. Validate script names don't conflict with built-in subcommands
    validate_script_names(&raw_json, &mut result);

    result
}

/// Helper to add a missing required field error.
pub fn missing_field(result: &mut ValidationResult, location: &str, field: &str) {
    result.errors.push(ValidationIssue {
        code: ErrorCode::MissingRequiredField.into(),
        message: "missing required field".into(),
        location: location.into(),
        details: format!("field `{}` is required", field),
        help: None,
    });
}
