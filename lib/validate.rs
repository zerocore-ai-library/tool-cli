//! Tool manifest validation for MCPB format.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::mcpb::{McpbManifest, McpbServerType};
use crate::vars::extract_user_config_vars;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt;
use std::path::Path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Validation error codes.
///
/// These represent errors that always cause validation to fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorCode {
    /// E000: manifest.json file not found.
    #[serde(rename = "E000")]
    ManifestNotFound,

    /// E001: Invalid JSON syntax or cannot read file.
    #[serde(rename = "E001")]
    InvalidJson,

    /// E002: A required field is missing.
    #[serde(rename = "E002")]
    MissingRequiredField,

    /// E003: Package name doesn't match required format.
    #[serde(rename = "E003")]
    InvalidPackageName,

    /// E004: Version string is not valid semver.
    #[serde(rename = "E004")]
    InvalidVersion,

    /// E005: Server type is not one of: node, python, binary.
    #[serde(rename = "E005")]
    InvalidServerType,

    /// E006: entry_point is required for all server types.
    #[serde(rename = "E006")]
    MissingEntryPoint,

    /// E007: The referenced entry point file does not exist.
    #[serde(rename = "E007")]
    EntryPointNotFound,

    /// E008: mcp_config is required for all server types.
    #[serde(rename = "E008")]
    MissingMcpConfig,

    /// E009: A ${user_config.X} variable references an undefined key.
    #[serde(rename = "E009")]
    InvalidVariableReference,

    /// E010: Missing command field for stdio transport.
    #[serde(rename = "E010")]
    MissingCommand,

    /// E011: Missing url field for http transport.
    #[serde(rename = "E011")]
    MissingUrl,

    /// E012: Invalid URL format.
    #[serde(rename = "E012")]
    InvalidUrl,

    /// E013: Path escapes package directory (path traversal).
    #[serde(rename = "E013")]
    PathTraversal,

    /// E014: Referenced file does not exist.
    #[serde(rename = "E014")]
    FileNotFound,

    /// E015: Tool declaration missing required name field.
    #[serde(rename = "E015")]
    ToolMissingName,

    /// E016: Tool declaration missing required description field.
    #[serde(rename = "E016")]
    ToolMissingDescription,

    /// E017: Duplicate tool name in tools array.
    #[serde(rename = "E017")]
    DuplicateToolName,

    /// E018: Invalid inputSchema - must be a JSON Schema object.
    #[serde(rename = "E018")]
    InvalidInputSchema,

    /// E019: Standard-defined field has extra fields not in MCPB spec.
    #[serde(rename = "E019")]
    ExtraFieldsInStandardField,
}

/// Validation warning codes.
///
/// These represent issues that don't fail validation but indicate
/// potential problems or missing recommended fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WarningCode {
    /// W001: author.email is recommended for publication.
    #[serde(rename = "W001")]
    MissingAuthorEmail,

    /// W002: license field is recommended for publication.
    #[serde(rename = "W002")]
    MissingLicense,

    /// W003: No icon specified for the bundle.
    #[serde(rename = "W003")]
    MissingIcon,

    /// W004: Dependencies (node_modules/ or venv/) not found.
    #[serde(rename = "W004")]
    DependenciesNotBundled,

    /// W005: Entry point file extension doesn't match server type.
    #[serde(rename = "W005")]
    EntryPointExtensionMismatch,

    /// W007: Using a deprecated manifest_version (current: 0.3).
    #[serde(rename = "W007")]
    DeprecatedManifestVersion,

    /// W008: Missing description field.
    #[serde(rename = "W008")]
    MissingDescription,

    /// W009: Missing authors field.
    #[serde(rename = "W009")]
    MissingAuthors,

    /// W010: A referenced user_config field has no default and isn't required.
    #[serde(rename = "W010")]
    ReferencedFieldNoDefault,

    /// W011: Tool in static_responses not declared in top-level tools.
    #[serde(rename = "W011")]
    StaticToolNotInTopLevel,

    /// W012: Top-level tool missing from static_responses.
    #[serde(rename = "W012")]
    TopLevelToolMissingSchema,
}

