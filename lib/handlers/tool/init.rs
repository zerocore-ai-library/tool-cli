//! Tool initialization handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::mcpb::{
    InitMode, McpbAuthor, McpbManifest, McpbServerType, McpbTransport, NodePackageManager,
    PackageManager, PythonPackageManager,
};
use crate::scaffold::{
    mcpbignore_template, node_gitignore_template, node_scaffold, python_gitignore_template,
    python_scaffold, rust_gitignore_template, rust_mcpbignore_template, rust_scaffold,
};
use colored::Colorize;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    entry: Option<String>,
    transport: Option<String>,
    force: bool,
    verify: bool,
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

    // Check directory state
    let manifest_path = target_dir.join(MCPB_MANIFEST_FILE);
    let manifest_exists = manifest_path.exists();
    let is_empty = is_dir_empty(&target_dir)?;

    // Check if manifest.json already exists
    if manifest_exists && !force {
        return Err(ToolError::Generic(
            "manifest.json already exists. Use --force to overwrite.".into(),
        ));
    }

    // Non-empty directory -> migration flow (detection-based)
    // Handles both: new migration and re-migration with --force
    if !is_empty {
        return init_migrate(
            target_dir,
            name,
            entry,
            transport,
            yes,
            force,
            path.as_deref(),
            verify,
        )
        .await;
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
    let mcpbignore_content: String = if is_rust {
        rust_mcpbignore_template(&pkg_name)
    } else {
        mcpbignore_template().to_string()
    };
    std::fs::write(&mcpbignore_path, &mcpbignore_content)?;

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
pub(super) fn parse_package_manager(pm: &str) -> Option<PackageManager> {
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

/// Check if a directory is empty (ignoring hidden files like .git).
fn is_dir_empty(dir: &Path) -> ToolResult<bool> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Ignore hidden files/directories (like .git)
        if !name_str.starts_with('.') {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Handle migration of existing project to MCPB format.
#[allow(clippy::too_many_arguments)]
async fn init_migrate(
    target_dir: PathBuf,
    name: Option<String>,
    entry: Option<String>,
    transport: Option<String>,
    yes: bool,
    _force: bool,
    display_path: Option<&str>,
    verify: bool,
) -> ToolResult<()> {
    use crate::detect::{
        DetectOptions, DetectorRegistry, EnvConfigType, EnvVar, parse_env_example,
    };
    use crate::mcpb::{
        McpbSystemConfigField, McpbSystemConfigType, McpbTransport, McpbUserConfigField,
        McpbUserConfigType,
    };
    use std::collections::BTreeMap;
    use std::io::IsTerminal;

    let manifest_path = target_dir.join(MCPB_MANIFEST_FILE);

    // Run detection with verbose signal reporting
    let registry = DetectorRegistry::new();

    println!("\n  {}", "Signals".dimmed());
    let on_signal = |label: &str, passed: bool, weight: &str| {
        if passed {
            println!(
                "  {} {:<40} {}",
                "✓".bright_green(),
                label,
                format!("+{}", weight).dimmed()
            );
        } else {
            println!(
                "  {} {:<40} {}",
                "✗".bright_red(),
                label,
                format!("-{}", weight).bright_red()
            );
        }
    };

    let detection = registry
        .detect_verbose(&target_dir, &on_signal)
        .ok_or_else(|| {
            ToolError::Generic(
                "No MCP server project detected in this directory.\n\n  \
             · If this is a new project, remove existing files or use an empty directory.\n\n  \
             Checked for:\n  \
             · Node.js with @modelcontextprotocol/sdk\n  \
             · Python with mcp package\n  \
             · Rust with rmcp crate"
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

    println!("  · {:<12} {}", "Type".dimmed(), detection.display_name);
    println!(
        "  · {:<12} {}",
        "Transport".dimmed(),
        transport_display.to_string().to_lowercase()
    );

    if let Some(ep) = entry_display {
        let ep_exists = target_dir.join(ep).exists();
        if ep_exists {
            println!("  · {:<12} {}", "Entry".dimmed(), ep);
        } else {
            println!(
                "  · {:<12} {} {}",
                "Entry".dimmed(),
                ep,
                "(inferred)".bright_yellow()
            );
        }
    } else {
        println!(
            "  · {:<12} {}",
            "Entry".dimmed(),
            "(not detected)".bright_yellow()
        );
    }

    if let Some(pm) = &detection.result.details.package_manager {
        println!("  · {:<12} {}", "Package".dimmed(), pm);
    }

    println!(
        "  · {:<12} {:.0}%",
        "Confidence".dimmed(),
        detection.result.confidence * 100.0
    );

    // Show build command
    if let Some(build_cmd) = &detection.result.details.build_command {
        println!("  · {:<12} {}", "Build".dimmed(), build_cmd.dimmed());
    }

    // Verify: start server and send MCP initialize
    if verify {
        let verified =
            super::detect_cmd::verify_server(&target_dir, &detection, transport_display, yes).await;
        let final_confidence = if verified {
            100.0
        } else {
            detection.result.confidence * 100.0
        };
        println!(
            "\n  · {:<12} {:.0}%",
            "Confidence".dimmed(),
            final_confidence
        );
    }

    // Show notes/warnings
    for note in &detection.result.details.notes {
        println!("\n  {} {}", "⚠".bright_yellow(), note.bright_yellow());
    }

    // Parse .env.example for env vars
    let env_vars = parse_env_example(&target_dir);
    let selected_env_vars: Vec<EnvVar> = if !env_vars.is_empty() {
        if yes {
            // With --yes, include all env vars
            env_vars
        } else if std::io::stdin().is_terminal() {
            // Interactive: let user select which env vars to include
            println!();
            crate::prompt::init_theme();

            let options: Vec<(EnvVar, String, String)> = env_vars
                .into_iter()
                .map(|var| {
                    let label = format!(
                        "{} → {}.{}{}",
                        var.name,
                        match var.config_type {
                            EnvConfigType::System => "system_config",
                            EnvConfigType::User => "user_config",
                        },
                        var.config_key(),
                        if var.sensitive { " (sensitive)" } else { "" }
                    );
                    let hint = var
                        .default
                        .as_ref()
                        .map(|d| format!("default: {}", d))
                        .unwrap_or_default();
                    (var, label, hint)
                })
                .collect();

            let selected: Vec<EnvVar> =
                cliclack::multiselect("Select env vars to include in manifest (from .env.example)")
                    .items(
                        &options
                            .iter()
                            .map(|(var, label, hint)| (var.clone(), label.as_str(), hint.as_str()))
                            .collect::<Vec<_>>(),
                    )
                    .required(false)
                    .interact()?;

            selected
        } else {
            // Non-interactive without --yes: skip env vars
            vec![]
        }
    } else {
        vec![]
    };

    // Show preview of files to create
    println!("\n  {}:", "Files to create".dimmed());
    println!("  · manifest.json");
    println!("  · .mcpbignore");

    // Confirmation prompt (unless --yes)
    if !yes && std::io::stdin().is_terminal() {
        crate::prompt::init_theme();
        println!();
        let confirmed: bool = cliclack::confirm("Proceed with migration?")
            .initial_value(true)
            .interact()?;

        if !confirmed {
            cliclack::outro_cancel("Migration cancelled.")?;
            return Ok(());
        }

        cliclack::outro("Migrating...")?;
    }

    // Generate scaffolding (manifest + mcpbignore only)
    let mut scaffold = registry.generate(
        detection.detector_name,
        &target_dir,
        &detection.result,
        &options,
    )?;

    // Convert selected env vars to config fields and merge into manifest
    if !selected_env_vars.is_empty() {
        let mut user_config: BTreeMap<String, McpbUserConfigField> =
            scaffold.manifest.user_config.take().unwrap_or_default();
        let mut system_config: BTreeMap<String, McpbSystemConfigField> =
            scaffold.manifest.system_config.take().unwrap_or_default();

        for var in selected_env_vars {
            match var.config_type {
                EnvConfigType::System => {
                    // Only Port is a valid system config type; others go to user_config
                    if matches!(var.value_type, crate::detect::EnvValueType::Port) {
                        let default: Option<serde_json::Value> = var.default.as_ref().map(|d| {
                            if let Ok(n) = d.parse::<i64>() {
                                serde_json::Value::Number(serde_json::Number::from(n))
                            } else {
                                serde_json::Value::String(d.clone())
                            }
                        });
                        system_config.insert(
                            var.config_key(),
                            McpbSystemConfigField {
                                field_type: McpbSystemConfigType::Port,
                                title: var.name.clone(),
                                description: None,
                                required: None,
                                default,
                            },
                        );
                    } else {
                        // Non-port system vars (like hostname) go to user_config as strings
                        let default = var
                            .default
                            .as_ref()
                            .map(|d| serde_json::Value::String(d.clone()));
                        user_config.insert(
                            var.config_key(),
                            McpbUserConfigField {
                                field_type: McpbUserConfigType::String,
                                title: var.name.clone(),
                                description: None,
                                required: None,
                                default,
                                multiple: None,
                                sensitive: None,
                                enum_values: None,
                                min: None,
                                max: None,
                            },
                        );
                    }
                }
                EnvConfigType::User => {
                    let field_type = match var.value_type {
                        crate::detect::EnvValueType::String => McpbUserConfigType::String,
                        crate::detect::EnvValueType::Number => McpbUserConfigType::Number,
                        crate::detect::EnvValueType::Boolean => McpbUserConfigType::Boolean,
                        crate::detect::EnvValueType::Port => McpbUserConfigType::Number,
                        crate::detect::EnvValueType::Hostname => McpbUserConfigType::String,
                    };
                    let default: Option<serde_json::Value> =
                        var.default.as_ref().map(|d| match var.value_type {
                            crate::detect::EnvValueType::Number
                            | crate::detect::EnvValueType::Port => d
                                .parse::<i64>()
                                .map(|n| serde_json::Value::Number(serde_json::Number::from(n)))
                                .unwrap_or_else(|_| serde_json::Value::String(d.clone())),
                            crate::detect::EnvValueType::Boolean => {
                                serde_json::Value::Bool(d == "true")
                            }
                            _ => serde_json::Value::String(d.clone()),
                        });
                    user_config.insert(
                        var.config_key(),
                        McpbUserConfigField {
                            field_type,
                            title: var.name.clone(),
                            description: None,
                            required: None,
                            default,
                            multiple: None,
                            sensitive: if var.sensitive { Some(true) } else { None },
                            enum_values: None,
                            min: None,
                            max: None,
                        },
                    );
                }
            }
        }

        if !user_config.is_empty() {
            scaffold.manifest.user_config = Some(user_config);
        }
        if !system_config.is_empty() {
            scaffold.manifest.system_config = Some(system_config);
        }
    }

    // Write manifest.json
    let manifest_json = serde_json::to_string_pretty(&scaffold.manifest)?;
    std::fs::write(&manifest_path, &manifest_json)?;

    // Write .mcpbignore
    let mcpbignore_path = target_dir.join(".mcpbignore");
    std::fs::write(&mcpbignore_path, &scaffold.mcpbignore)?;

    let is_mcpbx = scaffold.manifest.requires_mcpbx();
    let format_display = if is_mcpbx {
        "mcpbx".bright_yellow()
    } else {
        "mcpb".bright_green()
    };
    println!(
        "\n  {} Created manifest.json ({})",
        "✓".bright_green(),
        format_display
    );
    println!("  {} Created .mcpbignore", "✓".bright_green());

    // Print next steps
    print_migrate_next_steps(
        &detection,
        &target_dir,
        entry_display,
        display_path,
        is_mcpbx,
    );

    Ok(())
}

/// Print next steps after migration.
fn print_migrate_next_steps(
    detection: &crate::detect::DetectionMatch,
    target_dir: &Path,
    entry_display: Option<&String>,
    display_path: Option<&str>,
    is_mcpbx: bool,
) {
    println!("\n  {}:", "Next steps".bold());

    let has_build = detection.result.details.build_command.is_some();
    let entry_missing = entry_display
        .map(|ep| !target_dir.join(ep).exists())
        .unwrap_or(true);

    let mut step = 1;

    let display_path = display_path.unwrap_or(".");

    if has_build && entry_missing {
        println!(
            "  {}. {}",
            step,
            format!("tool build {}", display_path).bright_white(),
        );
        step += 1;
    }

    println!(
        "  {}. {}",
        step,
        format!("tool info {}", display_path).bright_white(),
    );
    step += 1;

    println!(
        "  {}. {}",
        step,
        format!("tool run {}", display_path).bright_white(),
    );
    step += 1;

    let pack_ext = if is_mcpbx {
        ".mcpbx".bright_yellow().to_string()
    } else {
        ".mcpb".bright_green().to_string()
    };
    println!(
        "  {}. {}  {}",
        step,
        format!("tool pack {}", display_path).bright_white(),
        format!("# create {} bundle", pack_ext).dimmed(),
    );
}

/// Validate package name format.
pub(super) fn is_valid_package_name(name: &str) -> bool {
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
    let is_mcpbx = mode.is_reference() || mode.is_http();

    println!("  · {}       {}", "Type".dimmed(), type_display);
    println!("  · {}  {}", "Transport".dimmed(), transport_display);
    if is_mcpbx {
        println!("  · {}     {}", "Format".dimmed(), "mcpbx".bright_yellow());
    } else {
        println!("  · {}     {}", "Format".dimmed(), "mcpb".bright_green());
    }

    if !mode.is_reference() {
        if is_rust {
            println!("  · {}      target/release/{}", "Entry".dimmed(), name);
        } else {
            match mode.server_type() {
                Some(McpbServerType::Node) => {
                    println!("  · {}      server/index.js", "Entry".dimmed());
                }
                Some(McpbServerType::Python) => {
                    println!("  · {}      server/main.py", "Entry".dimmed());
                }
                Some(McpbServerType::Binary) | None => {}
            }
        }
    }
    println!("  · {}    0.1.0\n", "Version".dimmed());

    // Tree structure
    let prefix = match dir_path {
        Some(p) => format!("{}/", p),
        None => "./".to_string(),
    };

    println!("  {}", prefix.bold());

    if mode.is_reference() {
        println!("  ├── manifest.json");
        println!("  ├── README.md");
        println!("  └── .mcpbignore");
    } else if is_rust {
        println!("  ├── manifest.json");
        println!("  ├── README.md");
        println!("  ├── Cargo.toml");
        println!("  ├── .gitignore");
        println!("  ├── .mcpbignore");
        println!("  └── src/");
        println!("      ├── main.rs");
        println!("      └── lib.rs");
    } else {
        match mode.server_type() {
            Some(McpbServerType::Node) => {
                println!("  ├── manifest.json");
                println!("  ├── README.md");
                println!("  ├── package.json");
                println!("  ├── .gitignore");
                println!("  ├── .mcpbignore");
                println!("  └── server/");
                println!("      └── index.js");
            }
            Some(McpbServerType::Python) => {
                let project_file = match mode.python_package_manager() {
                    Some(PythonPackageManager::Pip) => "requirements.txt",
                    _ => "pyproject.toml",
                };
                println!("  ├── manifest.json");
                println!("  ├── README.md");
                println!("  ├── {}", project_file);
                println!("  ├── .gitignore");
                println!("  ├── .mcpbignore");
                println!("  └── server/");
                println!("      └── main.py");
            }
            Some(McpbServerType::Binary) | None => {
                println!("  ├── manifest.json");
                println!("  ├── README.md");
                println!("  └── .mcpbignore");
            }
        }
    }

    // Next steps
    println!("\n  {}:", "Next Steps".bold());

    let mut step = 1;

    if let Some(p) = dir_path {
        println!("  {}. cd {}", step, p);
        step += 1;
    }

    if mode.is_reference() {
        if mode.is_http() {
            println!(
                "  {}. {}",
                step,
                "# Set url and credentials in manifest.json".dimmed()
            );
        } else {
            println!(
                "  {}. {}",
                step,
                "# Set command path in manifest.json".dimmed()
            );
        }
        println!(
            "  {}. tool info               {}",
            step + 1,
            "# verify connection".dimmed()
        );
    } else if is_rust {
        println!(
            "  {}. tool build              {}",
            step,
            "# build binary".dimmed()
        );
        println!(
            "  {}. tool info               {}",
            step + 1,
            "# list tools".dimmed()
        );
        println!(
            "  {}. tool call -m hello      {}",
            step + 2,
            "# test a tool".dimmed()
        );
        println!(
            "  {}. tool run                {}",
            step + 3,
            "# run server interactively".dimmed()
        );
        let pack_hint = if is_mcpbx {
            format!("# create {} bundle", ".mcpbx".bright_yellow())
        } else {
            format!("# create {} bundle", ".mcpb".bright_green())
        };
        println!(
            "  {}. tool pack               {}",
            step + 4,
            pack_hint.dimmed()
        );
    } else {
        println!(
            "  {}. tool build              {}",
            step,
            "# install dependencies".dimmed()
        );
        println!(
            "  {}. tool info               {}",
            step + 1,
            "# list tools".dimmed()
        );
        println!(
            "  {}. tool call -m hello      {}",
            step + 2,
            "# test a tool".dimmed()
        );
        println!(
            "  {}. tool run                {}",
            step + 3,
            "# run server interactively".dimmed()
        );
        let pack_hint = if is_mcpbx {
            format!("# create {} bundle", ".mcpbx".bright_yellow())
        } else {
            format!("# create {} bundle", ".mcpb".bright_green())
        };
        println!(
            "  {}. tool pack               {}",
            step + 4,
            pack_hint.dimmed()
        );
    }
}
