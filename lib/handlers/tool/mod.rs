//! Tool command handlers.

mod call;
mod common;
mod config_cmd;
mod detect_cmd;
mod grep;
mod host_cmd;
mod info;
mod init;
mod install;
mod list;
mod pack_cmd;
mod preview;
mod publish;
mod run;
mod scripts;
mod search;
mod uninstall;
mod validate_cmd;

//--------------------------------------------------------------------------------------------------
// Re-Exports
//--------------------------------------------------------------------------------------------------

pub use call::tool_call;
pub use common::{PrepareToolOptions, PreparedTool, prepare_tool};
pub use config_cmd::{config_tool, load_tool_config};
pub use detect_cmd::detect_mcpb;
pub use grep::grep_tool;
pub use host_cmd::handle_host_command;
pub use info::tool_info;
pub use init::init_mcpb;
pub use install::{LinkResult, add_tools, download_tools, link_local_tool, link_local_tool_force};
pub use list::{ResolvedToolPath, list_tools, resolve_tool_path};
pub use pack_cmd::pack_mcpb;
pub use preview::tool_preview;
pub use publish::publish_mcpb;
pub use run::tool_run;
pub use scripts::{list_scripts, run_external_script, run_script};
pub use search::search_tools;
pub use uninstall::remove_tools;
pub use validate_cmd::validate_mcpb;
