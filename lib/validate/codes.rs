//! Validation error and warning codes.

use serde::Serialize;
use std::fmt;

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

    /// W013: Invalid platform key format in platform_overrides.
    #[serde(rename = "W013")]
    InvalidPlatformKey,

    /// W014: tool.store namespace platforms don't cover spec-level platforms.
    #[serde(rename = "W014")]
    PlatformAlignmentMismatch,

    /// W015: Binary path in platform_overrides doesn't exist.
    #[serde(rename = "W015")]
    BinaryOverridePathNotFound,

    /// W016: compatibility.platforms doesn't match platform_overrides keys.
    #[serde(rename = "W016")]
    CompatibilityPlatformMismatch,
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
            WarningCode::InvalidPlatformKey => "W013",
            WarningCode::PlatformAlignmentMismatch => "W014",
            WarningCode::BinaryOverridePathNotFound => "W015",
            WarningCode::CompatibilityPlatformMismatch => "W016",
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
