//! Validation functions for MCPB manifests.

mod core;
mod paths;
mod platforms;
mod recommended;
mod scripts;
mod standard;
mod tools;
mod variables;

pub mod fields;

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use core::validate_manifest;
pub use fields::is_valid_package_name;
