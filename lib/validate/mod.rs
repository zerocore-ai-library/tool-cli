//! Tool manifest validation for MCPB format.

mod codes;
mod result;
mod validators;

#[cfg(test)]
mod tests;

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use codes::{ErrorCode, ValidationCode, WarningCode};
pub use result::{ValidationIssue, ValidationResult};
pub use validators::validate_manifest;
