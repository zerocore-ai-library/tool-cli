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
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Available commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a new tool package.
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
    },

    /// Validate a tool manifest.
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

    /// Run a script defined in manifest.json.
    Run {
        /// Script name to run (e.g., build, test).
        script: Option<String>,

        /// Path to tool directory.
        path: Option<String>,

        /// List available scripts.
        #[arg(short, long)]
        list: bool,

        /// Additional arguments to pass to the script.
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// Catch-all for dynamic script names (e.g., `tool build`, `tool test`).
    #[command(external_subcommand)]
    External(Vec<OsString>),

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
        #[arg(short, long)]
        config: Vec<String>,

        /// Path to config file (JSON).
        #[arg(long)]
        config_file: Option<String>,

        /// Show verbose output.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Call a tool method.
    Call {
        /// Tool reference or path (default: current directory).
        #[arg(default_value = ".")]
        tool: String,

        /// Method name to call.
        #[arg(short, long)]
        method: String,

        /// Method parameters (KEY=VALUE or KEY=JSON).
        #[arg(short, long)]
        param: Vec<String>,

        /// Configuration values (KEY=VALUE).
        #[arg(short, long)]
        config: Vec<String>,

        /// Path to config file (JSON).
        #[arg(long)]
        config_file: Option<String>,

        /// Show verbose output.
        #[arg(short, long)]
        verbose: bool,
    },

    /// List installed tools.
    List {
        /// Filter by name pattern.
        filter: Option<String>,

        /// Output as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Download a tool from the registry.
    Download {
        /// Tool reference (namespace/name[@version]).
        name: String,

        /// Download to this directory (defaults to current directory).
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Add a tool from the registry.
    Add {
        /// Tool reference (namespace/name[@version]).
        name: String,
    },

    /// Remove an installed tool.
    Remove {
        /// Tool reference.
        name: String,
    },

    /// Search for tools in the registry.
    Search {
        /// Search query.
        query: String,
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
}
