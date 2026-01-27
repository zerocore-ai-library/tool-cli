//! Interactive CLI prompts for `tool init`.
//!
//! Uses cliclack with a custom theme matching the tool.store style (emerald green).

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use cliclack::{Theme, ThemeState, input, intro, outro, select, set_theme};
use console::{Style, Term};

use crate::error::{ToolError, ToolResult};
use crate::mcpb::{
    InitMode, McpbServerType, McpbTransport, NodePackageManager, PackageManager,
    PythonPackageManager,
};

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

static CTRLC_HANDLER_SET: AtomicBool = AtomicBool::new(false);

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Custom theme matching the tool.store style.
/// Uses emerald green as the primary/accent color.
pub struct ToolTheme;

/// Result from the MCPB interactive prompt.
pub struct McpbPromptResult {
    pub name: String,
    pub mode: InitMode,
    pub is_rust: bool,
    pub description: Option<String>,
    pub license: Option<String>,
    pub author: Option<String>,
}

/// Pre-filled values for MCPB prompt (skip prompts for Some values).
#[derive(Default)]
pub struct McpbPrefill {
    pub name: Option<String>,
    pub reference: bool,
    pub server_type: Option<McpbServerType>,
    pub transport: Option<McpbTransport>,
    pub package_manager: Option<PackageManager>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub author: Option<String>,
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl Theme for ToolTheme {
    fn bar_color(&self, state: &ThemeState) -> Style {
        match state {
            ThemeState::Active => Style::new().color256(42), // Emerald-ish in 256 color mode
            ThemeState::Error(_) => Style::new().red(),
            _ => Style::new().dim(),
        }
    }

    fn state_symbol_color(&self, state: &ThemeState) -> Style {
        match state {
            ThemeState::Active => Style::new().color256(42),
            ThemeState::Submit => Style::new().color256(42),
            ThemeState::Error(_) => Style::new().red(),
            _ => Style::new().dim(),
        }
    }

    fn input_style(&self, _state: &ThemeState) -> Style {
        Style::new()
    }

    fn placeholder_style(&self, _state: &ThemeState) -> Style {
        Style::new().dim()
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Initialize the theme for cliclack prompts and set up Ctrl+C handler.
pub fn init_theme() {
    set_theme(ToolTheme);

    // Set up Ctrl+C handler to restore terminal state (only once)
    if !CTRLC_HANDLER_SET.swap(true, Ordering::SeqCst) {
        let _ = ctrlc::set_handler(|| {
            // Restore cursor and terminal state
            let term = Term::stderr();
            let _ = term.show_cursor();
            std::process::exit(130); // Standard exit code for Ctrl+C
        });
    }
}

/// Check if an error indicates the user cancelled (ESC or Ctrl+C).
fn is_cancelled(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::Interrupted
}

/// Convert IO interrupted errors to Cancelled for clean exit on ESC.
fn map_cancelled<T>(result: Result<T, std::io::Error>) -> ToolResult<T> {
    result.map_err(|e| {
        if is_cancelled(&e) {
            ToolError::Cancelled
        } else {
            ToolError::Io(e)
        }
    })
}

/// Prompt for package manager if not prefilled (only for Node and Python).
fn prompt_package_manager(
    server_type: McpbServerType,
    prefill: Option<PackageManager>,
) -> ToolResult<Option<PackageManager>> {
    if let Some(pm) = prefill {
        return Ok(Some(pm));
    }

    match server_type {
        McpbServerType::Node => {
            let pm: &str = map_cancelled(
                select("Package manager")
                    .item("npm", "npm", "Node Package Manager")
                    .item("pnpm", "pnpm", "Fast, disk space efficient")
                    .item("bun", "bun", "All-in-one JavaScript runtime")
                    .item("yarn", "yarn", "Yarn package manager")
                    .interact(),
            )?;
            Ok(Some(PackageManager::Node(match pm {
                "pnpm" => NodePackageManager::Pnpm,
                "bun" => NodePackageManager::Bun,
                "yarn" => NodePackageManager::Yarn,
                _ => NodePackageManager::Npm,
            })))
        }
        McpbServerType::Python => {
            let pm: &str = map_cancelled(
                select("Package manager")
                    .item("uv", "uv", "Fast Python package manager")
                    .item("pip", "pip", "Standard Python package installer")
                    .item("poetry", "poetry", "Dependency management and packaging")
                    .interact(),
            )?;
            Ok(Some(PackageManager::Python(match pm {
                "pip" => PythonPackageManager::Pip,
                "poetry" => PythonPackageManager::Poetry,
                _ => PythonPackageManager::Uv,
            })))
        }
        McpbServerType::Binary => Ok(None),
    }
}

/// Prompt for transport if not prefilled.
fn prompt_transport(prefill: Option<McpbTransport>) -> ToolResult<McpbTransport> {
    if let Some(t) = prefill {
        return Ok(t);
    }

    let t: &str = map_cancelled(
        select("Transport")
            .item("stdio", "Stdio", "Communicate via stdin/stdout")
            .item("http", "HTTP", "Run HTTP server, connect via HTTP [mcpbx]")
            .interact(),
    )?;
    Ok(if t == "http" {
        McpbTransport::Http
    } else {
        McpbTransport::Stdio
    })
}

/// Validate package name format.
fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    let mut chars = name.chars();

    // Must start with lowercase letter
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }

    // Rest must be lowercase letters, numbers, or hyphens
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Run interactive prompts for MCPB package initialization.
///
/// Skips prompts for any values already provided in the prefill struct.
pub fn prompt_init_mcpb(
    prefill: McpbPrefill,
    default_name: Option<&str>,
    default_author: Option<&str>,
) -> ToolResult<McpbPromptResult> {
    init_theme();

    map_cancelled(intro("tool init"))?;

    // Package name (required) - skip if prefilled
    let name = if let Some(name) = prefill.name {
        name
    } else {
        map_cancelled(
            input("Package name")
                .placeholder(default_name.unwrap_or("my-tool"))
                .default_input(default_name.unwrap_or(""))
                .validate(|input: &String| {
                    if input.is_empty() {
                        Err("Package name is required")
                    } else if !is_valid_package_name(input) {
                        Err("Must be lowercase letters, numbers, and hyphens, starting with a letter")
                    } else {
                        Ok(())
                    }
                })
                .interact(),
        )?
    };

    // Mode selection - prompt only for unprefilled components
    let (mode, is_rust) = if prefill.reference {
        // Reference mode: only need transport
        let transport = if let Some(t) = prefill.transport {
            t
        } else {
            let t: &str = map_cancelled(
                select("Transport")
                    .item("http", "HTTP", "Connect to remote server via HTTP [mcpbx]")
                    .item(
                        "stdio",
                        "Stdio",
                        "Spawn external command, communicate via stdin/stdout [mcpbx]",
                    )
                    .interact(),
            )?;
            if t == "http" {
                McpbTransport::Http
            } else {
                McpbTransport::Stdio
            }
        };
        (InitMode::Reference { transport }, false)
    } else if let Some(server_type) = prefill.server_type {
        // Bundle mode with prefilled server type
        let package_manager = prompt_package_manager(server_type, prefill.package_manager)?;
        let transport = prompt_transport(prefill.transport)?;
        (
            InitMode::Bundle {
                server_type,
                transport,
                package_manager,
            },
            false,
        )
    } else {
        // Nothing prefilled - ask bundle vs reference first
        let is_reference: &str = map_cancelled(
            select("Package type")
                .item("bundle", "Bundle", "Create server code and package it")
                .item(
                    "reference",
                    "Reference",
                    "Point to external server (no scaffold) [mcpbx]",
                )
                .interact(),
        )?;

        if is_reference == "reference" {
            let transport: &str = map_cancelled(
                select("Transport")
                    .item("http", "HTTP", "Connect to remote server via HTTP [mcpbx]")
                    .item(
                        "stdio",
                        "Stdio",
                        "Spawn external command, communicate via stdin/stdout [mcpbx]",
                    )
                    .interact(),
            )?;
            let transport = if transport == "http" {
                McpbTransport::Http
            } else {
                McpbTransport::Stdio
            };
            (InitMode::Reference { transport }, false)
        } else {
            let server_type_str: &str = map_cancelled(
                select("Server type")
                    .item("node", "Node.js", "JavaScript/TypeScript MCP server")
                    .item("python", "Python", "Python MCP server")
                    .item("rust", "Rust", "Rust MCP server")
                    .item("binary", "Binary", "Pre-built binary")
                    .interact(),
            )?;
            let is_rust = server_type_str == "rust";
            let server_type = match server_type_str {
                "node" => McpbServerType::Node,
                "python" => McpbServerType::Python,
                "rust" | "binary" => McpbServerType::Binary,
                _ => McpbServerType::Node,
            };
            let package_manager = prompt_package_manager(server_type, None)?;
            let transport = prompt_transport(None)?;
            (
                InitMode::Bundle {
                    server_type,
                    transport,
                    package_manager,
                },
                is_rust,
            )
        }
    };

    // Description (optional) - skip if prefilled
    let description = if prefill.description.is_some() {
        prefill.description
    } else {
        let desc: String =
            map_cancelled(input("Description (optional)").required(false).interact())?;
        if desc.is_empty() { None } else { Some(desc) }
    };

    // License (optional) - skip if prefilled
    let license = if prefill.license.is_some() {
        prefill.license
    } else {
        let lic: String = map_cancelled(input("License (optional)").required(false).interact())?;
        if lic.is_empty() { None } else { Some(lic) }
    };

    // Author (optional) - skip if prefilled
    let author = if prefill.author.is_some() {
        prefill.author
    } else {
        let mut author_input = input("Author").required(false);
        if let Some(author_default) = default_author {
            author_input = author_input
                .placeholder(author_default)
                .default_input(author_default);
        }
        let auth: String = map_cancelled(author_input.interact())?;
        if auth.is_empty() { None } else { Some(auth) }
    };

    map_cancelled(outro("Configuration complete!"))?;

    Ok(McpbPromptResult {
        name,
        mode,
        is_rust,
        description,
        license,
        author,
    })
}

/// Try to get author name from git config.
pub fn get_git_author_name() -> Option<String> {
    Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}
