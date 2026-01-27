//! Recommended field validation.

use crate::mcpb::{McpbManifest, McpbServerType};
use std::path::Path;

use super::super::codes::WarningCode;
use super::super::result::{ValidationIssue, ValidationResult};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate recommended fields for publication.
pub fn validate_recommended_fields(
    dir: &Path,
    manifest: &McpbManifest,
    result: &mut ValidationResult,
) {
    // Check author email
    if manifest
        .author
        .as_ref()
        .map(|a| a.email.is_none())
        .unwrap_or(true)
    {
        result.warnings.push(ValidationIssue {
            code: WarningCode::MissingAuthorEmail.into(),
            message: "missing recommended field".into(),
            location: "manifest.json".into(),
            details: "field `author.email` is recommended for publication".into(),
            help: None,
        });
    }

    // Check license
    if manifest.license.is_none() {
        result.warnings.push(ValidationIssue {
            code: WarningCode::MissingLicense.into(),
            message: "missing recommended field".into(),
            location: "manifest.json".into(),
            details: "field `license` is recommended for publication".into(),
            help: Some("add SPDX identifier like \"MIT\" or \"Apache-2.0\"".into()),
        });
    }

    // Check icon
    if manifest.icon.is_none() && manifest.icons.is_none() {
        result.warnings.push(ValidationIssue {
            code: WarningCode::MissingIcon.into(),
            message: "missing icon".into(),
            location: "manifest.json".into(),
            details: "no icon specified for the bundle".into(),
            help: Some("add `icon` field for better presentation in clients".into()),
        });
    }

    // Check .mcpbignore
    if !dir.join(".mcpbignore").exists() {
        result.warnings.push(ValidationIssue {
            code: WarningCode::MissingMcpbIgnore.into(),
            message: "missing .mcpbignore".into(),
            location: dir.display().to_string(),
            details: "no .mcpbignore found, bundling files that are typically excluded".into(),
            help: Some(
                "create a .mcpbignore file to exclude unnecessary files from the bundle".into(),
            ),
        });
    }

    // Check dependencies bundled (only for bundled tools with server type)
    match manifest.server.server_type {
        Some(McpbServerType::Node) => {
            if !dir.join("node_modules").exists() {
                result.warnings.push(ValidationIssue {
                    code: WarningCode::DependenciesNotBundled.into(),
                    message: "dependencies not bundled".into(),
                    location: dir.display().to_string(),
                    details: "`node_modules/` not found".into(),
                    help: Some("run `npm install --production` before packing".into()),
                });
            }
        }
        Some(McpbServerType::Python) => {
            let has_deps = dir.join("server/lib").exists()
                || dir.join("server/venv").exists()
                || dir.join(".venv").exists();
            if !has_deps {
                result.warnings.push(ValidationIssue {
                    code: WarningCode::DependenciesNotBundled.into(),
                    message: "dependencies not bundled".into(),
                    location: dir.display().to_string(),
                    details: "no Python dependencies found (server/lib/ or venv/)".into(),
                    help: Some("bundle dependencies in server/lib/ or include venv/".into()),
                });
            }
        }
        Some(McpbServerType::Binary) | None => {}
    }
}
