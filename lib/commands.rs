//! CLI command definitions.

use crate::styles::styles;
use clap::{Parser, Subcommand};
use std::ffi::OsString;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Tool CLI - Manage MCP tools.
#[derive(Debug, Parser)]
#[command(name = "tool", author, styles=styles())]
#[command(about = "Manage MCP tools and packages")]
pub struct Cli {
    /// Concise output for AI agents (minimal formatting, machine-parseable).
    #[arg(short, long, global = true)]
    pub concise: bool,

    /// Suppress header line in concise mode (requires -c).
    #[arg(short = 'H', long, global = true)]
    pub no_header: bool,

    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Available commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a new MCPB package.
    Init {
        /// Directory path to initialize (defaults to current directory).
        path: Option<String>,

        /// Package name (defaults to directory name).
        #[arg(short, long)]
        name: Option<String>,

        /// Server type: node, python, rust, or binary.
        #[arg(short = 't', long = "type")]
        server_type: Option<String>,

        /// Package description.
        #[arg(short, long)]
        description: Option<String>,

        /// Author name.
        #[arg(short, long)]
        author: Option<String>,

        /// License (SPDX identifier).
        #[arg(short, long)]
        license: Option<String>,

        /// Use HTTP transport instead of stdio.
        #[arg(long)]
        http: bool,

        /// Create reference manifest (no scaffolding).
        #[arg(long)]
        reference: bool,

        /// Skip prompts and use defaults.
        #[arg(short, long)]
        yes: bool,

        /// Package manager: npm, pnpm, bun, yarn, uv, pip, poetry.
        #[arg(long = "pm")]
        package_manager: Option<String>,

        /// Override detected entry point (for existing projects).
        #[arg(short, long)]
        entry: Option<String>,

        /// Override detected transport (stdio or http) for existing projects.
        #[arg(long)]
        transport: Option<String>,

        /// Force overwrite existing manifest.json.
        #[arg(short, long)]
        force: bool,
    },

    /// Determine if an existing MCP server can be converted to a MCPB package.
    Detect {
        /// Path to project directory (defaults to current directory).
        #[arg(default_value = ".")]
        path: String,

        /// Override detected entry point.
        #[arg(short, long)]
        entry: Option<String>,

        /// Override detected transport (stdio or http).
        #[arg(long)]
        transport: Option<String>,

        /// Override package name.
        #[arg(short, long)]
        name: Option<String>,
    },

    /// Search for tools in the registry.
    Search {
        /// Search query.
        query: String,
    },

    /// Install a tool from the registry or a local path.
    Install {
        /// Tool reference (`namespace/name[@version]`) or local path.
        name: String,
    },

    /// Uninstall an installed tool.
    Uninstall {
        /// Tool reference.
        name: String,
    },

    /// List installed tools.
    List {
        /// Filter by name pattern.
        filter: Option<String>,

        /// Output as JSON.
        #[arg(long)]
        json: bool,

        /// Include full tool info (tools, prompts, resources) for each server.
        #[arg(long)]
        full: bool,
    },

    /// Search installed tool schemas by pattern.
    Grep {
        /// Regex pattern to search for.
        pattern: String,

        /// Tool reference or path (default: search all installed tools).
        tool: Option<String>,

        /// Search tool names only.
        #[arg(short = 'n', long = "name")]
        name_only: bool,

        /// Search descriptions only.
        #[arg(short = 'd', long = "description")]
        description_only: bool,

        /// Search parameter names only.
        #[arg(short = 'p', long = "params")]
        params_only: bool,

        /// Case-insensitive search.
        #[arg(short = 'i', long = "ignore-case")]
        ignore_case: bool,

        /// List matching tool names only (no details).
        #[arg(short = 'l', long = "list")]
        list_only: bool,

        /// Output as JSON.
        #[arg(long)]
        json: bool,

        /// Max depth for expanding nested types in output schemas (default: 3).
        #[arg(short = 'L', long, default_value = "3")]
        level: usize,
    },

    /// Inspect a tool's capabilities.
    Info {
        /// Tool reference or path (default: current directory).
        #[arg(default_value = ".")]
        tool: String,

        /// Show only tools.
        #[arg(long)]
        tools: bool,

        /// Show only prompts.
        #[arg(long)]
        prompts: bool,

        /// Show only resources.
        #[arg(long)]
        resources: bool,

        /// Show all capabilities.
        #[arg(short, long)]
        all: bool,

        /// Output as JSON.
        #[arg(long)]
        json: bool,

        /// Configuration values (KEY=VALUE).
        #[arg(short = 'k', long)]
        config: Vec<String>,

        /// Path to config file (JSON).
        #[arg(long)]
        config_file: Option<String>,

        /// Don't auto-save config values for future use.
        #[arg(long)]
        no_save: bool,

        /// Skip interactive prompts (error if required config missing).
        #[arg(short, long)]
        yes: bool,

        /// Show verbose output.
        #[arg(short, long)]
        verbose: bool,

        /// Max depth for expanding nested types in output schemas (default: 3).
        #[arg(short = 'L', long, default_value = "3")]
        level: usize,
    },