/// A validation code that can be either an error or warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ValidationCode {
    /// An error code.
    Error(ErrorCode),
    /// A warning code.
    Warning(WarningCode),
}

/// Validation result with categorized issues.
#[derive(Debug, Default, Serialize)]
pub struct ValidationResult {
    /// Validation errors (always fail).
    pub errors: Vec<ValidationIssue>,
    /// Validation warnings (fail with --strict).
    pub warnings: Vec<ValidationIssue>,
}

/// A validation issue (error or warning).
#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    /// Error/warning code.
    pub code: ValidationCode,

    /// Short description (e.g., "missing required field").
    pub message: String,

    /// Location in manifest (e.g., "manifest.json", "manifest.json:server.entry_point").
    pub location: String,

    /// Detailed explanation.
    pub details: String,

    /// Optional help suggestion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            ErrorCode::ManifestNotFound => "E000",
            ErrorCode::InvalidJson => "E001",
            ErrorCode::MissingRequiredField => "E002",
            ErrorCode::InvalidPackageName => "E003",
            ErrorCode::InvalidVersion => "E004",
            ErrorCode::InvalidServerType => "E005",
            ErrorCode::MissingEntryPoint => "E006",
            ErrorCode::EntryPointNotFound => "E007",
            ErrorCode::MissingMcpConfig => "E008",
            ErrorCode::InvalidVariableReference => "E009",
            ErrorCode::MissingCommand => "E010",
            ErrorCode::MissingUrl => "E011",
            ErrorCode::InvalidUrl => "E012",
            ErrorCode::PathTraversal => "E013",
            ErrorCode::FileNotFound => "E014",
            ErrorCode::ToolMissingName => "E015",
            ErrorCode::ToolMissingDescription => "E016",
            ErrorCode::DuplicateToolName => "E017",
            ErrorCode::InvalidInputSchema => "E018",
            ErrorCode::ExtraFieldsInStandardField => "E019",
        };
        write!(f, "{}", code)
    }
}

impl fmt::Display for WarningCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            WarningCode::MissingAuthorEmail => "W001",
            WarningCode::MissingLicense => "W002",
            WarningCode::MissingIcon => "W003",
            WarningCode::DependenciesNotBundled => "W004",
            WarningCode::EntryPointExtensionMismatch => "W005",
            WarningCode::DeprecatedManifestVersion => "W007",
            WarningCode::MissingDescription => "W008",
            WarningCode::MissingAuthors => "W009",
            WarningCode::ReferencedFieldNoDefault => "W010",
            WarningCode::StaticToolNotInTopLevel => "W011",
            WarningCode::TopLevelToolMissingSchema => "W012",
        };
        write!(f, "{}", code)
    }
}

impl fmt::Display for ValidationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationCode::Error(e) => write!(f, "{}", e),
            ValidationCode::Warning(w) => write!(f, "{}", w),
        }
    }
}

impl From<ErrorCode> for ValidationCode {
    fn from(code: ErrorCode) -> Self {
        ValidationCode::Error(code)
    }
}

impl From<WarningCode> for ValidationCode {
    fn from(code: WarningCode) -> Self {
        ValidationCode::Warning(code)
    }
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl ValidationResult {
    /// Returns true if there are no errors.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns true if there are no errors or warnings.
    pub fn is_strict_valid(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty()
    }
}

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

    result
}

/// Helper to add a missing required field error.
fn missing_field(result: &mut ValidationResult, location: &str, field: &str) {
    result.errors.push(ValidationIssue {
        code: ErrorCode::MissingRequiredField.into(),
        message: "missing required field".into(),
        location: location.into(),
        details: format!("field `{}` is required", field),
        help: None,
    });
}

/// Validate required fields are present.
fn validate_required_fields(manifest: &McpbManifest, result: &mut ValidationResult) {
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
    if manifest.server.entry_point.is_none() {
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
fn validate_formats(manifest: &McpbManifest, result: &mut ValidationResult) {
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
            details: format!("`{}` must be lowercase alphanumeric with hyphens", name),
            help: Some("use format: my-package-name".into()),
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
fn validate_file_references(dir: &Path, manifest: &McpbManifest, result: &mut ValidationResult) {
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
                &icon_entry.path,
                &format!("icons[{}].path", i),
                "manifest.json",
                result,
            );
        }
    }
}

