//! Tool command handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::mcp::{ToolType, call_tool_from_path, get_tool_info_from_path};
use crate::mcpb::{
    InitMode, McpbAuthor, McpbManifest, McpbServerType, McpbTransport, McpbUserConfigField,
    NodePackageManager, PackageManager, PythonPackageManager,
};
use crate::pack::{PackError, PackOptions, pack_bundle};
use crate::resolver::{FilePluginResolver, load_tool_from_path};
use crate::scaffold::{
    mcpbignore_template, node_gitignore_template, node_scaffold, python_gitignore_template,
    python_scaffold, rust_gitignore_template, rust_mcpbignore_template, rust_scaffold,
};
use crate::validate::{ValidationResult, validate_manifest};
use colored::Colorize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

struct ToolListEntry {
    name: String,
    tool_type: String,
    description: Option<String>,
    path: PathBuf,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Initialize a new tool package.
#[allow(clippy::too_many_arguments)]
pub async fn init_mcpb(
    path: Option<String>,
    name: Option<String>,
    server_type: Option<String>,
    description: Option<String>,
    author: Option<String>,
    license: Option<String>,
    http: bool,
    reference: bool,
    yes: bool,
    package_manager: Option<String>,
) -> ToolResult<()> {
    use crate::prompt::{McpbPrefill, get_git_author_name, prompt_init_mcpb};

    // Determine target directory
    let target_dir = match &path {
        Some(p) => {
            let target = std::path::PathBuf::from(p);
            let target = if target.is_absolute() {
                target
            } else {
                std::env::current_dir()?.join(&target)
            };

            if !target.exists() {
                std::fs::create_dir_all(&target)?;
            }
            target
        }
        None => std::env::current_dir()?,
    };

    // Check if manifest.json already exists
    let manifest_path = target_dir.join(MCPB_MANIFEST_FILE);
    if manifest_path.exists() {
        return Err(ToolError::Generic(
            "manifest.json already exists. Use --force to overwrite.".into(),
        ));
    }

    // Resolve name: --name flag OR path argument (directory name)
    let resolved_name = name.or_else(|| {
        path.as_ref().and_then(|p| {
            std::path::Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
    });

    // Default name from target directory (for prompts and -y mode)
    let default_name = target_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string());

    // Parse CLI flags into individual components
    let parsed_server_type = server_type
        .as_ref()
        .and_then(|t| match t.to_lowercase().as_str() {
            "node" => Some(McpbServerType::Node),
            "python" => Some(McpbServerType::Python),
            "rust" | "binary" => Some(McpbServerType::Binary),
            _ => None,
        });

    let parsed_transport = if http {
        Some(McpbTransport::Http)
    } else {
        None
    };

    let parsed_pm = package_manager.as_deref().and_then(parse_package_manager);

    // Get final values based on -y flag
    let (pkg_name, mode, is_rust, description, license, author) = if yes {
        // Non-interactive: use CLI args or defaults
        let pkg_name = resolved_name.or(default_name.clone()).ok_or_else(|| {
            ToolError::Generic("Could not determine package name. Use --name.".into())
        })?;
        let mode = build_init_mode(reference, parsed_server_type, parsed_transport, parsed_pm);
        // Detect if this is a Rust bundle from CLI flag
        let is_rust = server_type
            .as_ref()
            .is_some_and(|t| t.to_lowercase() == "rust");
        (pkg_name, mode, is_rust, description, license, author)
    } else {
        // Interactive: prompt for values, prefill with CLI args
        let default_author = get_git_author_name();
        let prefill = McpbPrefill {
            name: resolved_name,
            reference,
            server_type: parsed_server_type,
            transport: parsed_transport,
            package_manager: parsed_pm,
            description,
            license,
            author,
        };
        let result = prompt_init_mcpb(prefill, default_name.as_deref(), default_author.as_deref())?;
        // Use is_rust from prompt result, or fall back to CLI flag if prefilled
        let is_rust = result.is_rust
            || server_type
                .as_ref()
                .is_some_and(|t| t.to_lowercase() == "rust");
        (
            result.name,
            result.mode,
            is_rust,
            result.description,
            result.license,
            result.author,
        )
    };

    // Validate name format
    if !is_valid_package_name(&pkg_name) {
        return Err(ToolError::Generic(format!(
            "Invalid package name \"{}\"\nName must be lowercase and contain only letters, numbers, and hyphens.",
            pkg_name
        )));
    }

    // Build manifest from mode
    let mut manifest = if is_rust {
        McpbManifest::new_rust_with_transport(&pkg_name, mode.transport())
    } else {
        McpbManifest::from_mode(&mode).with_name(&pkg_name)
    };

    if let Some(desc) = description {
        manifest = manifest.with_description(desc);
    }

    if let Some(lic) = license {
        manifest = manifest.with_license(lic);
    }

    // Try to get author from --author flag or git config
    if let Some(author_name) = author {
        manifest = manifest.with_author(McpbAuthor::new(author_name));
    } else if let Some(git_author) = get_git_author() {
        manifest = manifest.with_author(git_author);
    }

    // Write manifest.json
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, &manifest_json)?;

    // Write .mcpbignore
    let mcpbignore_path = target_dir.join(".mcpbignore");
    let mcpbignore_content = if is_rust {
        rust_mcpbignore_template()
    } else {
        mcpbignore_template()
    };
    std::fs::write(&mcpbignore_path, mcpbignore_content)?;

    // Write README.md
    let readme_path = target_dir.join("README.md");
    let readme_content = format!("# {}\n", pkg_name);
    std::fs::write(&readme_path, readme_content)?;

    // Write .gitignore (type-specific) - only for bundle modes
    let gitignore_path = target_dir.join(".gitignore");
    let gitignore_content = if mode.is_reference() {
        ""
    } else if is_rust {
        rust_gitignore_template()
    } else {
        match mode.server_type() {
            Some(McpbServerType::Node) => node_gitignore_template(),
            Some(McpbServerType::Python) => python_gitignore_template(),
            Some(McpbServerType::Binary) | None => "",
        }
    };
    if !gitignore_content.is_empty() {
        std::fs::write(&gitignore_path, gitignore_content)?;
    }

    // Write scaffold files for bundle mode only
    if !mode.is_reference() {
        let transport = mode.transport();

        if is_rust {
            let scaffold = rust_scaffold(&pkg_name, transport);
            let src_dir = target_dir.join("src");
            std::fs::create_dir_all(&src_dir)?;
            std::fs::write(src_dir.join("main.rs"), &scaffold.main_rs)?;
            std::fs::write(src_dir.join("lib.rs"), &scaffold.lib_rs)?;
            std::fs::write(target_dir.join("Cargo.toml"), &scaffold.cargo_toml)?;
        } else if let Some(server_type) = mode.server_type() {
            match server_type {
                McpbServerType::Node => {
                    let scaffold = node_scaffold(&pkg_name, transport);
                    let server_dir = target_dir.join("server");
                    std::fs::create_dir_all(&server_dir)?;
                    std::fs::write(server_dir.join("index.js"), &scaffold.index_js)?;
                    std::fs::write(target_dir.join("package.json"), &scaffold.package_json)?;
                }
                McpbServerType::Python => {
                    let pkg_manager = mode
                        .python_package_manager()
                        .unwrap_or(PythonPackageManager::default());
                    let scaffold = python_scaffold(&pkg_name, transport, pkg_manager);
                    let server_dir = target_dir.join("server");
                    std::fs::create_dir_all(&server_dir)?;
                    std::fs::write(server_dir.join("main.py"), &scaffold.main_py)?;
                    std::fs::write(
                        target_dir.join(scaffold.project_file_name),
                        &scaffold.project_file,
                    )?;
                }
                McpbServerType::Binary => {}
            }
        }
    }

    // Print success message
    print_init_success(&pkg_name, &mode, is_rust, path.as_deref());

    Ok(())
}

/// Build InitMode for non-interactive mode.
fn build_init_mode(
    reference: bool,
    server_type: Option<McpbServerType>,
    transport: Option<McpbTransport>,
    package_manager: Option<PackageManager>,
) -> InitMode {
    let transport = transport.unwrap_or(McpbTransport::Stdio);

    if reference {
        InitMode::Reference { transport }
    } else {
        let server_type = server_type.unwrap_or(match &package_manager {
            Some(PackageManager::Python(_)) => McpbServerType::Python,
            _ => McpbServerType::Node,
        });
        let package_manager = package_manager.or(match server_type {
            McpbServerType::Node => Some(PackageManager::Node(NodePackageManager::Npm)),
            McpbServerType::Python => Some(PackageManager::Python(PythonPackageManager::Uv)),
            McpbServerType::Binary => None,
        });
        InitMode::Bundle {
            server_type,
            transport,
            package_manager,
        }
    }
}

/// Parse a package manager string.
fn parse_package_manager(pm: &str) -> Option<PackageManager> {
    match pm.to_lowercase().as_str() {
        "npm" => Some(PackageManager::Node(NodePackageManager::Npm)),
        "pnpm" => Some(PackageManager::Node(NodePackageManager::Pnpm)),
        "bun" => Some(PackageManager::Node(NodePackageManager::Bun)),
        "yarn" => Some(PackageManager::Node(NodePackageManager::Yarn)),
        "uv" => Some(PackageManager::Python(PythonPackageManager::Uv)),
        "pip" => Some(PackageManager::Python(PythonPackageManager::Pip)),
        "poetry" => Some(PackageManager::Python(PythonPackageManager::Poetry)),
        _ => None,
    }
}

/// Validate package name format.
fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Try to get author info from git config.
fn get_git_author() -> Option<McpbAuthor> {
    let name = Command::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())?;

