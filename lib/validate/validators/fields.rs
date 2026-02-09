//! Required fields and format validation.

use crate::mcpb::{McpbManifest, McpbServerType};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use super::super::codes::{ErrorCode, WarningCode};
use super::super::result::{ValidationIssue, ValidationResult};
use super::core::missing_field;
use super::paths::{is_path_safe, validate_file_path};

/// Regex for valid icon size format: WIDTHxHEIGHT (e.g., "32x32", "128x128")
static SIZE_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d+x\d+$").unwrap());

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Minimum package name length
pub const MIN_PACKAGE_NAME_LENGTH: usize = 3;

/// Maximum package name length
pub const MAX_PACKAGE_NAME_LENGTH: usize = 64;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Validate required fields are present.
pub fn validate_required_fields(manifest: &McpbManifest, result: &mut ValidationResult) {
    if manifest.name.is_none() {
        missing_field(result, "manifest.json", "name");
    }

    if manifest.version.is_none() {
        missing_field(result, "manifest.json", "version");
    }

    if manifest.description.is_none() {
        missing_field(result, "manifest.json", "description");
    }

    match &manifest.author {
        None => missing_field(result, "manifest.json", "author"),
        Some(author) if author.name.is_empty() => {
            missing_field(result, "manifest.json:author", "author.name");
        }
        _ => {}
    }

    // Server required fields (required for all server types per MCPB spec)
    // Reference mode = no entry_point AND no type (HTTP reference to external server)
    let is_reference_mode =
        manifest.server.entry_point.is_none() && manifest.server.server_type.is_none();

    if manifest.server.entry_point.is_none() && !is_reference_mode {
        result.errors.push(ValidationIssue {
            code: ErrorCode::MissingEntryPoint.into(),
            message: "missing entry point".into(),
            location: "manifest.json:server".into(),
            details: "`entry_point` is required".into(),
            help: Some("add `entry_point` field to server config".into()),
        });
    }

    if manifest.server.mcp_config.is_none() {
        result.errors.push(ValidationIssue {
            code: ErrorCode::MissingMcpConfig.into(),
            message: "missing mcp_config".into(),
            location: "manifest.json:server".into(),
            details: "`mcp_config` is required".into(),
            help: Some("add `mcp_config` with command, args, and env".into()),
        });
    }
}

/// Validate field value formats.
pub fn validate_formats(manifest: &McpbManifest, result: &mut ValidationResult) {
    // Check manifest_version
    if manifest.manifest_version != "0.3" {
        result.warnings.push(ValidationIssue {
            code: WarningCode::DeprecatedManifestVersion.into(),
            message: "deprecated manifest version".into(),
            location: "manifest.json:manifest_version".into(),
            details: format!("`{}` is not the current version", manifest.manifest_version),
            help: Some("update to \"0.3\"".into()),
        });
    }

    // Validate name format
    if let Some(name) = &manifest.name
        && !is_valid_package_name(name)
    {
        result.errors.push(ValidationIssue {
            code: ErrorCode::InvalidPackageName.into(),
            message: "invalid package name".into(),
            location: "manifest.json:name".into(),
            details: format!("`{}` must be 3-64 lowercase alphanumeric chars with hyphens, starting with a letter", name),
            help: Some("use format: my-package-name (3-64 chars)".into()),
        });
    }

    // Validate semver
    if let Some(version) = &manifest.version
        && semver::Version::parse(version).is_err()
    {
        result.errors.push(ValidationIssue {
            code: ErrorCode::InvalidVersion.into(),
            message: "invalid version".into(),
            location: "manifest.json:version".into(),
            details: format!("`{}` is not valid semver", version),
            help: Some("use format: MAJOR.MINOR.PATCH (e.g., 1.0.0)".into()),
        });
    }
}

/// Validate file references exist.
pub fn validate_file_references(
    dir: &Path,
    manifest: &McpbManifest,
    result: &mut ValidationResult,
) {
    // Check entry point exists and doesn't escape package
    if let Some(entry_point) = &manifest.server.entry_point {
        validate_file_path(
            dir,
            entry_point,
            "server.entry_point",
            "manifest.json",
            result,
        );

        // Check extension matches server type (only if path is safe)
        if is_path_safe(dir, entry_point) {
            let expected_ext = match manifest.server.server_type {
                Some(McpbServerType::Node) => Some("js"),
                Some(McpbServerType::Python) => Some("py"),
                Some(McpbServerType::Binary) | None => None,
            };

            if let Some(ext) = expected_ext
                && !entry_point.ends_with(&format!(".{}", ext))
                && let Some(ref server_type) = manifest.server.server_type
            {
                result.warnings.push(ValidationIssue {
                    code: WarningCode::EntryPointExtensionMismatch.into(),
                    message: "entry point extension mismatch".into(),
                    location: "manifest.json:server.entry_point".into(),
                    details: format!(
                        "`{}` doesn't have .{} extension for {} type",
                        entry_point, ext, server_type
                    ),
                    help: None,
                });
            }
        }
    }

    // Check icon exists and doesn't escape package
    if let Some(icon) = &manifest.icon {
        validate_file_path(dir, icon, "icon", "manifest.json", result);
    }

    // Check icons array
    if let Some(icons) = &manifest.icons {
        for (i, icon_entry) in icons.iter().enumerate() {
            validate_file_path(
                dir,
                &icon_entry.src,
                &format!("icons[{}].src", i),
                "manifest.json",
                result,
            );
        }
    }
}

