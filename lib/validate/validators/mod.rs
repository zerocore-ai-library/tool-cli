//! Validation functions for MCPB manifests.

mod core;
mod fields;
mod paths;
mod platforms;
mod recommended;
mod standard;
mod tools;
mod variables;

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use core::validate_manifest;