    let email = Command::new("git")
        .args(["config", "user.email"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let mut author = McpbAuthor::new(name);
    if let Some(email) = email {
        author.email = Some(email);
    }

    Some(author)
}

/// Print the scaffolding success output.
fn print_init_success(name: &str, mode: &InitMode, is_rust: bool, dir_path: Option<&str>) {
    let action = if mode.is_reference() {
        "Created"
    } else {
        "Scaffolded"
    };
    println!("  {} {} {}\n", "✓".bright_green(), action, name.bold());

    let type_display = if mode.is_reference() {
        "reference".to_string()
    } else if is_rust {
        "rust".to_string()
    } else {
        mode.server_type()
            .map(|t| t.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    let transport_display = if mode.is_http() { "http" } else { "stdio" };

    println!("    {}       {}", "Type".dimmed(), type_display);
    println!("    {}  {}", "Transport".dimmed(), transport_display);

    if !mode.is_reference() {
        if is_rust {
            println!("    {}      target/release/{}", "Entry".dimmed(), name);
        } else {
            match mode.server_type() {
                Some(McpbServerType::Node) => {
                    println!("    {}      server/index.js", "Entry".dimmed());
                }
                Some(McpbServerType::Python) => {
                    println!("    {}      server/main.py", "Entry".dimmed());
                }
                Some(McpbServerType::Binary) | None => {}
            }
        }
    }
    println!("    {}    0.1.0\n", "Version".dimmed());

    // Tree structure
    let prefix = match dir_path {
        Some(p) => format!("{}/", p),
        None => "./".to_string(),
    };

    println!("    {}", prefix.bold());

    if mode.is_reference() {
        println!("    ├── manifest.json");
        println!("    ├── README.md");
        println!("    └── .mcpbignore");
    } else if is_rust {
        println!("    ├── manifest.json");
        println!("    ├── README.md");
        println!("    ├── Cargo.toml");
        println!("    ├── .gitignore");
        println!("    ├── .mcpbignore");
        println!("    └── src/");
        println!("        ├── main.rs");
        println!("        └── lib.rs");
    } else {
        match mode.server_type() {
            Some(McpbServerType::Node) => {
                println!("    ├── manifest.json");
                println!("    ├── README.md");
                println!("    ├── package.json");
                println!("    ├── .gitignore");
                println!("    ├── .mcpbignore");
                println!("    └── server/");
                println!("        └── index.js");
            }
            Some(McpbServerType::Python) => {
                let project_file = match mode.python_package_manager() {
                    Some(PythonPackageManager::Pip) => "requirements.txt",
                    _ => "pyproject.toml",
                };
                println!("    ├── manifest.json");
                println!("    ├── README.md");
                println!("    ├── {}", project_file);
                println!("    ├── .gitignore");
                println!("    ├── .mcpbignore");
                println!("    └── server/");
                println!("        └── main.py");
            }
            Some(McpbServerType::Binary) | None => {
                println!("    ├── manifest.json");
                println!("    ├── README.md");
                println!("    └── .mcpbignore");
            }
        }
    }

    // Next steps
    println!("\n  {}:", "Next Steps".bold());

    let mut step = 1;

    if let Some(p) = dir_path {
        println!("    {}. cd {}", step, p);
        step += 1;
    }

    if mode.is_reference() {
        if mode.is_http() {
            println!(
                "    {}. {}",
                step,
                "# Set url and credentials in manifest.json".dimmed()
            );
        } else {
            println!(
                "    {}. {}",
                step,
                "# Set command path in manifest.json".dimmed()
            );
        }
        println!(
            "    {}. tool info               {}",
            step + 1,
            "# verify connection".dimmed()
        );
    } else if is_rust {
        println!(
            "    {}. tool build              {}",
            step,
            "# build binary".dimmed()
        );
        println!(
            "    {}. tool info               {}",
            step + 1,
            "# list tools".dimmed()
        );
        println!(
            "    {}. tool call -m hello      {}",
            step + 2,
            "# test a tool".dimmed()
        );
        println!(
            "    {}. tool pack               {}",
            step + 3,
            "# create .mcpb bundle".dimmed()
        );
    } else {
        println!(
            "    {}. tool build              {}",
            step,
            "# install dependencies".dimmed()
        );
        println!(
            "    {}. tool info               {}",
            step + 1,
            "# list tools".dimmed()
        );
        println!(
            "    {}. tool call -m hello      {}",
            step + 2,
            "# test a tool".dimmed()
        );
        println!(
            "    {}. tool pack               {}",
            step + 3,
            "# create .mcpb bundle".dimmed()
        );
    }
}

/// Validate a tool manifest.
pub async fn validate_mcpb(
    path: Option<String>,
    strict: bool,
    json_output: bool,
    quiet: bool,
) -> ToolResult<()> {
    let dir = path
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let result = validate_manifest(&dir);
    let format_name = "manifest.json";

    if json_output {
        output_json(&result, format_name)?;
        return check_exit_status(&result, strict);
    }

    if quiet {
        output_quiet(&result);
    } else {
        output_full(&result, strict, format_name);
    }

    check_exit_status(&result, strict)
}

/// Output validation result as JSON.
fn output_json(result: &ValidationResult, format_name: &str) -> ToolResult<()> {
    let output = serde_json::json!({
        "format": format_name,
        "valid": result.is_valid(),
        "strict_valid": result.is_strict_valid(),
        "errors": result.errors.iter().map(|e| {
            serde_json::json!({
                "code": e.code,
                "message": e.message,
                "location": e.location,
                "details": e.details,
                "help": e.help,
            })
        }).collect::<Vec<_>>(),
        "warnings": result.warnings.iter().map(|w| {
            serde_json::json!({
                "code": w.code,
                "message": w.message,
                "location": w.location,
                "details": w.details,
                "help": w.help,
            })
        }).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Output validation result in quiet mode.
fn output_quiet(result: &ValidationResult) {
    for error in &result.errors {
        println!(
            "  {}: {}: {}",
            format!("error[{}]", error.code).bright_red(),
            error.message,
            error.details
        );
    }
}

/// Output validation result in full format.
fn output_full(result: &ValidationResult, strict: bool, format_name: &str) {
    println!("  Validating {}\n", format_name.bold());

    let all_issues: Vec<_> = if strict {
        result
            .errors
            .iter()
            .map(|e| ("error", e))
            .chain(result.warnings.iter().map(|w| ("error", w)))
            .collect()
    } else {
        result
            .errors
            .iter()
            .map(|e| ("error", e))
            .chain(result.warnings.iter().map(|w| ("warning", w)))
            .collect()
    };

    for (severity, issue) in &all_issues {
        let label = if *severity == "error" {
            format!("error[{}]", issue.code).bright_red().bold()
        } else {
            format!("warning[{}]", issue.code).bright_yellow().bold()
        };
        println!("  {}: → {}", label, issue.location.bold());

        if let Some(help) = &issue.help {
            println!("      {} {}", "├─".dimmed(), issue.details.dimmed());
            println!(
                "      {} {}: {}",
                "└─".dimmed(),
                "help".bright_green().dimmed(),
                help.dimmed()
            );
        } else {
            println!("      {} {}", "└─".dimmed(), issue.details.dimmed());
        }

        println!();
    }

    // Summary line
    let error_count = result.errors.len();
    let warning_count = result.warnings.len();

    if strict {
        let total = error_count + warning_count;
        if total > 0 {
            println!(
                "  {} {} (strict mode)",
                "✗".bright_red(),
                if total == 1 {
                    "1 error".to_string()
                } else {
                    format!("{} errors", total)
                }
            );
        } else {
            println!("  {} valid", "✓".bright_green());
        }
    } else if error_count > 0 {
        let summary = if warning_count > 0 {
            format!(
                "{} {}, {} {}",
                error_count,
                if error_count == 1 { "error" } else { "errors" },
                warning_count,
                if warning_count == 1 {
                    "warning"
                } else {
                    "warnings"
                }
            )
        } else if error_count == 1 {
            "1 error".to_string()
        } else {
            format!("{} errors", error_count)
        };
        println!("  {} {}", "✗".bright_red(), summary);
    } else if warning_count > 0 {
        println!(
            "  {} valid ({} {})",
            "✓".bright_green(),
            warning_count,
            if warning_count == 1 {
                "warning"
            } else {
                "warnings"
            }
        );
    } else {
        println!("  {} valid", "✓".bright_green());
    }
}

/// Check if we should exit with error status.
fn check_exit_status(result: &ValidationResult, strict: bool) -> ToolResult<()> {
    if strict {
        if !result.is_strict_valid() {
            std::process::exit(1);
        }
    } else if !result.is_valid() {
        std::process::exit(1);
    }
    Ok(())
}

/// Pack a tool into an .mcpb bundle.
pub async fn pack_mcpb(
    path: Option<String>,
    output: Option<String>,
    no_validate: bool,
    include_dotfiles: bool,
    verbose: bool,
) -> ToolResult<()> {
    let dir = path
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let options = PackOptions {
        output: output.map(PathBuf::from),
        validate: !no_validate,
        include_dotfiles,
        verbose,
    };

    match pack_bundle(&dir, &options) {
        Ok(result) => {
            if !no_validate {
                println!("  {} Validation passed", "✓".bright_green());
            }

            if verbose {
                for ignored in &result.ignored_files {
                    println!(
                        "    {} {} {}",
                        "-".dimmed(),
                        ignored.dimmed(),
                        "(ignored)".dimmed()
                    );
                }
            }

            println!(
                "  {} Created {} ({})",
                "✓".bright_green(),
                result.output_path.display(),
                format_size(result.compressed_size)
            );
            println!(
                "    Files: {}, Compressed: {} (from {})",
                result.file_count,
                format_size(result.compressed_size),
                format_size(result.total_size)
            );
        }
        Err(PackError::ValidationFailed(validation)) => {
            println!("  {} Validation failed\n", "✗".bright_red());

            for error in &validation.errors {
                println!(
                    "  {}: → {}",
                    format!("error[{}]", error.code).bright_red().bold(),
                    error.location.bold()
                );
                if let Some(help) = &error.help {
                    println!("      {} {}", "├─".dimmed(), error.details.dimmed());
                    println!(
                        "      {} {}: {}",
                        "└─".dimmed(),
                        "help".bright_green().dimmed(),
                        help.dimmed()
                    );
                } else {
                    println!("      {} {}", "└─".dimmed(), error.details.dimmed());
                }
                println!();
            }

            let error_count = validation.errors.len();
            println!(
                "  {} {}",
                "✗".bright_red(),
                if error_count == 1 {
                    "1 error".to_string()
                } else {
                    format!("{} errors", error_count)
                }
            );
            println!("\n  Cannot pack invalid manifest. Fix errors and retry.");
            std::process::exit(1);
        }
        Err(PackError::ManifestNotFound(path)) => {
            println!(
                "  {}: manifest.json not found in {}",
                "error".bright_red().bold(),
                path.display()
            );
            println!("  Run `tool init` to create one.");
            std::process::exit(1);
        }
        Err(e) => {
            return Err(ToolError::Generic(format!("Pack failed: {}", e)));
        }
    }

    Ok(())
}

/// Run a script from manifest.json `_meta.company.superrad.radical.scripts`
pub async fn run_script(
    script_name: &str,
    path: Option<String>,
    extra_args: Vec<String>,
) -> ToolResult<()> {
    let target_dir = resolve_target_dir(&path)?;

    // Load manifest.json
    let manifest_path = target_dir.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(ToolError::Generic(format!(
            "No manifest.json found in {}\nRun `tool init` to create one.",
            target_dir.display()
        )));
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ToolError::Generic(format!("Invalid JSON: {}", e)))?;

    // Extract script from _meta.company.superrad.radical.scripts
    let script_cmd = manifest
        .get("_meta")
        .and_then(|m| m.get("company.superrad.radical"))
        .and_then(|r| r.get("scripts"))
        .and_then(|s| s.get(script_name))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolError::Generic(format!(
                "Script '{}' not found in manifest.json\nDefine it in _meta.company.superrad.radical.scripts\nUse `tool run --list` to see available scripts.",
                script_name
            ))
        })?;

    // Substitute ${__dirname} with target directory
    let dirname = target_dir.to_string_lossy();
    let script_cmd = script_cmd.replace("${__dirname}", &dirname);

    // Build full command with extra args
    let full_cmd = if extra_args.is_empty() {
        script_cmd
    } else {
        format!("{} {}", script_cmd, extra_args.join(" "))
    };

    println!("  {} {}", "Running:".bright_cyan(), full_cmd.bright_white());

    // Execute via shell
    let status = Command::new("sh")
        .arg("-c")
        .arg(&full_cmd)
        .current_dir(&target_dir)
        .status()?;

    if !status.success() {
        return Err(ToolError::Generic(format!(
            "Script '{}' failed with exit code: {}",
            script_name,
            status.code().unwrap_or(-1)
        )));
    }

    Ok(())
}

/// List available scripts from manifest.json
pub async fn list_scripts(path: Option<String>) -> ToolResult<()> {
    let target_dir = resolve_target_dir(&path)?;
    let manifest_path = target_dir.join(MCPB_MANIFEST_FILE);

    if !manifest_path.exists() {
        return Err(ToolError::Generic(format!(
            "No manifest.json found in {}",
            target_dir.display()
        )));
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| ToolError::Generic(format!("Invalid JSON: {}", e)))?;

    let scripts = manifest
        .get("_meta")
        .and_then(|m| m.get("company.superrad.radical"))
        .and_then(|r| r.get("scripts"))
        .and_then(|s| s.as_object());

    match scripts {
        Some(scripts) if !scripts.is_empty() => {
            println!("  {}", "Available scripts:".bright_cyan().bold());
            for (name, cmd) in scripts {
                if let Some(cmd_str) = cmd.as_str() {
                    println!("    {} {}", name.bright_white(), cmd_str.bright_black());
                }
            }
        }
        _ => {
            println!("  {}", "No scripts defined in manifest.json".yellow());
            println!("  Add scripts to _meta.company.superrad.radical.scripts");
        }
    }

    Ok(())
}

/// Run a script from external subcommand (e.g., `tool build ./path -- extra args`)
pub async fn run_external_script(args: Vec<std::ffi::OsString>) -> ToolResult<()> {
    if args.is_empty() {
        return Err(ToolError::Generic("No script name provided".into()));
    }

    // First arg is the script name
    let script_name = args[0].to_string_lossy().to_string();

    // Parse remaining args: [path] [-- extra_args...]
    let remaining: Vec<String> = args[1..]
        .iter()
        .map(|s| s.to_string_lossy().into())
        .collect();

    // Find "--" separator if present
    let separator_pos = remaining.iter().position(|s| s == "--");

    let (path, extra_args) = match separator_pos {
        Some(pos) => {
            let path = if pos > 0 {
                Some(remaining[0].clone())
            } else {
                None
            };
            let extra = remaining[pos + 1..].to_vec();
            (path, extra)
        }
        None => {
            // No separator - first arg (if any) is path, no extra args
            let path = remaining.first().cloned();
            (path, vec![])
        }
    };

    run_script(&script_name, path, extra_args).await
}

/// Helper to resolve target directory from optional path
fn resolve_target_dir(path: &Option<String>) -> ToolResult<PathBuf> {
    match path {
        Some(p) => {
            let target = PathBuf::from(p);
            Ok(if target.is_absolute() {
                target
            } else {
                std::env::current_dir()?.join(&target)
            })
        }
        None => Ok(std::env::current_dir()?),
    }
}

/// Format byte size.
fn format_size(bytes: u64) -> String {
    if bytes < 1_000 {
        format!("{} B", bytes)
    } else if bytes < 1_000_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}

/// Get info about a tool (list tools, prompts, resources).
#[allow(clippy::too_many_arguments)]
pub async fn tool_info(
    tool: String,
    show_tools: bool,
    show_prompts: bool,
    show_resources: bool,
    show_all: bool,
    json_output: bool,
    config: Vec<String>,
    config_file: Option<String>,
    verbose: bool,
) -> ToolResult<()> {
    // Parse user config from -c flags and config file
    let mut user_config = parse_user_config(&config, config_file.as_deref())?;

    // Resolve tool path
    let tool_path = resolve_tool_path(&tool).await?;

    // Load manifest to get user_config schema
    let resolved_plugin = load_tool_from_path(&tool_path)?;
    let manifest_schema = resolved_plugin.template.user_config.as_ref();

    // Prompt for missing required config values, then apply defaults
    prompt_missing_user_config(manifest_schema, &mut user_config)?;
    apply_user_config_defaults(manifest_schema, &mut user_config);

    // Get tool info - handle EntryPointNotFound specially
    let (capabilities, tool_type, manifest_path) =
        match get_tool_info_from_path(&tool_path, &user_config, verbose).await {
            Ok(result) => result,
            Err(ToolError::EntryPointNotFound {
                entry_point,
                full_path: _,
                build_script,
                bundle_path,
            }) => {
                println!(
                    "  {} Entry point not found: {}\n",
                    "✗".bright_red(),
                    entry_point.bright_white()
                );
                if let Some(build_cmd) = build_script {
                    println!("    The tool needs to be built before it can be run.\n");
                    println!("    {}:", "To build".dimmed());
                    println!("      cd {} && tool build\n", bundle_path);
                    println!("    {}: {}", "Runs".dimmed(), build_cmd.dimmed());
                } else {
                    println!("    {}:", "If this tool requires building".dimmed());
                    println!("      Add a build script to manifest.json:\n");
                    println!("      {}", "\"_meta\": {".dimmed());
                    println!("        {}", "\"company.superrad.radical\": {".dimmed());
                    println!(
                        "          {}",
                        "\"scripts\": { \"build\": \"...\" }".dimmed()
                    );
                    println!("        {}", "}".dimmed());
                    println!("      {}", "}".dimmed());
                }
                std::process::exit(1);
            }
            Err(ToolError::OAuthNotConfigured) | Err(ToolError::AuthRequired { tool_ref: _ }) => {
                println!(
                    "  {} This tool requires OAuth authentication\n",
                    "✗".bright_red()
                );
                println!(
                    "    To enable OAuth, set the {} environment variable:\n",
                    "CREDENTIALS_SECRET_KEY".bright_cyan()
                );
                println!("    {}  Generate a key:", "1.".dimmed());
                println!("       {}\n", "openssl rand -base64 32".bright_white());
                println!("    {}  Set it in your shell:", "2.".dimmed());
                println!(
                    "       {}\n",
                    "export CREDENTIALS_SECRET_KEY=\"<your-key>\"".bright_white()
                );
                println!(
                    "    {}  Re-run this command to start OAuth flow",
                    "3.".dimmed()
                );
                std::process::exit(1);
            }
            Err(e) => return Err(e),
        };

    if json_output {
        output_tool_info_json(&capabilities, tool_type, &manifest_path)?;
        return Ok(());
    }

    // Determine what to show
    let show_all = show_all || (!show_tools && !show_prompts && !show_resources);

    // Header - matching rad tool format
    println!(
        "  {} Connected to {} v{}\n",
        "✓".bright_green(),
        capabilities.server_info.name.bold(),
        capabilities.server_info.version
    );

    // Show server metadata
    println!("    {}       {}", "Type".dimmed(), tool_type);
    println!(
        "    {}   {}",
        "Location".dimmed(),
        manifest_path.display().to_string().dimmed()
    );
    println!();

    // Tools section
    if (show_all || show_tools) && !capabilities.tools.is_empty() {
        println!("    {}:", "Tools".dimmed());
        for (idx, tool) in capabilities.tools.iter().enumerate() {
            let desc = tool
                .description
                .as_ref()
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("      {}{}", tool.name.bright_cyan(), desc);

            let has_input = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
                .is_some_and(|p| !p.is_empty());
            let has_output = tool.output_schema.is_some();

            // Show input parameters with tree structure
            if has_input {
                let schema = &tool.input_schema;
                let props = schema
                    .get("properties")
                    .and_then(|p| p.as_object())
                    .unwrap();
                let required: Vec<&str> = schema
                    .get("required")
                    .and_then(|r| r.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                let input_branch = if has_output { "├──" } else { "└──" };
                println!("      {} {}", input_branch.dimmed(), "Input".dimmed());

                let prop_count = props.len();
                for (i, (name, prop)) in props.iter().enumerate() {
                    let is_last = i == prop_count - 1;
                    let prefix = if has_output { "│" } else { " " };
                    let branch = if is_last { "└──" } else { "├──" };
                    let type_str = prop.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                    let req_marker = if required.contains(&name.as_str()) {
                        "*"
                    } else {
                        ""
                    };
                    let param_desc = prop
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("");

                    let param_name = format!("{}{}", name, req_marker);
                    println!(
                        "      {}   {} {:<20} {:<10} {}",
                        prefix.dimmed(),
                        branch.dimmed(),
                        param_name,
                        type_str.dimmed(),
                        param_desc.dimmed()
                    );
                }
            }

            // Show output schema with tree structure
            if let Some(output_schema) = &tool.output_schema {
                println!("      {} {}", "└──".dimmed(), "Output".dimmed());

                if let Some(props) = output_schema.get("properties").and_then(|p| p.as_object()) {
                    let required: Vec<&str> = output_schema
                        .get("required")
                        .and_then(|r| r.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();

                    let prop_count = props.len();
                    for (i, (name, prop)) in props.iter().enumerate() {
                        let is_last = i == prop_count - 1;
                        let branch = if is_last { "└──" } else { "├──" };
                        let type_str = prop.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                        let req_marker = if required.contains(&name.as_str()) {
                            "*"
                        } else {
                            ""
                        };
                        let param_desc = prop
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");

                        let param_name = format!("{}{}", name, req_marker);
                        println!(
                            "          {} {:<20} {:<10} {}",
                            branch.dimmed(),
                            param_name,
                            type_str.dimmed(),
                            param_desc.dimmed()
                        );
                    }
                }
            }

            // Add spacing between tools
            if idx < capabilities.tools.len() - 1 {
                println!();
            }
        }
        println!();
    }

    // Prompts section
    if (show_all || show_prompts) && !capabilities.prompts.is_empty() {
        println!("    {}:", "Prompts".dimmed());
        for (idx, prompt) in capabilities.prompts.iter().enumerate() {
            let desc = prompt
                .description
                .as_ref()
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("      {}{}", prompt.name.to_string().bright_magenta(), desc);

            // Show arguments if available
            if let Some(args) = &prompt.arguments
                && !args.is_empty()
            {
                for (i, arg) in args.iter().enumerate() {
                    let is_last = i == args.len() - 1;
                    let req_marker = if arg.required.unwrap_or(false) {
                        "*"
                    } else {
                        ""
                    };
                    let arg_name = format!("{}{}", arg.name, req_marker);
                    let arg_desc = arg.description.as_deref().unwrap_or("");
                    let branch = if is_last { "└──" } else { "├──" };
                    println!(
                        "      {} {:<20} {:<10} {}",
                        branch.dimmed(),
                        arg_name.bright_white(),
                        "string".dimmed(),
                        arg_desc.dimmed()
                    );
                }
            }

            if idx < capabilities.prompts.len() - 1 {
                println!();
            }
        }
        println!();
    }

    // Resources section
    if (show_all || show_resources) && !capabilities.resources.is_empty() {
        println!("    {}:", "Resources".dimmed());
        for (idx, resource) in capabilities.resources.iter().enumerate() {
            let desc = resource
                .description
                .as_ref()
                .map(|d| format!("  {}", d.dimmed()))
                .unwrap_or_default();
            println!("      {}{}", resource.uri.to_string().bright_yellow(), desc);

            // Show resource details
            let has_name = !resource.name.is_empty();
            let has_mime = resource.mime_type.is_some();

            if has_name {
                let branch = if has_mime { "├──" } else { "└──" };
                println!(
                    "      {} {:<12} {}",
                    branch.dimmed(),
                    "name".dimmed(),
                    resource.name
                );
            }

            if let Some(mime) = &resource.mime_type {
                println!("      {} {:<12} {}", "└──".dimmed(), "mime".dimmed(), mime);
            }

            if idx < capabilities.resources.len() - 1 {
                println!();
            }
        }
        println!();
    }

    Ok(())
}

/// Output tool info as JSON.
fn output_tool_info_json(
    capabilities: &crate::mcp::ToolCapabilities,
    tool_type: ToolType,
    manifest_path: &Path,
) -> ToolResult<()> {
    let output = serde_json::json!({
        "server": {
            "name": capabilities.server_info.name,
            "version": capabilities.server_info.version,
        },
        "type": tool_type.to_string(),
        "manifest_path": manifest_path.display().to_string(),
        "tools": capabilities.tools.iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.input_schema,
            })
        }).collect::<Vec<_>>(),
        "prompts": capabilities.prompts.iter().map(|p| {
            serde_json::json!({
                "name": p.name,
                "description": p.description,
            })
        }).collect::<Vec<_>>(),
        "resources": capabilities.resources.iter().map(|r| {
            serde_json::json!({
                "name": r.name,
                "description": r.description,
                "uri": r.uri,
            })
        }).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// Call a tool method.
pub async fn tool_call(
    tool: String,
    method: String,
    params: Vec<String>,
    config: Vec<String>,
    config_file: Option<String>,
    verbose: bool,
) -> ToolResult<()> {
    // Parse user config from -c flags and config file
    let mut user_config = parse_user_config(&config, config_file.as_deref())?;

    // Parse method parameters
    let arguments = parse_method_params(&params)?;

    // Resolve tool path
    let tool_path = resolve_tool_path(&tool).await?;

    // Load manifest to get user_config schema
    let resolved_plugin = load_tool_from_path(&tool_path)?;
    let manifest_schema = resolved_plugin.template.user_config.as_ref();

    // Prompt for missing required config values, then apply defaults
    prompt_missing_user_config(manifest_schema, &mut user_config)?;
    apply_user_config_defaults(manifest_schema, &mut user_config);

    // Get tool name for display
    let tool_name = tool_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&tool);

    // Call the tool - handle EntryPointNotFound specially
    let result =
        match call_tool_from_path(&tool_path, &method, arguments, &user_config, verbose).await {
            Ok(result) => result,
            Err(ToolError::EntryPointNotFound {
                entry_point,
                full_path: _,
                build_script,
                bundle_path,
            }) => {
                println!(
                    "  {} Entry point not found: {}\n",
                    "✗".bright_red(),
                    entry_point.bright_white()
                );
                if let Some(build_cmd) = build_script {
                    println!("    The tool needs to be built before it can be run.\n");
                    println!("    {}:", "To build".dimmed());
                    println!("      cd {} && tool build\n", bundle_path);
                    println!("    {}: {}", "Runs".dimmed(), build_cmd.dimmed());
                } else {
                    println!("    {}:", "If this tool requires building".dimmed());
                    println!("      Add a build script to manifest.json:\n");
                    println!("      {}", "\"_meta\": {".dimmed());
                    println!("        {}", "\"company.superrad.radical\": {".dimmed());
                    println!(
                        "          {}",
                        "\"scripts\": { \"build\": \"...\" }".dimmed()
                    );
                    println!("        {}", "}".dimmed());
                    println!("      {}", "}".dimmed());
                }
                std::process::exit(1);
            }
            Err(ToolError::OAuthNotConfigured) | Err(ToolError::AuthRequired { tool_ref: _ }) => {
                println!(
                    "  {} This tool requires OAuth authentication\n",
                    "✗".bright_red()
                );
                println!(
                    "    To enable OAuth, set the {} environment variable:\n",
                    "CREDENTIALS_SECRET_KEY".bright_cyan()
                );
                println!("    {}  Generate a key:", "1.".dimmed());
                println!("       {}\n", "openssl rand -base64 32".bright_white());
                println!("    {}  Set it in your shell:", "2.".dimmed());
                println!(
                    "       {}\n",
                    "export CREDENTIALS_SECRET_KEY=\"<your-key>\"".bright_white()
                );
                println!(
                    "    {}  Re-run this command to start OAuth flow",
                    "3.".dimmed()
                );
                std::process::exit(1);
            }
            Err(e) => return Err(e),
        };

    let is_error = result.result.is_error.unwrap_or(false);

    // Print header matching rad tool format
    if is_error {
        println!(
            "  {} {} {} on {}",
            "✗".bright_red(),
            "Error calling".bright_red(),
            method.bold(),
            tool_name.bold()
        );
    } else {
        println!(
            "  {} Called {} on {}\n",
            "✓".bright_green(),
            method.bold(),
            tool_name.bold()
        );
    }

    // Output the result content
    for content in &result.result.content {
        // Content is wrapped in Annotated, so we dereference to get the inner RawContent
        match &**content {
            rmcp::model::RawContent::Text(text) => {
                // Try to parse as JSON for pretty printing
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text.text) {
                    let pretty = serde_json::to_string_pretty(&json).unwrap_or(text.text.clone());
                    for line in pretty.lines() {
                        if is_error {
                            println!("    {}", line.bright_red());
                        } else {
                            println!("    {}", line);
                        }
                    }
                } else {
                    // Plain text output
                    for line in text.text.lines() {
                        if is_error {
                            println!("    {}", line.bright_red());
                        } else {
                            println!("    {}", line);
                        }
                    }
                }
            }
            rmcp::model::RawContent::Image(img) => {
                println!("    [Image: {} bytes]", img.data.len());
            }
            rmcp::model::RawContent::Audio(audio) => {
                println!("    [Audio: {} bytes]", audio.data.len());
            }
            rmcp::model::RawContent::Resource(res) => {
                println!("    [Resource: {:?}]", res.resource);
            }
            rmcp::model::RawContent::ResourceLink(link) => {
                println!("    [ResourceLink: {}]", link.uri);
            }
        }
    }

    if is_error {
        std::process::exit(1);
    }

    Ok(())
}

/// Parse user config from -c flags and config file.
fn parse_user_config(
    config_flags: &[String],
    config_file: Option<&str>,
) -> ToolResult<BTreeMap<String, String>> {
    let mut config = BTreeMap::new();

    // Load from config file first
    if let Some(file_path) = config_file {
        let content = std::fs::read_to_string(file_path)?;
        let file_config: BTreeMap<String, String> = serde_json::from_str(&content)
            .or_else(|_| toml::from_str(&content))
            .map_err(|e| ToolError::Generic(format!("Failed to parse config file: {}", e)))?;
        config.extend(file_config);
    }

    // Parse -c flags (key=value format)
    for flag in config_flags {
        if let Some((key, value)) = flag.split_once('=') {
            config.insert(key.to_string(), value.to_string());
        } else {
            return Err(ToolError::Generic(format!(
                "Invalid config format '{}'. Expected key=value",
                flag
            )));
        }
    }

    Ok(config)
}

/// Apply default values from user_config schema.
///
/// For any field in the schema that has a `default` value and isn't already
/// provided in user_config, applies the default. This ensures variable
/// substitution works even when users don't explicitly provide values.
fn apply_user_config_defaults(
    schema: Option<&BTreeMap<String, McpbUserConfigField>>,
    user_config: &mut BTreeMap<String, String>,
) {
    let Some(schema) = schema else {
        return;
    };

    for (key, field) in schema {
        // Skip if already provided
        if user_config.contains_key(key) {
            continue;
        }

        // Apply default if present
        if let Some(default) = &field.default {
            let value = match default {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => default.to_string(),
            };
            user_config.insert(key.clone(), value);
        }
    }
}

/// Prompt for user_config values interactively.
///
/// Prompts for all config fields except those that have defaults and aren't required
/// (those are auto-applied by `apply_user_config_defaults`).
fn prompt_missing_user_config(
    schema: Option<&BTreeMap<String, McpbUserConfigField>>,
    user_config: &mut BTreeMap<String, String>,
) -> ToolResult<()> {
    use std::io::IsTerminal;

    let Some(schema) = schema else {
        return Ok(());
    };

    // Find fields that need prompting:
    // - Already provided via --config: skip
    // - Has default AND not required: skip (auto-applied later)
    // - Otherwise: prompt
    let to_prompt: Vec<(&String, &McpbUserConfigField)> = schema
        .iter()
        .filter(|(key, field)| {
            // Skip if already provided
            if user_config.contains_key(*key) {
                return false;
            }

            let is_required = field.required.unwrap_or(false);
            let has_default = field.default.is_some();

            // Skip if has default and not required (will be auto-applied)
            if has_default && !is_required {
                return false;
            }

            // Prompt for: required fields OR fields without defaults
            true
        })
        .collect();

    if to_prompt.is_empty() {
        return Ok(());
    }

    // Check if we have a TTY for interactive prompting
    if !std::io::stdin().is_terminal() {
        // Non-interactive: only error for required fields without defaults
        let required_missing: Vec<String> = to_prompt
            .iter()
            .filter(|(_, field)| {
                let is_required = field.required.unwrap_or(false);
                let has_default = field.default.is_some();
                is_required && !has_default
            })
            .map(|(key, field)| {
                let desc = field.description.as_deref().unwrap_or("");
                if desc.is_empty() {
                    format!("  --config {}=<value>", key)
                } else {
                    format!("  --config {}=<value>  ({})", key, desc)
                }
            })
            .collect();

        if !required_missing.is_empty() {
            return Err(ToolError::Generic(format!(
                "Missing required configuration:\n\n{}\n\nProvide via --config flags or run interactively.",
                required_missing.join("\n")
            )));
        }
        return Ok(());
    }

    // Interactive: prompt for each field
    cliclack::intro("Tool configuration")?;

    for (key, field) in to_prompt {
        let is_required = field.required.unwrap_or(false);

        // Get description
        let description = field.description.as_deref().unwrap_or("");

        // Default can be number, string, or bool - convert to string
        let default_value = field.default.as_ref().map(|d| match d {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => d.to_string(),
        });

        // Build prompt text
        let prompt_text = if description.is_empty() {
            key.clone()
        } else {
            format!("{} ({})", key, description)
        };

        // Get user input using cliclack
        let value: String = match default_value {
            Some(default) => cliclack::input(&prompt_text)
                .default_input(&default)
                .interact()?,
            None => cliclack::input(&prompt_text)
                .required(is_required)
                .interact()?,
        };

        // Only insert non-empty values (skip optional fields left blank)
        if !value.is_empty() {
            user_config.insert(key.clone(), value);
        }
    }

    cliclack::outro("Configuration complete!")?;

    Ok(())
}

/// Parse method parameters from command line.
fn parse_method_params(params: &[String]) -> ToolResult<BTreeMap<String, serde_json::Value>> {
    let mut result = BTreeMap::new();

    for param in params {
        if let Some((key, value)) = param.split_once('=') {
            // Try to parse as JSON, otherwise treat as string
            let json_value = serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
            result.insert(key.to_string(), json_value);
        } else {
            return Err(ToolError::Generic(format!(
                "Invalid parameter format '{}'. Expected key=value",
                param
            )));
        }
    }

    Ok(result)
}

/// Resolve a tool reference to a path.
async fn resolve_tool_path(tool: &str) -> ToolResult<PathBuf> {
    // Check if it's a local path
    let path = PathBuf::from(tool);
    if path.exists() || tool == "." || tool.starts_with("./") || tool.starts_with("/") {
        let abs_path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()?.join(&path)
        };
        return Ok(abs_path);
    }

    // Try to resolve from installed tools
    let resolver = FilePluginResolver::default();
    if let Some(resolved) = resolver.resolve_tool(tool).await? {
        // Get the directory containing the manifest
        let dir = resolved.path.parent().unwrap_or(&resolved.path);
        return Ok(dir.to_path_buf());
    }

    Err(ToolError::Generic(format!(
        "Tool '{}' not found. Use a path or install it first.",
        tool
    )))
}

/// List all installed tools.
pub async fn list_tools(filter: Option<&str>, json_output: bool) -> ToolResult<()> {
    let resolver = FilePluginResolver::default();
    let tools = resolver.list_tools().await?;

    // Filter if specified
    let filtered: Vec<_> = if let Some(f) = filter {
        let pattern_lower = f.to_lowercase();
        tools
            .iter()
            .filter(|t| t.to_string().to_lowercase().contains(&pattern_lower))
            .collect()
    } else {
        tools.iter().collect()
    };

    if filtered.is_empty() {
        if let Some(pattern) = filter {
            println!(
                "  {} No tools found matching: {}",
                "✗".bright_red(),
                pattern.bright_white().bold()
            );
        } else {
            println!("  {} No tools installed", "✗".bright_red());
            println!("\n    {}", "Searched:".dimmed());
            if let Ok(cwd) = std::env::current_dir() {
                println!("      {}", cwd.join("tools").display().to_string().dimmed());
            }
            if let Some(home) = dirs::home_dir() {
                println!(
                    "      {}",
                    home.join(".tool/tools").display().to_string().dimmed()
                );
            }
        }
        return Ok(());
    }

    // Collect tool info for each ref
    let mut tool_entries: Vec<ToolListEntry> = Vec::new();

    for plugin_ref in &filtered {
        let entry = match resolver.resolve_tool(&plugin_ref.to_string()).await {
            Ok(Some(resolved)) => {
                let description = resolved
                    .template
                    .description
                    .clone()
                    .or_else(|| resolved.template.display_name.clone());
                let transport = resolved.template.server.transport.to_string();

                ToolListEntry {
                    name: plugin_ref.to_string(),
                    tool_type: transport,
                    description,
                    path: resolved
                        .path
                        .parent()
                        .unwrap_or(&resolved.path)
                        .to_path_buf(),
                }
            }
            _ => ToolListEntry {
                name: plugin_ref.to_string(),
                tool_type: "unknown".to_string(),
                description: None,
                path: PathBuf::new(),
            },
        };
        tool_entries.push(entry);
    }

    // JSON output
    if json_output {
        let output: Vec<_> = tool_entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "name": e.name,
                    "type": e.tool_type,
                    "description": e.description,
                    "location": e.path.display().to_string(),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&output).expect("Failed to serialize JSON output")
        );
        return Ok(());
    }