/// Check if a package name is valid.
/// Rules: 3-64 chars, starts with lowercase letter, contains only lowercase letters, digits, hyphens
pub fn is_valid_package_name(name: &str) -> bool {
    let len = name.len();
    if !(MIN_PACKAGE_NAME_LENGTH..=MAX_PACKAGE_NAME_LENGTH).contains(&len) {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Validate icon fields according to MCPB spec.
///
/// Validates:
/// - `icons[].src` is required and non-empty (Error)
/// - `icons[].size` format is WIDTHxHEIGHT if present (Error)
/// - Icon files should be PNG format (Warning)
pub fn validate_icons(manifest: &McpbManifest, result: &mut ValidationResult) {
    // Validate legacy `icon` field format
    if let Some(icon) = &manifest.icon {
        validate_icon_format(icon, "icon", result);
    }

    // Validate `icons` array
    if let Some(icons) = &manifest.icons {
        for (i, icon_entry) in icons.iter().enumerate() {
            let field_prefix = format!("icons[{}]", i);

            // Error: src is required and cannot be empty
            if icon_entry.src.is_empty() {
                result.errors.push(ValidationIssue {
                    code: ErrorCode::MissingIconSrc.into(),
                    message: "icon src is required".into(),
                    location: format!("manifest.json:{}.src", field_prefix),
                    details: "`src` field is required and cannot be empty".into(),
                    help: Some("add a path to the icon file (e.g., \"icon.png\")".into()),
                });
            } else {
                // Validate format if src is present
                validate_icon_format(&icon_entry.src, &format!("{}.src", field_prefix), result);
            }

            // Error: size must match WIDTHxHEIGHT format if present
            if let Some(size) = &icon_entry.size
                && !SIZE_PATTERN.is_match(size)
            {
                result.errors.push(ValidationIssue {
                    code: ErrorCode::InvalidIconSize.into(),
                    message: "invalid icon size format".into(),
                    location: format!("manifest.json:{}.size", field_prefix),
                    details: format!(
                        "`{}` is not valid. Must be WIDTHxHEIGHT (e.g., \"32x32\")",
                        size
                    ),
                    help: Some("use format: WIDTHxHEIGHT (e.g., \"16x16\", \"128x128\")".into()),
                });
            }
        }
    }
}

/// Validate icon file format (warn if not PNG).
fn validate_icon_format(src: &str, field: &str, result: &mut ValidationResult) {
    // Skip URL validation for https:// icons
    if src.starts_with("https://") {
        if !src.to_lowercase().ends_with(".png") {
            result.warnings.push(ValidationIssue {
                code: WarningCode::NonPngIcon.into(),
                message: "icon should be PNG format".into(),
                location: format!("manifest.json:{}", field),
                details: format!("`{}` is not a .png file", src),
                help: Some("MCPB spec recommends PNG format for icons".into()),
            });
        }
        return;
    }

    // Check file extension for local paths
    let extension = Path::new(src)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match extension {
        Some(ext) if ext == "png" => {
            // Valid PNG extension
        }
        Some(ext) => {
            // Warning: non-PNG format
            result.warnings.push(ValidationIssue {
                code: WarningCode::NonPngIcon.into(),
                message: "icon should be PNG format".into(),
                location: format!("manifest.json:{}", field),
                details: format!("`{}` uses .{} format", src, ext),
                help: Some("MCPB spec recommends PNG format for icons".into()),
            });
        }
        None => {
            // Warning: no extension
            result.warnings.push(ValidationIssue {
                code: WarningCode::NonPngIcon.into(),
                message: "icon should be PNG format".into(),
                location: format!("manifest.json:{}", field),
                details: format!("`{}` has no file extension", src),
                help: Some("MCPB spec recommends PNG format for icons".into()),
            });
        }
    }
}
