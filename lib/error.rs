//! Error types for tool-cli.

use thiserror::Error;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Result type for tool-cli operations.
pub type ToolResult<T> = Result<T, ToolError>;

/// Error type for tool-cli operations.
#[derive(Debug, Error)]
pub enum ToolError {
    /// Invalid tool configuration.
    #[error("Invalid tool configuration: {0}")]
    InvalidToolConfig(String),

    /// Invalid reference format.
    #[error("Invalid reference: {0}")]
    InvalidReference(String),

    /// Ambiguous plugin reference with multiple matches.
    #[error(
        "Ambiguous tool reference '{requested}'. Found multiple versions:\n{candidates}\n\nPlease specify the full reference."
    )]
    AmbiguousReference {
        requested: String,
        candidates: String,
        suggestion: String,
    },

    /// Tool not found.
    #[error("{kind} not found: {reference}")]
    NotFound { kind: String, reference: String },

    /// Invalid spec format or content.
    #[error("Invalid spec: {0}")]
    InvalidSpec(String),

    /// Configuration parse error.
    #[error("Failed to parse configuration: {0}")]
    ConfigParseError(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    SerializationError(#[from] toml::ser::Error),

    /// Deserialization error.
    #[error("Deserialization error: {0}")]
    DeserializationError(#[from] toml::de::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Generic error.
    #[error("{0}")]
    Generic(String),

    /// OAuth authentication required.
    #[error("OAuth authentication required for tool: {tool_ref}")]
    AuthRequired { tool_ref: String },

    /// OAuth not configured (credential storage unavailable).
    #[error("OAuth not configured")]
    OAuthNotConfigured,

    /// Entry point file not found.
    #[error("Entry point not found: {entry_point}")]
    EntryPointNotFound {
        /// The entry point path from manifest.
        entry_point: String,
        /// Full path where the file should exist.
        full_path: String,
        /// Build script if defined in manifest.
        build_script: Option<String>,
        /// Bundle directory path.
        bundle_path: String,
    },

    /// Validation failed.
    #[error("Validation failed")]
    ValidationFailed(crate::validate::ValidationResult),

    /// Pack error.
    #[error("Pack error: {0}")]
    PackError(String),

    /// Zip error.
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// Walkdir error.
    #[error("Walkdir error: {0}")]
    WalkDir(#[from] walkdir::Error),

    /// Path strip error.
    #[error("Path error: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    /// Ignore pattern error.
    #[error("Ignore pattern error: {0}")]
    Ignore(#[from] ignore::Error),

    /// Manifest not found.
    #[error("manifest.json not found in {0}")]
    ManifestNotFound(std::path::PathBuf),

    /// Regex error.
    #[error("Regex error: {0}")]
    Regex(#[from] regex::Error),

    /// User cancelled operation.
    #[error("Operation cancelled")]
    Cancelled,

    /// Registry API error with structured response.
    #[error("{operation} failed")]
    RegistryApi {
        /// The operation that failed (e.g., "Upload", "Publish").
        operation: String,
        /// API error code (e.g., "CONFLICT", "BAD_REQUEST").
        code: String,
        /// Human-readable error message.
        message: String,
        /// HTTP status code.
        status: u16,
    },
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl From<anyhow::Error> for ToolError {
    fn from(err: anyhow::Error) -> Self {
        ToolError::Generic(err.to_string())
    }
}
