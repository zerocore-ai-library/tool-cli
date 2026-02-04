//! CLI command definitions.

use crate::styles::styles;
use crate::{examples, examples_section};
use clap::{Parser, Subcommand};
use std::ffi::OsString;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

const INIT_EXAMPLES: &str = examples![
    "tool init                         " # "Interactive mode in current directory",
    "tool init my-tool -t node         " # "Create Node.js MCP server",
    "tool init my-tool -t python       " # "Create Python MCP server",
    "tool init my-tool -t node -y      " # "Skip prompts, use defaults",
    "tool init . --http                " # "Use HTTP transport instead of stdio",
    "tool init existing-project        " # "Detect and migrate existing MCP server",
    "tool init . --reference           " # "Create manifest only (no scaffolding)",
    "tool init . --pm pnpm             " # "Use pnpm as package manager",
    "tool init . --command npx --args \"@anthropic/mcp-server\"" # "Reference external command",
    "tool init . --url https://api.example.com/mcp/" # "Reference remote HTTP server",
    "tool init . --url https://example.com --oauth-client-id abc" # "HTTP with OAuth",
];

const DETECT_EXAMPLES: &str = examples![
    "tool detect                       " # "Analyze current directory",
    "tool detect ./my-server           " # "Analyze specific project",
    "tool detect -e src/main.py        " # "Override detected entry point",
    "tool detect --transport http      " # "Override detected transport",
    "tool detect -n custom-name        " # "Override detected package name",
];

const SEARCH_EXAMPLES: &str = examples![
    "tool search filesystem            " # "Find file-related tools",
    "tool search weather               " # "Find weather tools",
    "tool search \"database sql\"        " # "Multi-word search",
    "tool search bash -c               " # "Concise output for scripts",
];

const INSTALL_EXAMPLES: &str = examples![
    "tool install appcypher/bash              " # "Install from registry (latest)",
    "tool install appcypher/bash@1.0.0        " # "Install specific version",
    "tool install ./my-local-tool             " # "Install from local directory",
    "tool install ~/tools/custom              " # "Install from home directory",
    "tool install ./local ns/a ns/b           " # "Install multiple packages",
    "tool install ns/tool --platform=universal" # "Install universal bundle",
];

const UNINSTALL_EXAMPLES: &str = examples![
    "tool uninstall appcypher/bash     " # "Remove installed tool",
    "tool uninstall my-local-tool      " # "Remove local tool",
    "tool uninstall tool1 tool2 tool3  " # "Remove multiple tools",
    "tool uninstall --all              " # "Remove all installed tools",
    "tool uninstall --all -y           " # "Remove all without confirmation",
];

const LIST_EXAMPLES: &str = examples![
    "tool list                         " # "List all installed tools",
    "tool list bash                    " # "Filter by name pattern",
    "tool list -c                      " # "Concise output for scripts",
    "tool list --full                  " # "Include tools, prompts, resources",
    "tool list --json                  " # "JSON output for parsing",
];

const GREP_EXAMPLES: &str = examples![
    "tool grep file                    " # "Search \"file\" across all tools",
    "tool grep temperature -c          " # "Concise output",
    "tool grep \"read|write\" bash       " # "Regex search in specific tool",
    "tool grep api_key -d              " # "Search descriptions only",
    "tool grep path -n                 " # "Search names/keys only",
    "tool grep config --input          " # "Search input schemas only",
    "tool grep result --output         " # "Search output schemas only",
    "tool grep database -m query       " # "Search within specific method",
    "tool grep \"^get_\" -i              " # "Case-insensitive regex",
    "tool grep file -l                 " # "List matching paths only",
];

const INFO_EXAMPLES: &str = examples![
    "tool info                         " # "Inspect tool in current directory",
    "tool info appcypher/bash          " # "Inspect installed tool",
    "tool info appcypher/bash -c       " # "Concise output",
    "tool info . -m exec               " # "Show specific method details",
    "tool info . -m exec -m read       " # "Show multiple methods",
    "tool info . -m exec --input       " # "Show only input schema",
    "tool info . -m exec --output      " # "Show only output schema",
    "tool info . -m exec -d            " # "Show only description",
    "tool info . --tools               " # "List tools only",
    "tool info . --prompts             " # "List prompts only",
    "tool info . --resources           " # "List resources only",
    "tool info . -a                    " # "Show all capabilities",
    "tool info . --json                " # "JSON output for parsing",
    "tool info . -k API_KEY=xxx        " # "Pass config value",
    "tool info . -L 5                  " # "Expand nested types to depth 5",
];

