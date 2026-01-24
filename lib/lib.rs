//! `tool-cli` library.

pub mod commands;
pub mod concise;
pub mod constants;
pub mod detect;
pub mod error;
pub mod format;
pub mod handlers;
pub mod hosts;
pub mod mcp;
pub mod mcpb;
pub mod oauth;
pub mod output;
pub mod pack;
pub mod prompt;
pub mod proxy;
pub mod references;
pub mod registry;
pub mod resolver;
pub mod scaffold;
pub mod security;
pub mod self_update;
pub mod styles;
pub mod system_config;
pub mod tree;
pub mod validate;
pub mod vars;

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use commands::*;
pub use concise::*;
pub use constants::*;
pub use detect::*;
pub use error::*;
pub use handlers::*;
pub use hosts::*;
pub use mcp::*;
pub use mcpb::*;
pub use output::*;
pub use pack::*;
pub use references::*;
pub use registry::*;
pub use resolver::*;
pub use scaffold::*;
pub use self_update::*;
pub use system_config::*;
pub use validate::*;
pub use vars::*;
