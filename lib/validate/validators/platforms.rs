//! Platform override validation.

use crate::mcpb::{McpbManifest, McpbPlatformOverride, McpbServerType, TOOL_STORE_NAMESPACE};
use std::collections::HashSet;
use std::path::Path;

use super::super::codes::WarningCode;
use super::super::result::{ValidationIssue, ValidationResult};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Valid OS values for platform keys.
const VALID_OS_VALUES: &[&str] = &["darwin", "linux", "win32"];

/// Valid architecture values for platform keys.
const VALID_ARCH_VALUES: &[&str] = &["arm64", "x86_64"];

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate platform override keys in mcp_config and tool.store namespace.
pub fn validate_platform_override_keys(manifest: &McpbManifest, result: &mut ValidationResult) {
    // Validate spec-level platform_overrides
    if let Some(mcp_config) = &manifest.server.mcp_config {
        for key in mcp_config.platform_overrides.keys() {
            validate_platform_key(key, "server.mcp_config.platform_overrides", result);
        }
    }

    // Validate tool.store namespace platform_overrides
    if let Some(overrides) = manifest
        .meta
        .as_ref()
        .and_then(|m| m.get(TOOL_STORE_NAMESPACE))
        .and_then(|r| r.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.as_object())
    {
        for key in overrides.keys() {
            // Check if it deserializes as a valid McpbPlatformOverride
            if let Some(value) = overrides.get(key)
                && serde_json::from_value::<McpbPlatformOverride>(value.clone()).is_ok()
            {
                validate_platform_key(
                    key,
                    &format!(
                        "_meta[\"{}\"].mcp_config.platform_overrides",
                        TOOL_STORE_NAMESPACE
                    ),
                    result,
                );
            }
        }
    }
}

/// Validate a single platform key.
fn validate_platform_key(key: &str, location: &str, result: &mut ValidationResult) {
    // Check for OS-only format (spec level)
    if VALID_OS_VALUES.contains(&key) {
        return;
    }

    // Check for OS-arch format (tool.store extension)
    if let Some((os, arch)) = key.split_once('-') {
        if !VALID_OS_VALUES.contains(&os) {
            result.warnings.push(ValidationIssue {
                code: WarningCode::InvalidPlatformKey.into(),
                message: "invalid platform OS".into(),
                location: format!("manifest.json:{}.{}", location, key),
                details: format!(
                    "`{}` has invalid OS `{}`, expected one of: {}",
                    key,
                    os,
                    VALID_OS_VALUES.join(", ")
                ),
                help: Some("use darwin, linux, or win32".into()),
            });
        }
        if !VALID_ARCH_VALUES.contains(&arch) {
            result.warnings.push(ValidationIssue {
                code: WarningCode::InvalidPlatformKey.into(),
                message: "invalid platform architecture".into(),
                location: format!("manifest.json:{}.{}", location, key),
                details: format!(
                    "`{}` has invalid arch `{}`, expected one of: {}",
                    key,
                    arch,
                    VALID_ARCH_VALUES.join(", ")
                ),
                help: Some("use arm64 or x86_64".into()),
            });
        }
    } else {
        // Neither OS-only nor OS-arch format
        result.warnings.push(ValidationIssue {
            code: WarningCode::InvalidPlatformKey.into(),
            message: "invalid platform key format".into(),
            location: format!("manifest.json:{}.{}", location, key),
            details: format!(
                "`{}` is not a valid platform key, expected OS (darwin/linux/win32) or OS-arch (darwin-arm64)",
                key
            ),
            help: Some("use format: darwin, linux, win32, darwin-arm64, linux-x86_64, etc.".into()),
        });
    }
}

/// Validate that tool.store namespace platforms align with spec-level platforms.
///
/// For each spec-level OS (e.g., "darwin"), there should be at least one
/// corresponding tool.store namespace platform (e.g., "darwin-arm64" or "darwin-x86_64").
pub fn validate_platform_alignment(raw_json: &serde_json::Value, result: &mut ValidationResult) {
    // Get spec-level platform keys
    let spec_platforms: HashSet<String> = raw_json
        .get("server")
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    // Get tool.store namespace platform keys
    let tool_store_platforms: HashSet<String> = raw_json
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|r| r.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    // Skip if either is empty
    if spec_platforms.is_empty() || tool_store_platforms.is_empty() {
        return;
    }

    // Check each spec-level OS has at least one tool.store namespace platform
    for spec_os in &spec_platforms {
        if !VALID_OS_VALUES.contains(&spec_os.as_str()) {
            continue; // Skip invalid keys, they're warned about elsewhere
        }

        let has_coverage = tool_store_platforms
            .iter()
            .any(|p| p.starts_with(&format!("{}-", spec_os)));

        if !has_coverage {
            result.warnings.push(ValidationIssue {
                code: WarningCode::PlatformAlignmentMismatch.into(),
                message: "missing tool.store namespace coverage".into(),
                location: "manifest.json:_meta[\"store.tool.mcpb\"].mcp_config.platform_overrides"
                    .to_string(),
                details: format!(
                    "spec-level platform `{}` has no corresponding tool.store namespace overrides",
                    spec_os
                ),
                help: Some(format!(
                    "add `{}-arm64` and/or `{}-x86_64` to tool.store namespace platform_overrides",
                    spec_os, spec_os
                )),
            });
        }
    }
}

