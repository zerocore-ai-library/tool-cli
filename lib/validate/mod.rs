//! Tool manifest validation for MCPB format.

mod codes;
mod result;

pub mod validators;

#[cfg(test)]
mod tests;

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use codes::{ErrorCode, ValidationCode, WarningCode};
pub use result::{ValidationIssue, ValidationResult};
pub use validators::{is_valid_package_name, validate_manifest};
