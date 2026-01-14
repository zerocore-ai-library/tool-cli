//! Tool command handlers.

mod call;
mod detect_cmd;
mod grep;
mod info;
mod init;
mod list;
mod pack_cmd;
mod registry;
mod scripts;
mod validate_cmd;

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use call::tool_call;
pub use detect_cmd::detect_mcpb;
pub use grep::grep_tool;
pub use info::tool_info;
pub use init::init_mcpb;
pub use list::{list_tools, resolve_tool_path};
pub use pack_cmd::pack_mcpb;
pub use registry::{add_tool, download_tool, publish_mcpb, remove_tool, search_tools};
pub use scripts::{list_scripts, run_external_script, run_script};
pub use validate_cmd::validate_mcpb;