const CALL_EXAMPLES: &str = examples![
    "tool call . -m exec -p command=\"ls\" " # "Call method in current dir",
    "tool call bash -m .exec -p cmd=\"pwd\"" # "Shorthand: .exec -> bash__exec",
    "tool call bash -m .exec -p cmd=ls   " # "Use -p flag for params",
    "tool call weather -m get -p loc=NYC " # "Unquoted param value",
    "tool call api -m query -k KEY=xxx   " # "Pass config inline",
    "tool call . -m test --config-file   " # "Config from file",
    "tool call . -m run -y               " # "Skip interactive prompts",
    "tool call . -m debug -v             " # "Verbose output",
];

const DOWNLOAD_EXAMPLES: &str = examples![
    "tool download appcypher/bash                  " # "Download to current dir",
    "tool download appcypher/bash@1.0.0            " # "Download specific version",
    "tool download ns/a ns/b ns/c                  " # "Download multiple packages",
    "tool download ns/tool -o ./dist               " # "Download to specific directory",
    "tool download ns/tool --platform=darwin-arm64 " # "Download for specific platform",
    "tool download ns/tool --platform=universal    " # "Download universal bundle",
];

const VALIDATE_EXAMPLES: &str = examples![
    "tool validate                     " # "Validate current directory",
    "tool validate ./my-tool           " # "Validate specific path",
    "tool validate --strict            " # "Treat warnings as errors",
    "tool validate --json              " # "JSON output for CI/CD",
    "tool validate -q                  " # "Quiet mode (errors only)",
];

const PACK_EXAMPLES: &str = examples![
    "tool pack                         " # "Pack current directory",
    "tool pack ./my-tool               " # "Pack specific directory",
    "tool pack -o release.mcpb         " # "Custom output filename",
    "tool pack --no-validate           " # "Skip validation step",
    "tool pack --include-dotfiles      " # "Include dotfiles (except .git)",
    "tool pack -v                      " # "Show files being added",
    "tool pack --multi-platform        " # "Pack bundles for each platform override",
];

const RUN_EXAMPLES: &str = examples![
    "tool run                          " # "Run tool in current directory",
    "tool run appcypher/bash           " # "Run installed tool",
    "tool run . --expose http          " # "Expose stdio tool via HTTP",
    "tool run . --expose http -p 8080  " # "Custom port",
    "tool run . --expose http --host 0 " # "Bind to all interfaces",
    "tool run . -k API_KEY=xxx         " # "Pass config value",
    "tool run . --config-file creds.json" # "Config from file",
    "tool run . -v                     " # "Verbose output",
];

const PUBLISH_EXAMPLES: &str = examples![
    "tool publish                                                 " # "Publish current directory",
    "tool publish ./my-tool                                       " # "Publish specific directory",
    "tool publish --dry-run                                       " # "Preview without uploading",
    "tool publish --multi-platform                                " # "Publish bundles for each platform",
    "tool publish --multi-platform --darwin-arm64 ./dist/mac.mcpb " # "Use pre-built bundle",
    "tool publish --multi-platform --universal ./dist/all.mcpb    " # "Specify universal bundle",
];

const LOGIN_EXAMPLES: &str = examples![
    "tool login                        " # "Interactive login (prompts for token)",
    "tool login --token \"your-token\"   " # "Non-interactive login",
];

const SELF_UPDATE_EXAMPLES: &str = examples![
    "tool self update                  " # "Update to latest version",
    "tool self update --check          " # "Check for updates only",
    "tool self update --version 0.2.0  " # "Install specific version",
];

const SELF_UNINSTALL_EXAMPLES: &str = examples![
    "tool self uninstall               " # "Uninstall (with confirmation)",
    "tool self uninstall -y            " # "Uninstall without confirmation",
];

