//! Validation result types.

use serde::Serialize;

use super::codes::ValidationCode;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

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