    // Human-readable output
    let count = tool_entries.len();
    let label = if count == 1 { "tool" } else { "tools" };
    println!(
        "  {} Found {} {}\n",
        "✓".bright_green(),
        count.to_string().bold(),
        label
    );

    for entry in &tool_entries {
        let desc = entry
            .description
            .as_ref()
            .map(|d| format!("  {}", d.dimmed()))
            .unwrap_or_default();
        println!("    {}{}", entry.name.bright_cyan(), desc);
        println!(
            "    └── {}  {}",
            entry.tool_type.dimmed(),
            entry.path.display().to_string().dimmed()
        );
        println!();
    }

    Ok(())
}

/// Download a tool from the registry.
pub async fn download_tool(name: &str, output: Option<&str>) -> ToolResult<()> {
    use crate::references::PluginRef;
    use crate::registry::RegistryClient;

    // Parse tool reference
    let plugin_ref = name
        .parse::<PluginRef>()
        .map_err(|e| ToolError::Generic(format!("Invalid tool reference '{}': {}", name, e)))?;

    if plugin_ref.namespace().is_none() {
        println!(
            "  {} Tool reference must include a namespace for registry fetch",
            "✗".bright_red()
        );
        println!(
            "    Example: {} or {}",
            "namespace/tool".bright_cyan(),
            "namespace/tool@1.0.0".bright_cyan()
        );
        return Ok(());
    }

    let namespace = plugin_ref.namespace().unwrap();
    let tool_name = plugin_ref.name();

    // Create registry client
    let client = RegistryClient::new();

    // Determine version - use specified or fetch latest
    let version = if let Some(v) = plugin_ref.version_str() {
        v.to_string()
    } else {
        println!(
            "  {} Resolving {}...",
            "→".bright_blue(),
            plugin_ref.to_string().bright_cyan()
        );

        let artifact = client.get_artifact(namespace, tool_name).await?;
        artifact.latest_version.map(|v| v.version).ok_or_else(|| {
            ToolError::Generic(format!(
                "No versions published for {}/{}",
                namespace, tool_name
            ))
        })?
    };

    // Determine output path
    let bundle_name = format!("{}@{}.mcpb", tool_name, version);
    let output_path = match output {
        Some(p) => {
            let path = PathBuf::from(p);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            }
        }
        None => std::env::current_dir()?.join(&bundle_name),
    };

    // Create parent directory if it doesn't exist
    if let Some(parent) = output_path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }

    let download_ref = format!("{}/{}@{}", namespace, tool_name, version);
    println!(
        "  {} Downloading {}...",
        "→".bright_blue(),
        download_ref.bright_cyan()
    );

    let download_size = client
        .download_artifact(namespace, tool_name, &version, &output_path)
        .await?;

    println!(
        "  {} Downloaded {} ({})",
        "✓".bright_green(),
        output_path.display().to_string().dimmed(),
        format_size(download_size)
    );

    Ok(())
}