const CONFIG_SET_EXAMPLES: &str = examples![
    "tool config set bash API_KEY=xxx  " # "Set single value",
    "tool config set weather k=a u=m   " # "Set multiple values",
    "tool config set api -k TOKEN=xxx  " # "Use -k flag",
    "tool config set service           " # "Interactive prompts",
    "tool config set api -y key=xxx    " # "Non-interactive",
];

const CONFIG_GET_EXAMPLES: &str = examples![
    "tool config get bash              " # "Show all config for tool",
    "tool config get bash API_KEY      " # "Show specific key",
];

const CONFIG_UNSET_EXAMPLES: &str = examples![
    "tool config unset bash API_KEY    " # "Remove specific key",
    "tool config unset bash K1 K2 K3   " # "Remove multiple keys",
    "tool config unset bash --all      " # "Remove all keys for tool",
    "tool config unset --all           " # "Remove config for all tools",
    "tool config unset --all API_KEY   " # "Remove key from all tools",
    "tool config unset --all -y        " # "Skip confirmation prompt",
];

const HOST_ADD_EXAMPLES: &str = examples![
    "tool host add claude-desktop      " # "Add all tools",
    "tool host add claude-desktop bash " # "Add specific tools",
    "tool host add cursor appcypher/bash" # "Add to Cursor",
    "tool host add vscode --dry-run    " # "Preview changes",
    "tool host add claude-code --overwrite" # "Overwrite existing",
    "tool host add claude-desktop -y   " # "Skip confirmation",
];

const HOST_REMOVE_EXAMPLES: &str = examples![
    "tool host remove claude-desktop   " # "Remove all managed tools",
    "tool host remove claude-desktop bash" # "Remove specific tool",
    "tool host remove cursor --dry-run " # "Preview changes",
    "tool host remove vscode -y        " # "Skip confirmation",
];

const HOST_PREVIEW_EXAMPLES: &str = examples![
    "tool host preview claude-desktop  " # "Preview config for all tools",
    "tool host preview cursor bash     " # "Preview config for specific tool",
];

const HOST_PATH_EXAMPLES: &str = examples![
    "tool host path claude-desktop     " # "Print config file location",
    "tool host path cursor             " # "Print Cursor config path",
];