    /// Call a tool.
    Call {
        /// Tool reference or path (default: current directory).
        #[arg(default_value = ".")]
        tool: String,

        /// Method name to call (use .method as shorthand for tool__method).
        #[arg(short, long)]
        method: String,

        /// Method parameters (KEY=VALUE or KEY=JSON).
        #[arg(short, long)]
        param: Vec<String>,

        /// Method parameters as trailing arguments (KEY=VALUE or KEY=JSON).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,

        /// Configuration values (KEY=VALUE).
        #[arg(short = 'k', long)]
        config: Vec<String>,

        /// Path to config file (JSON).
        #[arg(long)]
        config_file: Option<String>,

        /// Don't auto-save config values for future use.
        #[arg(long)]
        no_save: bool,

        /// Skip interactive prompts (error if required config missing).
        #[arg(short = 'y', long)]
        yes: bool,

        /// Show verbose output.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Download a tool from the registry.
    Download {
        /// Tool reference (`namespace/name[@version]`).
        name: String,

        /// Download to this directory (defaults to current directory).
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Validate an MCPB package.
    Validate {
        /// Path to tool directory (defaults to current directory).
        path: Option<String>,

        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,

        /// Output as JSON.
        #[arg(long)]
        json: bool,

        /// Show only errors, no details.
        #[arg(short, long)]
        quiet: bool,
    },

    /// Pack a tool into an .mcpb bundle.
    Pack {
        /// Path to tool directory (defaults to current directory).
        path: Option<String>,

        /// Output file path.
        #[arg(short, long)]
        output: Option<String>,

        /// Skip validation before packing.
        #[arg(long)]
        no_validate: bool,

        /// Include dotfiles (except .git/).
        #[arg(long)]
        include_dotfiles: bool,

        /// Show files being added.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Run an MCP server in proxy mode.
    Run {
        /// Tool reference or path (default: current directory).
        #[arg(default_value = ".")]
        tool: String,

        /// Expose transport type (stdio or http). Uses native transport if not specified.
        #[arg(long, value_name = "TRANSPORT")]
        expose: Option<String>,

        /// Port for HTTP expose mode.
        #[arg(short, long, default_value = "3000")]
        port: u16,

        /// Bind address for HTTP expose mode.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Configuration values (KEY=VALUE).
        #[arg(short = 'k', long)]
        config: Vec<String>,

        /// Path to config file (JSON).
        #[arg(long)]
        config_file: Option<String>,

        /// Don't auto-save config values for future use.
        #[arg(long)]
        no_save: bool,

        /// Skip interactive prompts (error if required config missing).
        #[arg(short, long)]
        yes: bool,

        /// Show verbose output.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Publish a tool to the registry.
    Publish {
        /// Path to tool directory.
        path: Option<String>,

        /// Validate without uploading.
        #[arg(long)]
        dry_run: bool,
    },

    /// Login to the registry.
    Login {
        /// API token (prompts if not provided).
        #[arg(long)]
        token: Option<String>,
    },

    /// Logout from the registry.
    Logout,

    /// Show authentication status.
    Whoami,

    /// Manage the tool-cli installation itself.
    #[command(name = "self", subcommand)]
    SelfCmd(SelfCommand),

    /// Configure tool settings and authentication.
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Manage MCP host configurations.
    #[command(subcommand)]
    Host(HostCommand),

    /// Catch-all for dynamic script names (e.g., `tool build`, `tool test`).
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

/// Self-management commands for tool-cli.
#[derive(Debug, Subcommand)]
pub enum SelfCommand {
    /// Update tool-cli to the latest version.
    Update {
        /// Only check for updates, don't install.
        #[arg(long)]
        check: bool,

        /// Install a specific version.
        #[arg(long)]
        version: Option<String>,
    },

    /// Uninstall tool-cli from this system.
    Uninstall {
        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,
    },
}

/// Config subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Set configuration values and authenticate HTTP tools.
    Set {
        /// Tool reference.
        tool: String,

        /// Configuration values as trailing args (KEY=VALUE).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        values: Vec<String>,

        /// Skip interactive prompts, use provided values only.
        #[arg(short, long)]
        yes: bool,

        /// Configuration values (KEY=VALUE), repeatable.
        #[arg(short = 'k', long = "config")]
        config: Vec<String>,
    },

    /// Show configuration for a tool.
    Get {
        /// Tool reference.
        tool: String,

        /// Specific key to show (shows all if omitted).
        key: Option<String>,
    },

    /// List all tools with saved configuration.
    List,

    /// Remove a specific configuration key.
    Unset {
        /// Tool reference.
        tool: String,

        /// Key to remove.
        key: String,
    },

    /// Remove all configuration for a tool.
    Reset {
        /// Tool reference.
        tool: String,
    },
}

/// Host subcommands for managing MCP host configurations.
#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// Register tools with an MCP host.
    Add {
        /// Target host (claude-desktop, cursor, claude-code).
        host: String,

        /// Specific tools to register (default: all installed).
        tools: Vec<String>,

        /// Preview changes without modifying files.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing entries for these tools.
        #[arg(long)]
        overwrite: bool,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,
    },

    /// Remove tools from an MCP host.
    Remove {
        /// Target host.
        host: String,

        /// Specific tools to remove (default: all tool-cli managed).
        tools: Vec<String>,

        /// Preview changes without modifying files.
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,
    },

    /// List supported hosts and their status.
    List,

    /// Show the MCP config that would be generated.
    Show {
        /// Target host.
        host: String,

        /// Specific tools (default: all installed).
        tools: Vec<String>,
    },

    /// Print config file path for a host.
    Path {
        /// Target host.
        host: String,
    },
}
