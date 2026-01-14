//! Path validation utilities.

use std::path::Path;

use super::super::codes::ErrorCode;
use super::super::result::{ValidationIssue, ValidationResult};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Check if a path escapes the base directory (path traversal).
/// Returns true if the path is safe (stays within base_dir).
pub fn is_path_safe(base_dir: &Path, relative_path: &str) -> bool {
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
pub fn validate_file_path(
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