/// Add a tool from the registry.
pub async fn add_tool(name: &str) -> ToolResult<()> {
    use crate::references::PluginRef;
    use crate::registry::RegistryClient;

    let plugin_ref = name
        .parse::<PluginRef>()
        .map_err(|e| ToolError::Generic(format!("Invalid tool reference '{}': {}", name, e)))?;

    // Check if it has a namespace (required for registry fetch)
    if plugin_ref.namespace().is_none() {
        println!(
            "  {} Tool reference must include a namespace for registry fetch",
            "✗".bright_red()
        );
        println!(
            "    Example: {} or {}",
            "appcypher/filesystem".bright_cyan(),
            "myorg/mytool".bright_cyan()
        );
        return Ok(());
    }

    println!(
        "  {} Adding tool {} from registry...",
        "→".bright_blue(),
        name.bright_cyan()
    );

    // Create resolver with auto-install enabled
    let client = RegistryClient::new();
    let resolver = FilePluginResolver::default().with_auto_install(client);

    // Resolve will trigger auto-install if not found locally
    match resolver.resolve_tool(name).await? {
        Some(resolved) => {
            println!(
                "  {} Installed {} to {}",
                "✓".bright_green(),
                resolved.plugin_ref.to_string().bright_cyan(),
                resolved.path.parent().unwrap_or(&resolved.path).display()
            );
        }
        None => {
            println!(
                "  {} Tool {} not found in registry",
                "✗".bright_red(),
                name.bright_white().bold()
            );
        }
    }

    Ok(())
}

