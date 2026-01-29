//! Tool detection command handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::detect::{DetectOptions, DetectionMatch, DetectorRegistry};
use crate::error::{ToolError, ToolResult};
use crate::mcpb::McpbTransport;
use colored::Colorize;
use std::path::{Path, PathBuf};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Detect an existing MCP server project and generate MCPB scaffolding.
#[allow(clippy::too_many_arguments)]
pub async fn detect_mcpb(
    path: String,
    write: bool,
    entry: Option<String>,
    transport: Option<String>,
    name: Option<String>,
    force: bool,
    concise: bool,
    no_header: bool,
    verify: bool,
    yes: bool,
) -> ToolResult<()> {
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

    // For non-concise mode, use verbose detection to print signals as they happen
    let is_verbose = !concise;
    if is_verbose {
        println!("\n  {}", "Signals".dimmed());
    }
    let on_signal = |label: &str, passed: bool, weight: &str| {
        if is_verbose {
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
        }
    };

    let detection = registry.detect_verbose(&dir, &on_signal).ok_or_else(|| {
        ToolError::Generic(
            "No MCP server project detected.\n\n  \
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

    // Concise output: Header + TSV format
    if concise {
        use crate::concise::quote;
        if !no_header {
            println!("#type\ttransport\tentry\tconfidence\tbuild");
        }
        let entry_str = entry_display.map(|s| s.as_str()).unwrap_or("-");
        let build_str = detection
            .result
            .details
            .build_command
            .as_ref()
            .map(|s| quote(s))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{}\t{}\t{}\t{:.0}%\t{}",
            detection.display_name,
            transport_display.to_string().to_lowercase(),
            entry_str,
            detection.result.confidence * 100.0,
            build_str
        );
        return Ok(());
    }

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
        let ep_exists = dir.join(ep).exists();
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
        let verified = verify_server(&dir, &detection, transport_display, yes).await;
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

    // Format path for display in commands
    let path_arg = if path == "." {
        "".to_string()
    } else {
        format!(" {}", path)
    };

    if !write {
        // Dry-run mode - show what would be created
        println!("\n  {}:", "Files to create".dimmed());
        println!("  · manifest.json");
        println!("  · .mcpbignore");

        println!(
            "\n  Run {} to generate files.",
            format!("tool init{}", path_arg).bright_cyan()
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

    println!(
        "  {}. {}",
        step,
        format!("tool pack {}", display_path).bright_white(),
    );

    Ok(())
}

/// Verify detection by starting the server and sending an MCP initialize request.
/// Returns true if verification succeeded.
pub(super) async fn verify_server(
    dir: &Path,
    detection: &DetectionMatch,
    transport: McpbTransport,
    yes: bool,
) -> bool {
    use crate::mcpb::{McpbManifest, McpbServer, ResolvedMcpConfig, ResolvedMcpbManifest};
    use std::collections::BTreeMap;
    use std::io::IsTerminal;

    // Check entry point exists before attempting
    let mut entry_exists = detection
        .result
        .details
        .entry_point
        .as_ref()
        .map(|ep| dir.join(ep).exists())
        .unwrap_or(false);

    // If entry point doesn't exist but we have a build command, offer to build
    if !entry_exists {
        if let Some(build_cmd) = &detection.result.details.build_command {
            if !std::io::stdin().is_terminal() {
                println!(
                    "\n  {} {:<40} {}",
                    "–".dimmed(),
                    "Server responds to initialize",
                    "skipped (build first)".bright_yellow()
                );
                return false;
            }

            crate::prompt::init_theme();
            println!();
            let build = cliclack::confirm(format!(
                "Entry point not found. Build with `{}`?",
                build_cmd
            ))
            .initial_value(true)
            .interact();

            match build {
                Ok(true) => {
                    let _ = cliclack::outro("Building...");
                    let status = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(build_cmd)
                        .current_dir(dir)
                        .status();

                    match status {
                        Ok(s) if s.success() => {
                            // Re-check entry point after build
                            entry_exists = detection
                                .result
                                .details
                                .entry_point
                                .as_ref()
                                .map(|ep| dir.join(ep).exists())
                                .unwrap_or(false);

                            if !entry_exists {
                                println!(
                                    "\n  {} {:<40} {}",
                                    "✗".bright_red(),
                                    "Server responds to initialize",
                                    "entry point still not found after build".bright_red()
                                );
                                return false;
                            }
                        }
                        Ok(_) => {
                            println!(
                                "\n  {} {:<40} {}",
                                "✗".bright_red(),
                                "Server responds to initialize",
                                "build failed".bright_red()
                            );
                            return false;
                        }
                        Err(e) => {
                            println!(
                                "\n  {} {:<40} {}",
                                "✗".bright_red(),
                                "Server responds to initialize",
                                format!("build error: {}", e).bright_red()
                            );
                            return false;
                        }
                    }
                }
                Ok(false) => {
                    let _ = cliclack::outro_cancel("Build skipped.");
                    println!(
                        "\n  {} {:<40} {}",
                        "–".dimmed(),
                        "Server responds to initialize",
                        "skipped (build first)".bright_yellow()
                    );
                    return false;
                }
                Err(_) => {
                    return false;
                }
            }
        } else {
            println!(
                "\n  {} {:<40} {}",
                "–".dimmed(),
                "Server responds to initialize",
                "skipped (entry point not found)".bright_yellow()
            );
            return false;
        }
    }

    // Prompt user before running the server
    if !yes && std::io::stdin().is_terminal() {
        crate::prompt::init_theme();
        println!();
        let proceed = cliclack::confirm("Verify will start the server process. Proceed?")
            .initial_value(true)
            .interact();

        match proceed {
            Ok(true) => {
                let _ = cliclack::outro("Verifying...");
                println!();
            }
            Ok(false) => {
                let _ = cliclack::outro_cancel("Verification skipped.");
                println!(
                    "\n  {} {:<40} {}",
                    "–".dimmed(),
                    "Server responds to initialize",
                    "skipped".dimmed()
                );
                return false;
            }
            Err(_) => {
                return false;
            }
        }
    }

    // Build a ResolvedMcpbManifest from detection result
    let run_command = detection.result.details.run_command.clone();
    let run_args = detection.result.details.run_args.clone();

    // Replace ${__dirname} with actual dir path
    let dir_str = dir.to_string_lossy();
    let resolved_args: Vec<String> = run_args
        .into_iter()
        .map(|a| a.replace("${__dirname}", &dir_str))
        .collect();
    let resolved_command = run_command.map(|c| c.replace("${__dirname}", &dir_str));

    let resolved = ResolvedMcpbManifest {
        manifest: McpbManifest {
            manifest_version: "0.3".to_string(),
            name: None,
            version: None,
            description: None,
            author: None,
            server: McpbServer {
                server_type: Some(detection.server_type),
                transport,
                entry_point: detection.result.details.entry_point.clone(),
                mcp_config: None,
            },
            display_name: None,
            long_description: None,
            license: None,
            icon: None,
            icons: None,
            homepage: None,
            documentation: None,
            support: None,
            repository: None,
            keywords: None,
            tools: None,
            prompts: None,
            tools_generated: None,
            prompts_generated: None,
            user_config: None,
            system_config: None,
            compatibility: None,
            privacy_policies: None,
            localization: None,
            meta: None,
            bundle_path: None,
        },
        mcp_config: ResolvedMcpConfig {
            command: resolved_command,
            args: resolved_args,
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
            oauth_config: None,
        },
        transport,
        is_reference: false,
    };

    // Attempt connection with timeout
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        crate::mcp::connect(&resolved, false),
    )
    .await;

    match result {
        Ok(Ok(crate::mcp::ConnectResult::Connected(_conn))) => {
            println!(
                "  {} {:<40} {}",
                "✓".bright_green(),
                "Server responds to initialize",
                "+20%".dimmed()
            );
            // Connection dropped here, auto-kills process
            true
        }
        Ok(Ok(_)) => {
            // Auth required or other non-connected state
            println!(
                "  {} {:<40} {}",
                "✗".bright_red(),
                "Server responds to initialize",
                "auth required".bright_red()
            );
            false
        }
        Ok(Err(e)) => {
            println!(
                "  {} {:<40} {}",
                "✗".bright_red(),
                "Server responds to initialize",
                format!("failed: {}", e).bright_red()
            );
            false
        }
        Err(_) => {
            println!(
                "  {} {:<40} {}",
                "✗".bright_red(),
                "Server responds to initialize",
                "timed out (30s)".bright_red()
            );
            false
        }
    }
}