const CLI_EXAMPLES: &str = concat!(
    examples![
        "tool init                              " # "Create a new MCP server (interactive)",
        "tool install appcypher/bash            " # "Install a tool from the registry",
        "tool list                              " # "List installed tools",
        "tool info appcypher/bash               " # "Inspect a tool's capabilities",
        "tool call bash -m .exec -p cmd=\"ls\"    " # "Call a tool method",
        "tool grep file                         " # "Search tool schemas",
        "tool host add claude-desktop           " # "Register tools with Claude Desktop",
        "tool run . --expose http               " # "Run tool as HTTP server",
    ],
    "\n\n",
    examples_section!["Getting started:";
        "tool init                         " # "1. Create new tool (interactive)",
        "tool build                        " # "2. Build the tool",
        "tool info                         " # "3. Verify it works",
        "tool call . -m hello              " # "4. Call a method",
    ],
);

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Tool CLI - Manage MCP tools.
#[derive(Debug, Parser)]
#[command(name = "tool", author, version, styles=styles())]
#[command(about = "Manage MCP tools and packages", after_help = CLI_EXAMPLES)]
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
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Initialize a new MCPB package.
    #[command(after_help = INIT_EXAMPLES)]
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

        /// Verify detection by starting the server and sending an MCP initialize request.
        #[arg(long)]
        verify: bool,

        // === Reference mode options (mcp_config overrides) ===
        /// Command to execute (implies reference mode for stdio).
        #[arg(long)]
        command: Option<String>,

        /// Command arguments (space-separated string).
        #[arg(long, allow_hyphen_values = true)]
        args: Option<String>,

        /// Environment variables as KEY=VALUE (repeatable).
        #[arg(long = "env")]
        env: Vec<String>,

        /// Server URL (implies HTTP reference mode).
        #[arg(long)]
        url: Option<String>,

        /// HTTP headers as KEY=VALUE (repeatable).
        #[arg(long = "header")]
        headers: Vec<String>,

        /// OAuth client ID.
        #[arg(long = "oauth-client-id")]
        oauth_client_id: Option<String>,

        /// OAuth authorization endpoint URL.
        #[arg(long = "oauth-authorization-url")]
        oauth_authorization_url: Option<String>,

        /// OAuth token endpoint URL.
        #[arg(long = "oauth-token-url")]
        oauth_token_url: Option<String>,

        /// OAuth scopes (comma-separated).
        #[arg(long = "oauth-scopes")]
        oauth_scopes: Option<String>,
    },

    /// Determine if an existing MCP server can be converted to a MCPB package.
    #[command(after_help = DETECT_EXAMPLES)]
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

        /// Verify detection by starting the server and sending an MCP initialize request.
        #[arg(long)]
        verify: bool,

        /// Skip confirmation prompt for --verify.
        #[arg(short, long)]
        yes: bool,
    },

    /// Search for tools in the registry.
    #[command(after_help = SEARCH_EXAMPLES)]
    Search {
        /// Search query.
        query: String,
    },

    /// Install tools from the registry or local paths.
    #[command(after_help = INSTALL_EXAMPLES)]
    Install {
        /// Tool references (`namespace/name[@version]`) or local paths.
        #[arg(required = true)]
        names: Vec<String>,

        /// Override platform detection (use "universal" for universal bundle).
        #[arg(long)]
        platform: Option<String>,
    },

    /// Uninstall installed tools.
    #[command(after_help = UNINSTALL_EXAMPLES)]
    Uninstall {
        /// Tool references.
        names: Vec<String>,

        /// Uninstall all installed tools.
        #[arg(long)]
        all: bool,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,
    },

    /// List installed tools.
    #[command(after_help = LIST_EXAMPLES)]
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
    #[command(after_help = GREP_EXAMPLES)]
    Grep {
        /// Regex pattern to search for.
        pattern: String,

        /// Tool reference or path (default: search all installed tools).
        tool: Option<String>,

        /// Search within a specific method only.
        #[arg(short = 'm', long = "method")]
        method: Option<String>,

        /// Search only in input schemas.
        #[arg(long = "input")]
        input_only: bool,

        /// Search only in output schemas.
        #[arg(long = "output")]
        output_only: bool,

        /// Search names/keys only.
        #[arg(short = 'n', long = "name")]
        name_only: bool,

        /// Search descriptions only.
        #[arg(short = 'd', long = "description")]
        description_only: bool,

        /// Case-insensitive search.
        #[arg(short = 'i', long = "ignore-case")]
        ignore_case: bool,

        /// List matching paths only (no values).
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
    #[command(after_help = INFO_EXAMPLES)]
    Info {
        /// Tool reference or path (default: current directory).
        #[arg(default_value = ".")]
        tool: String,

        /// Focus on specific methods by name (can be repeated).
        #[arg(short = 'm', long = "method")]
        methods: Vec<String>,

        /// Show only input schema (requires -m).
        #[arg(long = "input")]
        input_only: bool,

        /// Show only output schema (requires -m).
        #[arg(long = "output")]
        output_only: bool,

        /// Show only description.
        #[arg(short = 'd', long = "description")]
        description_only: bool,

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
    #[command(after_help = CALL_EXAMPLES)]
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

        /// Output raw content without decorations.
        #[arg(long)]
        json: bool,
    },

    /// Download tools from the registry.
    #[command(after_help = DOWNLOAD_EXAMPLES)]
    Download {
        /// Tool references (`namespace/name[@version]`).
        #[arg(required = true)]
        names: Vec<String>,

        /// Download to this directory (defaults to current directory).
        #[arg(short, long)]
        output: Option<String>,

        /// Target platform (e.g., "darwin-arm64", "linux-x64", or "universal").
        /// Defaults to auto-detect, falling back to universal if no match.
        #[arg(long)]
        platform: Option<String>,
    },

    /// Validate an MCPB package.
    #[command(after_help = VALIDATE_EXAMPLES)]
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
    #[command(after_help = PACK_EXAMPLES)]
    Pack {
        /// Path to tool directory (defaults to current directory).
        path: Option<String>,

        /// Output file path (ignored with --multi-platform).
        #[arg(short, long)]
        output: Option<String>,

        /// Skip validation before packing.
        #[arg(long)]
        no_validate: bool,

        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,

        /// Include dotfiles (except .git/).
        #[arg(long)]
        include_dotfiles: bool,

        /// Show files being added.
        #[arg(short, long)]
        verbose: bool,

        /// Create bundles for each platform override (+ universal bundle).
        /// Checks _meta["store.tool.mcpb"].mcp_config.platform_overrides first,
        /// then falls back to server.mcp_config.platform_overrides.
        #[arg(long)]
        multi_platform: bool,
    },

    /// Run an MCP server in proxy mode.
    #[command(after_help = RUN_EXAMPLES)]
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
    #[command(after_help = PUBLISH_EXAMPLES)]
    Publish {
        /// Path to tool directory.
        path: Option<String>,

        /// Validate without uploading.
        #[arg(long)]
        dry_run: bool,

        /// Treat warnings as errors.
        #[arg(long)]
        strict: bool,

        /// Publish bundles for each platform override (+ universal bundle).
        /// Can be combined with platform flags to use pre-built bundles.
        #[arg(long)]
        multi_platform: bool,

        /// Pre-built bundle for darwin-arm64 (Apple Silicon Mac).
        #[arg(long, value_name = "PATH")]
        darwin_arm64: Option<String>,

        /// Pre-built bundle for darwin-x64 (Intel Mac).
        #[arg(long, value_name = "PATH")]
        darwin_x64: Option<String>,

        /// Pre-built bundle for linux-x64.
        #[arg(long, value_name = "PATH")]
        linux_x64: Option<String>,

        /// Pre-built bundle for linux-arm64.
        #[arg(long, value_name = "PATH")]
        linux_arm64: Option<String>,

        /// Pre-built bundle for win32-x64 (Windows x64).
        #[arg(long, value_name = "PATH")]
        win32_x64: Option<String>,

        /// Pre-built universal bundle (all platforms).
        #[arg(long, value_name = "PATH")]
        universal: Option<String>,
    },

    /// Login to the registry.
    #[command(after_help = LOGIN_EXAMPLES)]
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
    #[command(after_help = SELF_UPDATE_EXAMPLES)]
    Update {
        /// Only check for updates, don't install.
        #[arg(long)]
        check: bool,

        /// Install a specific version.
        #[arg(long)]
        version: Option<String>,
    },

    /// Uninstall tool-cli from this system.
    #[command(after_help = SELF_UNINSTALL_EXAMPLES)]
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
    #[command(after_help = CONFIG_SET_EXAMPLES)]
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
    #[command(after_help = CONFIG_GET_EXAMPLES)]
    Get {
        /// Tool reference.
        tool: String,

        /// Specific key to show (shows all if omitted).
        key: Option<String>,
    },

    /// List all tools with saved configuration.
    List,

    /// Remove configuration keys.
    #[command(after_help = CONFIG_UNSET_EXAMPLES)]
    Unset {
        /// Tool reference (required unless --all is used).
        tool: Option<String>,

        /// Keys to remove.
        keys: Vec<String>,

        /// Remove all keys for the tool, or all config for all tools if no tool specified.
        #[arg(long)]
        all: bool,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        yes: bool,
    },
}

/// Host subcommands for managing MCP host configurations.
#[derive(Debug, Subcommand)]
pub enum HostCommand {
    /// Register tools with an MCP host.
    #[command(after_help = HOST_ADD_EXAMPLES)]
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
    #[command(after_help = HOST_REMOVE_EXAMPLES)]
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

    /// Preview the MCP config that would be generated.
    #[command(after_help = HOST_PREVIEW_EXAMPLES)]
    Preview {
        /// Target host.
        host: String,

        /// Specific tools (default: all installed).
        tools: Vec<String>,
    },

    /// Print config file path for a host.
    #[command(after_help = HOST_PATH_EXAMPLES)]
    Path {
        /// Target host.
        host: String,
    },
}