/// Remove an installed tool.
pub async fn remove_tool(name: &str) -> ToolResult<()> {
    use tokio::fs;

    let resolver = FilePluginResolver::default();

    // First, find the tool
    match resolver.resolve_tool(name).await? {
        Some(resolved) => {
            // Get the directory containing the tool
            let tool_dir = resolved
                .path
                .parent()
                .ok_or_else(|| ToolError::Generic("Failed to get tool directory".into()))?;

            println!(
                "  {} Removing tool {}...",
                "→".bright_blue(),
                resolved.plugin_ref.to_string().bright_cyan()
            );

            // Remove the directory
            fs::remove_dir_all(tool_dir).await.map_err(|e| {
                ToolError::Generic(format!("Failed to remove tool directory: {}", e))
            })?;

            println!(
                "  {} Removed {}",
                "✓".bright_green(),
                resolved.plugin_ref.to_string().bright_cyan()
            );
        }
        None => {
            println!(
                "  {} Tool {} not found",
                "✗".bright_red(),
                name.bright_white().bold()
            );
        }
    }

    Ok(())
}

/// Search for tools in the registry.
pub async fn search_tools(query: &str) -> ToolResult<()> {
    use crate::registry::RegistryClient;

    let client = RegistryClient::new();

    println!(
        "  {} Searching registry for tools matching: {}",
        "→".bright_blue(),
        query.bright_cyan()
    );

    let results = client.search(query, Some(20)).await?;

    if results.is_empty() {
        println!(
            "  {} No tools found matching: {}",
            "✗".bright_red(),
            query.bright_white().bold()
        );
        return Ok(());
    }

    let label = if results.len() == 1 { "tool" } else { "tools" };
    println!(
        "\n  {} Found {} {}\n",
        "✓".bright_green(),
        results.len().to_string().bold(),
        label
    );

    for result in &results {
        let version_str = result
            .latest_version
            .as_ref()
            .map(|v| format!("@{}", v))
            .unwrap_or_default();

        println!(
            "    {}/{}{} {}",
            result.namespace.bright_blue(),
            result.name.bright_cyan(),
            version_str.dimmed(),
            format!("↓{}", result.total_downloads).dimmed()
        );

        if let Some(desc) = &result.description {
            println!("      {}", desc.dimmed());
        }
    }

    println!();
    println!(
        "    {} {}",
        "Install with:".dimmed(),
        "tool add <namespace>/<name>".bright_white()
    );

    Ok(())
}