/// Validate variable references in mcp_config.
fn validate_variable_references(manifest: &McpbManifest, result: &mut ValidationResult) {
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

/// Validate recommended fields for publication.
fn validate_recommended_fields(dir: &Path, manifest: &McpbManifest, result: &mut ValidationResult) {
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

/// Check if a package name is valid.
fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Check if a path escapes the base directory (path traversal).
/// Returns true if the path is safe (stays within base_dir).
fn is_path_safe(base_dir: &Path, relative_path: &str) -> bool {
    // Reject absolute paths
    if relative_path.starts_with('/') || relative_path.starts_with('\\') {
        return false;
    }

    // Check for path traversal patterns
    if relative_path.contains("..") {
        // Resolve the path and check if it's still within base_dir
        let full_path = base_dir.join(relative_path);
        if let (Ok(canonical), Ok(base_canonical)) =
            (full_path.canonicalize(), base_dir.canonicalize())
        {
            return canonical.starts_with(&base_canonical);
        }
        // If we can't canonicalize (file doesn't exist), check path components
        for component in std::path::Path::new(relative_path).components() {
            if matches!(component, std::path::Component::ParentDir) {
                return false;
            }
        }
    }

    true
}

/// Validate a file reference path (checks traversal and existence).
fn validate_file_path(
    dir: &Path,
    path: &str,
    field: &str,
    manifest_file: &str,
    result: &mut ValidationResult,
) {
    // Check for path traversal
    if !is_path_safe(dir, path) {
        result.errors.push(ValidationIssue {
            code: ErrorCode::PathTraversal.into(),
            message: "path escapes package directory".into(),
            location: format!("{}:{}", manifest_file, field),
            details: format!("`{}` references a path outside the package", path),
            help: Some("use a relative path within the package directory".into()),
        });
        return;
    }

    // Check if file exists
    let full_path = dir.join(path);
    if !full_path.exists() {
        result.errors.push(ValidationIssue {
            code: ErrorCode::FileNotFound.into(),
            message: format!(
                "{} not found",
                field.split('.').next_back().unwrap_or(field)
            ),
            location: format!("{}:{}", manifest_file, field),
            details: format!("file `{}` does not exist", path),
            help: Some(format!(
                "add the file or remove the {} field",
                field.split('.').next_back().unwrap_or(field)
            )),
        });
    }
}

/// Allowed fields in top-level tools per MCPB spec.
const ALLOWED_TOOL_FIELDS: &[&str] = &["name", "description"];

/// Allowed fields in top-level prompts per MCPB spec.
const ALLOWED_PROMPT_FIELDS: &[&str] = &["name", "description", "arguments", "text"];

/// Allowed fields in author per MCPB spec.
const ALLOWED_AUTHOR_FIELDS: &[&str] = &["name", "email", "url"];

/// Allowed fields in repository per MCPB spec.
const ALLOWED_REPOSITORY_FIELDS: &[&str] = &["type", "url"];

/// Allowed fields in server per MCPB spec + Radical extensions.
const ALLOWED_SERVER_FIELDS: &[&str] = &[
    // MCPB standard
    "type",
    "entry_point",
    "mcp_config",
    // Radical extension
    "transport",
];

/// Allowed fields in mcp_config per MCPB spec + Radical extensions.
const ALLOWED_MCP_CONFIG_FIELDS: &[&str] = &[
    // MCPB standard
    "command",
    "args",
    "env",
    "platform_overrides",
    // Radical extensions
    "url",
    "headers",
    "oauth_config",
];

/// Allowed fields in compatibility per MCPB spec.
const ALLOWED_COMPATIBILITY_FIELDS: &[&str] = &["claude_desktop", "platforms", "runtimes"];

/// Allowed fields in icons array items per MCPB spec.
const ALLOWED_ICON_FIELDS: &[&str] = &["src", "size"];

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

/// Helper to check for extra fields in a JSON object.
fn check_extra_fields(
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
fn validate_standard_fields(raw_json: &serde_json::Value, result: &mut ValidationResult) {
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

/// Validate tool and prompt declarations (top-level and static_responses).
fn validate_tools(
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
                "manifest.json:_meta[\"company.superrad.mcpb\"][\"static_responses\"][\"tools/list\"].tools[{}]",
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
                "manifest.json:_meta[\"company.superrad.mcpb\"][\"static_responses\"][\"tools/list\"].tools[name=\"{}\"].{}",
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
                "manifest.json:_meta[\"company.superrad.mcpb\"][\"static_responses\"][\"tools/list\"].tools[name=\"{}\"].{}.properties",
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
                "manifest.json:_meta[\"company.superrad.mcpb\"][\"static_responses\"][\"tools/list\"].tools[name=\"{}\"].{}.required",
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

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_valid_package_name() {
        assert!(is_valid_package_name("my-tool"));
        assert!(is_valid_package_name("tool123"));
        assert!(is_valid_package_name("a"));
        assert!(!is_valid_package_name(""));
        assert!(!is_valid_package_name("My-Tool"));
        assert!(!is_valid_package_name("123tool"));
        assert!(!is_valid_package_name("-tool"));
        assert!(!is_valid_package_name("tool_name"));
    }

    #[test]
    fn test_missing_manifest() {
        let dir = TempDir::new().unwrap();
        let result = validate_manifest(dir.path());
        assert!(!result.is_valid());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(
            result.errors[0].code,
            ValidationCode::Error(ErrorCode::ManifestNotFound)
        );
    }

    #[test]
    fn test_invalid_json() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("manifest.json"), "{ invalid json }").unwrap();
        let result = validate_manifest(dir.path());
        assert!(!result.is_valid());
        assert_eq!(
            result.errors[0].code,
            ValidationCode::Error(ErrorCode::InvalidJson)
        );
    }

    #[test]
    fn test_missing_required_fields() {
        let dir = TempDir::new().unwrap();
        let manifest = r#"{
            "manifest_version": "0.3",
            "server": { "type": "node" }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        let result = validate_manifest(dir.path());
        assert!(!result.is_valid());
        // Should have errors for: name, version, description, author, entry_point, mcp_config
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == ValidationCode::Error(ErrorCode::MissingRequiredField))
        );
    }

    #[test]
    fn test_invalid_version() {
        let dir = TempDir::new().unwrap();
        let manifest = r#"{
            "manifest_version": "0.3",
            "name": "my-tool",
            "version": "not-semver",
            "description": "A tool",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "server/index.js",
                "mcp_config": { "command": "node", "args": [] }
            }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        let result = validate_manifest(dir.path());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == ValidationCode::Error(ErrorCode::InvalidVersion))
        );
    }

    #[test]
    fn test_invalid_name() {
        let dir = TempDir::new().unwrap();
        let manifest = r#"{
            "manifest_version": "0.3",
            "name": "MyTool",
            "version": "1.0.0",
            "description": "A tool",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "server/index.js",
                "mcp_config": { "command": "node", "args": [] }
            }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        let result = validate_manifest(dir.path());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == ValidationCode::Error(ErrorCode::InvalidPackageName))
        );
    }

    #[test]
    fn test_missing_entry_point_file() {
        let dir = TempDir::new().unwrap();
        let manifest = r#"{
            "manifest_version": "0.3",
            "name": "my-tool",
            "version": "1.0.0",
            "description": "A tool",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "server/index.js",
                "mcp_config": { "command": "node", "args": [] }
            }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        let result = validate_manifest(dir.path());
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == ValidationCode::Error(ErrorCode::FileNotFound))
        );
    }

    #[test]
    fn test_valid_manifest_with_warnings() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("server")).unwrap();
        std::fs::write(dir.path().join("server/index.js"), "// entry").unwrap();

        let manifest = r#"{
            "manifest_version": "0.3",
            "name": "my-tool",
            "version": "1.0.0",
            "description": "A tool",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "server/index.js",
                "mcp_config": { "command": "node", "args": [] }
            }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        let result = validate_manifest(dir.path());

        // Should be valid (no errors)
        assert!(result.is_valid());
        // But should have warnings for missing: author.email, license, icon, node_modules
        assert!(!result.warnings.is_empty());
    }
}