/// Validate that binary paths in platform_overrides exist (for binary server type).
pub fn validate_binary_override_paths(
    dir: &Path,
    manifest: &McpbManifest,
    raw_json: &serde_json::Value,
    result: &mut ValidationResult,
) {
    // Only validate for binary server type
    if manifest.server.server_type != Some(McpbServerType::Binary) {
        return;
    }

    // Check spec-level platform_overrides
    if let Some(overrides) = raw_json
        .get("server")
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.as_object())
    {
        for (platform, override_val) in overrides {
            if let Some(command) = override_val.get("command").and_then(|c| c.as_str()) {
                validate_binary_path(
                    dir,
                    command,
                    &format!(
                        "manifest.json:server.mcp_config.platform_overrides[\"{}\"].command",
                        platform
                    ),
                    result,
                );
            }
        }
    }

    // Check tool.store namespace platform_overrides
    if let Some(overrides) = raw_json
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|r| r.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.as_object())
    {
        for (platform, override_val) in overrides {
            if let Some(command) = override_val.get("command").and_then(|c| c.as_str()) {
                validate_binary_path(
                    dir,
                    command,
                    &format!(
                        "manifest.json:_meta[\"store.tool.mcpb\"].mcp_config.platform_overrides[\"{}\"].command",
                        platform
                    ),
                    result,
                );
            }
        }
    }
}

/// Validate a binary path exists (stripping ${__dirname} prefix).
fn validate_binary_path(dir: &Path, command: &str, location: &str, result: &mut ValidationResult) {
    // Strip ${__dirname}/ or ${__dirname}\ prefix
    let path = command
        .strip_prefix("${__dirname}/")
        .or_else(|| command.strip_prefix("${__dirname}\\"))
        .unwrap_or(command);

    // Skip if it's a system command (no path separators and no ${__dirname})
    if !path.contains('/') && !path.contains('\\') && !command.contains("${__dirname}") {
        return;
    }

    let full_path = dir.join(path);
    if !full_path.exists() {
        result.warnings.push(ValidationIssue {
            code: WarningCode::BinaryOverridePathNotFound.into(),
            message: "binary path not found".into(),
            location: location.to_string(),
            details: format!("binary `{}` does not exist", path),
            help: Some("ensure the binary is built before packing".into()),
        });
    }
}

/// Validate consistency between compatibility.platforms and platform_overrides keys.
pub fn validate_compatibility_platforms(
    raw_json: &serde_json::Value,
    result: &mut ValidationResult,
) {
    // Check spec-level compatibility.platforms vs platform_overrides
    let spec_compat: HashSet<String> = raw_json
        .get("compatibility")
        .and_then(|c| c.get("platforms"))
        .and_then(|p| p.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let spec_overrides: HashSet<String> = raw_json
        .get("server")
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    // Warn if compatibility lists platforms not in overrides
    if !spec_compat.is_empty() && !spec_overrides.is_empty() {
        for platform in &spec_compat {
            if !spec_overrides.contains(platform) {
                result.warnings.push(ValidationIssue {
                    code: WarningCode::CompatibilityPlatformMismatch.into(),
                    message: "compatibility platform missing override".into(),
                    location: "manifest.json:compatibility.platforms".into(),
                    details: format!(
                        "`{}` listed in compatibility.platforms but not in platform_overrides",
                        platform
                    ),
                    help: Some("add a platform_override for this platform or remove from compatibility.platforms".into()),
                });
            }
        }
    }

    // Check tool.store namespace compatibility.platforms vs platform_overrides
    let tool_store_compat: HashSet<String> = raw_json
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|r| r.get("compatibility"))
        .and_then(|c| c.get("platforms"))
        .and_then(|p| p.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let tool_store_overrides: HashSet<String> = raw_json
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|r| r.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.as_object())
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();

    if !tool_store_compat.is_empty() && !tool_store_overrides.is_empty() {
        for platform in &tool_store_compat {
            if !tool_store_overrides.contains(platform) {
                result.warnings.push(ValidationIssue {
                    code: WarningCode::CompatibilityPlatformMismatch.into(),
                    message: "tool.store compatibility platform missing override".into(),
                    location: "manifest.json:_meta[\"store.tool.mcpb\"].compatibility.platforms".into(),
                    details: format!(
                        "`{}` listed in tool.store compatibility.platforms but not in platform_overrides",
                        platform
                    ),
                    help: Some("add a platform_override for this platform or remove from compatibility.platforms".into()),
                });
            }
        }
    }
}