/// Publish a tool to the registry.
pub async fn publish_mcpb(path: &str, dry_run: bool) -> ToolResult<()> {
    use crate::handlers::auth::{get_registry_token, load_credentials};
    use crate::pack::{PackOptions, pack_bundle};
    use crate::registry::RegistryClient;
    use sha2::{Digest, Sha256};

    // Resolve the directory
    let dir = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| ToolError::Generic(format!("Directory not found: {}", path)))?;

    // Check manifest exists
    let manifest_path = dir.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(ToolError::Generic(format!(
            "manifest.json not found in {}. Run `tool init` first.",
            dir.display()
        )));
    }

    // Read manifest
    let manifest_content = std::fs::read_to_string(&manifest_path)?;
    let manifest: McpbManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| ToolError::Generic(format!("Failed to parse manifest.json: {}", e)))?;

    let tool_name = manifest
        .name
        .as_ref()
        .ok_or_else(|| ToolError::Generic("manifest.json must include a name field".into()))?;

    let version = manifest
        .version
        .as_ref()
        .ok_or_else(|| ToolError::Generic("manifest.json must include a version field".into()))?;

    // Validate version is semver
    if semver::Version::parse(version).is_err() {
        return Err(ToolError::Generic(format!(
            "Version '{}' is not valid semver (expected format: x.y.z)",
            version
        )));
    }

    // Get authenticated user
    let (namespace, token) = if dry_run {
        let creds = load_credentials().await?.map(|c| (c.username, c.token));
        match creds {
            Some((username, token)) => (username, Some(token)),
            None => ("<your-username>".to_string(), None),
        }
    } else {
        let token = get_registry_token().await?.ok_or_else(|| {
            ToolError::Generic("Authentication required. Run `tool login` first.".into())
        })?;
        let client = RegistryClient::new().with_auth_token(&token);
        let user = client.validate_token().await?;
        (user.username, Some(token))
    };

    let description = manifest.description.as_deref();

    if dry_run {
        println!(
            "  {} Dry run: validating tool {}/{}...",
            "→".bright_blue(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    } else {
        println!(
            "  {} Publishing tool {}/{}...",
            "→".bright_blue(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    }

    println!("    {}: {}", "Version".dimmed(), version.bright_white());
    println!(
        "    {}: {}",
        "Source".dimmed(),
        dir.display().to_string().dimmed()
    );
    if let Some(desc) = description {
        println!("    {}: {}", "Description".dimmed(), desc.dimmed());
    }

    // Bundle the tool
    println!("\n    {} Creating bundle...", "→".bright_blue());
    let pack_options = PackOptions {
        validate: true,
        output: None,
        verbose: false,
        include_dotfiles: false,
    };
    let pack_result = pack_bundle(&dir, &pack_options)
        .map_err(|e| ToolError::Generic(format!("Pack failed: {}", e)))?;

    // Read the bundle
    let bundle = std::fs::read(&pack_result.output_path)
        .map_err(|e| ToolError::Generic(format!("Failed to read bundle: {}", e)))?;
    let bundle_size = bundle.len() as u64;
    println!(
        "    Bundle size: {}",
        format_size(bundle_size).bright_white()
    );

    // Clean up temp bundle
    let _ = std::fs::remove_file(&pack_result.output_path);

    if dry_run {
        println!(
            "\n  {} Dry run complete. Would publish {}/{}@{}",
            "✓".bright_green(),
            namespace.bright_blue(),
            tool_name.bright_cyan(),
            version.bright_white()
        );
        return Ok(());
    }

    // Create registry client with auth
    let token = token.unwrap();
    let client = RegistryClient::new().with_auth_token(&token);

    // Check if artifact exists
    println!(
        "\n    {} Checking registry ({})...",
        "→".bright_blue(),
        client.registry_url().bright_white()
    );
    if !client.artifact_exists(&namespace, tool_name).await? {
        println!("    Creating new artifact entry...");
        client
            .create_artifact(&namespace, tool_name, description)
            .await?;
        println!(
            "    {} Created {}/{}",
            "✓".bright_green(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    }

    // Compute SHA-256
    let mut hasher = Sha256::new();
    hasher.update(&bundle);
    let sha256 = format!("{:x}", hasher.finalize());

    // Initiate upload
    println!("\n    {} Uploading bundle...", "→".bright_blue());
    let upload_info = client
        .init_upload(&namespace, tool_name, version, bundle_size, &sha256)
        .await?;

    // Upload to presigned URL
    client
        .upload_bundle(&upload_info.upload_url, &bundle)
        .await?;
    println!("    {} Upload complete", "✓".bright_green());

    // Publish the version
    println!("\n    {} Publishing version...", "→".bright_blue());

    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_content)?;

    let result = client
        .publish_version(
            &namespace,
            tool_name,
            &upload_info.upload_id,
            version,
            manifest_json,
            description,
        )
        .await?;

    println!(
        "\n  {} Published {}/{}@{}",
        "✓".bright_green(),
        namespace.bright_blue(),
        tool_name.bright_cyan(),
        result.version.bright_white()
    );
    println!(
        "    {}/plugins/{}/{}",
        client.registry_url(),
        namespace,
        tool_name
    );

    Ok(())
}

/// Detect an existing MCP server project and generate MCPB scaffolding.
pub async fn detect_mcpb(
    path: String,
    write: bool,
    entry: Option<String>,
    transport: Option<String>,
    name: Option<String>,
    force: bool,
) -> ToolResult<()> {
    use crate::detect::{DetectOptions, DetectorRegistry};
    use crate::mcpb::McpbTransport;

    // Resolve path
    let dir = PathBuf::from(&path);
    let dir = if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()?.join(&dir)
    };

    if !dir.exists() {
        return Err(ToolError::Generic(format!(
            "Directory not found: {}",
            dir.display()
        )));
    }

    // Check if manifest already exists (only matters in write mode)
    let manifest_path = dir.join(MCPB_MANIFEST_FILE);
    if write && manifest_path.exists() && !force {
        return Err(ToolError::Generic(
            "manifest.json already exists. Use --force to overwrite.".into(),
        ));
    }

    // Run detection
    let registry = DetectorRegistry::new();
    let detection = registry.detect(&dir).ok_or_else(|| {
        ToolError::Generic(
            "No MCP server project detected.\n\n    \
             Checked for:\n    \
             • Node.js with @modelcontextprotocol/sdk\n    \
             • Python with mcp package\n    \
             • Rust with rmcp crate"
                .into(),
        )
    })?;

    // Parse transport override
    let transport_override = transport
        .as_ref()
        .map(|t| match t.to_lowercase().as_str() {
            "http" => Ok(McpbTransport::Http),
            "stdio" => Ok(McpbTransport::Stdio),
            _ => Err(ToolError::Generic(format!(
                "Invalid transport '{}'. Use 'stdio' or 'http'.",
                t
            ))),
        })
        .transpose()?;

    // Build options
    let options = DetectOptions {
        entry_point: entry.clone(),
        transport: transport_override,
        package_manager: None,
        name: name.clone(),
    };

    // Print detection result
    let entry_display = options.entry_point.as_ref().or(detection
        .result
        .details
        .entry_point
        .as_ref());
    let transport_display = options
        .transport
        .or(detection.result.details.transport)
        .unwrap_or(McpbTransport::Stdio);

    println!(
        "\n  {} Detected {} MCP server\n",
        "✓".bright_green(),
        detection.display_name.bold()
    );

    println!("    {:<12} {}", "Type".dimmed(), detection.display_name);
    println!(
        "    {:<12} {}",
        "Transport".dimmed(),
        transport_display.to_string().to_lowercase()
    );

    if let Some(ep) = entry_display {
        let ep_exists = dir.join(ep).exists();
        if ep_exists {
            println!("    {:<12} {}", "Entry".dimmed(), ep);
        } else {
            println!(
                "    {:<12} {} {}",
                "Entry".dimmed(),
                ep,
                "(not found)".bright_yellow()
            );
        }
    } else {
        println!(
            "    {:<12} {}",
            "Entry".dimmed(),
            "(not detected)".bright_yellow()
        );
    }

    if let Some(pm) = &detection.result.details.package_manager {
        println!("    {:<12} {}", "Package".dimmed(), pm);
    }

    println!(
        "    {:<12} {:.0}%",
        "Confidence".dimmed(),
        detection.result.confidence * 100.0
    );

    // Show build command
    if let Some(build_cmd) = &detection.result.details.build_command {
        println!("    {:<12} {}", "Build".dimmed(), build_cmd.dimmed());
    }

    // Show notes/warnings
    for note in &detection.result.details.notes {
        println!("\n    {} {}", "⚠".bright_yellow(), note.bright_yellow());
    }

    // Format path for display in commands
    let path_arg = if path == "." {
        "".to_string()
    } else {
        format!(" {}", path)
    };

    if !write {
        // Dry-run mode - show what would be created
        println!("\n  {}:", "Files to create".dimmed());
        println!("    manifest.json");
        println!("    .mcpbignore");

        println!(
            "\n  Run {} to generate files.",
            format!("tool migrate{}", path_arg).bright_cyan()
        );
        return Ok(());
    }

    // Generate scaffolding
    let scaffold = registry.generate(detection.detector_name, &dir, &detection.result, &options)?;

    // Write manifest.json
    let manifest_json = serde_json::to_string_pretty(&scaffold.manifest)?;
    std::fs::write(&manifest_path, &manifest_json)?;

    // Write .mcpbignore
    let mcpbignore_path = dir.join(".mcpbignore");
    std::fs::write(&mcpbignore_path, &scaffold.mcpbignore)?;

    println!("\n  {} Created manifest.json", "✓".bright_green());
    println!("  {} Created .mcpbignore", "✓".bright_green());

    // Next steps
    println!("\n  {}:", "Next steps".bold());

    let has_build = detection.result.details.build_command.is_some();
    let entry_missing = entry_display
        .map(|ep| !dir.join(ep).exists())
        .unwrap_or(true);

    let mut step = 1;

    // Format path for next steps (use . for current dir, otherwise the path)
    let display_path = if path == "." {
        ".".to_string()
    } else {
        path.clone()
    };

    if has_build && entry_missing {
        println!(
            "    {}. {}",
            step,
            format!("tool build {}", display_path).bright_white(),
        );
        step += 1;
    }

    println!(
        "    {}. {}",
        step,
        format!("tool info {}", display_path).bright_white(),
    );
    step += 1;

    println!(
        "    {}. {}",
        step,
        format!("tool pack {}", display_path).bright_white(),
    );

    Ok(())
}
