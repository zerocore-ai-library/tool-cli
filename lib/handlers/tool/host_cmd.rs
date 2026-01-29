//! Host command handlers for managing MCP host configurations.

use colored::Colorize;
use serde_json::json;

use crate::commands::HostCommand;
use crate::error::{ToolError, ToolResult};
use crate::hosts::{
    McpHost, create_backup, generate_codex_server_entry, generate_server_entry, load_config,
    load_metadata, save_config, save_metadata, tool_ref_to_server_name,
};
use crate::prompt::init_theme;
use crate::resolver::FilePluginResolver;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Main entry point for host command.
pub async fn handle_host_command(
    cmd: HostCommand,
    concise: bool,
    no_header: bool,
) -> ToolResult<()> {
    match cmd {
        HostCommand::Add {
            host,
            tools,
            dry_run,
            overwrite,
            yes,
        } => host_add(&host, tools, dry_run, overwrite, yes, concise).await,
        HostCommand::Remove {
            host,
            tools,
            dry_run,
            yes,
        } => host_remove(&host, tools, dry_run, yes, concise).await,
        HostCommand::List => host_list(concise, no_header).await,
        HostCommand::Preview { host, tools } => host_preview(&host, tools, concise).await,
        HostCommand::Path { host } => host_path(&host).await,
    }
}

/// List all installed tools.
async fn get_installed_tools() -> ToolResult<Vec<String>> {
    let resolver = FilePluginResolver::default();
    let tools = resolver.list_tools().await?;
    Ok(tools.into_iter().map(|t| t.to_string()).collect())
}

/// Add tools to a host.
async fn host_add(
    host_name: &str,
    tools: Vec<String>,
    dry_run: bool,
    overwrite: bool,
    yes: bool,
    concise: bool,
) -> ToolResult<()> {
    let host = McpHost::parse(host_name)?;

    // Get tools to add
    let tools_to_add = if tools.is_empty() {
        get_installed_tools().await?
    } else {
        tools
    };

    if tools_to_add.is_empty() {
        if concise {
            println!("ok\t0");
        } else {
            println!(
                "  {} No tools to add. Install tools first with {}.",
                "!".bright_yellow(),
                "tool install".bright_cyan()
            );
        }
        return Ok(());
    }

    // Load existing config
    let mut config = load_config(&host)?;
    let mut metadata = load_metadata(&host)?;

    // Ensure servers object exists (mcpServers for most hosts, servers for VSCode)
    let server_key = host.server_key();
    if config.get(server_key).is_none() {
        config[server_key] = json!({});
    }
    let servers = config[server_key].as_object_mut().unwrap();

    // Track changes
    let mut added = Vec::new();
    let mut skipped = Vec::new();

    for tool_ref in &tools_to_add {
        let server_name = tool_ref_to_server_name(tool_ref);

        if servers.contains_key(&server_name) && !overwrite {
            skipped.push(tool_ref.clone());
            continue;
        }

        let entry = if matches!(host, McpHost::Codex) {
            generate_codex_server_entry(tool_ref)
        } else {
            generate_server_entry(tool_ref, &host)
        };
        servers.insert(server_name, entry);

        if !metadata.managed_tools.contains(tool_ref) {
            metadata.managed_tools.push(tool_ref.clone());
        }
        added.push(tool_ref.clone());
    }

    // Dry-run output
    if dry_run {
        if concise {
            println!("#action\ttool");
            for tool in &added {
                println!("add\t{}", tool);
            }
            for tool in &skipped {
                println!("skip\t{}", tool);
            }
        } else {
            println!(
                "  {} Would modify: {}\n",
                "→".bright_blue(),
                host.config_path()?.display()
            );
            for tool in &added {
                println!("  {} {}    {}", "+".bright_green(), tool, "(new)".dimmed());
            }
            for tool in &skipped {
                println!(
                    "  {} {}    {}",
                    "~".bright_yellow(),
                    tool,
                    "(skip, already exists)".dimmed()
                );
            }
            println!(
                "\n  · Run without {} to apply changes.\n",
                "--dry-run".bold()
            );
        }
        return Ok(());
    }

    // Nothing to add
    if added.is_empty() {
        if concise {
            println!("ok\t0");
        } else {
            println!(
                "  {} All {} tool(s) already configured for {}.\n",
                "~".bright_yellow(),
                skipped.len(),
                host.display_name()
            );
        }
        return Ok(());
    }

    // Confirm if not --yes
    if !yes {
        init_theme();
        println!();
        let proceed = cliclack::confirm(format!(
            "Add {} tool(s) to {}?",
            added.len(),
            host.display_name()
        ))
        .initial_value(true)
        .interact()?;

        if !proceed {
            cliclack::outro_cancel("Cancelled.")?;
            return Err(ToolError::Cancelled);
        }
    }

    // Create backup before modification
    let backup_path = create_backup(&host)?;

    // Save config and metadata
    save_config(&host, &config)?;
    save_metadata(&host, &metadata)?;

    if !yes {
        cliclack::outro("Done!")?;
    }

    // Output result
    if concise {
        println!("ok\t{}", added.len());
    } else {
        println!(
            "\n  {} Added {} tool(s) to {}\n",
            "✓".bright_green(),
            added.len(),
            host.display_name()
        );
        for tool in &added {
            println!("  {} {}", "+".bright_green(), tool);
        }
        if let Some(backup) = backup_path {
            println!("\n  · {}: {}\n", "Backup".dimmed(), backup.display());
        } else {
            println!();
        }
    }

    Ok(())
}

/// Remove tools from a host.
async fn host_remove(
    host_name: &str,
    tools: Vec<String>,
    dry_run: bool,
    yes: bool,
    concise: bool,
) -> ToolResult<()> {
    let host = McpHost::parse(host_name)?;

    // Load config and metadata
    let mut config = load_config(&host)?;
    let mut metadata = load_metadata(&host)?;

    // Determine tools to remove (only allow removing tools managed by tool-cli)
    let tools_to_remove = if tools.is_empty() {
        metadata.managed_tools.clone()
    } else {
        tools
            .into_iter()
            .filter(|t| metadata.managed_tools.contains(t))
            .collect()
    };

    if tools_to_remove.is_empty() {
        if concise {
            println!("ok\t0");
        } else {
            println!(
                "  {} No tool-cli managed tools found for {}.\n",
                "!".bright_yellow(),
                host.display_name()
            );
        }
        return Ok(());
    }

    // Get servers object (mcpServers for most hosts, servers for VSCode)
    let server_key = host.server_key();
    let servers = match config.get_mut(server_key).and_then(|v| v.as_object_mut()) {
        Some(s) => s,
        None => {
            if concise {
                println!("ok\t0");
            } else {
                println!(
                    "  {} No {} configured for {}.\n",
                    "!".bright_yellow(),
                    server_key,
                    host.display_name()
                );
            }
            return Ok(());
        }
    };

    // Track removals
    let mut removed = Vec::new();

    for tool_ref in &tools_to_remove {
        let server_name = tool_ref_to_server_name(tool_ref);
        if servers.remove(&server_name).is_some() {
            removed.push(tool_ref.clone());
        }
        metadata.managed_tools.retain(|t| t != tool_ref);
    }

    // Dry-run output
    if dry_run {
        if concise {
            println!("#action\ttool");
            for tool in &removed {
                println!("remove\t{}", tool);
            }
        } else {
            println!(
                "  {} Would modify: {}\n",
                "→".bright_blue(),
                host.config_path()?.display()
            );
            for tool in &removed {
                println!("  {} {}", "-".bright_red(), tool);
            }
            println!(
                "\n  · Run without {} to apply changes.\n",
                "--dry-run".bold()
            );
        }
        return Ok(());
    }

    // Nothing to remove
    if removed.is_empty() {
        if concise {
            println!("ok\t0");
        } else {
            println!(
                "  {} No matching tools found to remove.\n",
                "!".bright_yellow()
            );
        }
        return Ok(());
    }

    // Confirm if not --yes
    if !yes {
        init_theme();
        println!();
        let proceed = cliclack::confirm(format!(
            "Remove {} tool(s) from {}?",
            removed.len(),
            host.display_name()
        ))
        .initial_value(true)
        .interact()?;

        if !proceed {
            cliclack::outro_cancel("Cancelled.")?;
            return Err(ToolError::Cancelled);
        }
    }

    // Create backup before modification
    let backup_path = create_backup(&host)?;

    // Save config and metadata
    save_config(&host, &config)?;
    save_metadata(&host, &metadata)?;

    if !yes {
        cliclack::outro("Done!")?;
    }

    // Output result
    if concise {
        println!("ok\t{}", removed.len());
    } else {
        println!(
            "\n  {} Removed {} tool(s) from {}\n",
            "✓".bright_green(),
            removed.len(),
            host.display_name()
        );
        for tool in &removed {
            println!("  {} {}", "-".bright_red(), tool);
        }
        if let Some(backup) = backup_path {
            println!("\n  · {}: {}\n", "Backup".dimmed(), backup.display());
        } else {
            println!();
        }
    }

    Ok(())
}

/// List all hosts and their status.
async fn host_list(concise: bool, no_header: bool) -> ToolResult<()> {
    if concise && !no_header {
        println!("#host\ttools\tstatus\tpath");
    }

    if !concise {
        println!("\n  {} Supported MCP hosts\n", "✓".bright_green());
    }

    for host in McpHost::all() {
        let path = host.config_path()?;
        let exists = path.exists();
        let metadata = load_metadata(host).unwrap_or_default();
        let tool_count = metadata.managed_tools.len();

        if concise {
            let status = if exists { "configured" } else { "not_found" };
            println!(
                "{}\t{}\t{}\t{}",
                host.canonical_name(),
                tool_count,
                status,
                path.display()
            );
        } else {
            let tool_label = if tool_count == 1 { "tool" } else { "tools" };
            let status = if exists {
                format!("{} {}", tool_count, tool_label)
            } else {
                format!("{} {} {}", tool_count, tool_label, "(not found)".dimmed())
            };
            println!(
                "  · {:<18} {:<15} {}",
                host.canonical_name(),
                status,
                path.display().to_string().dimmed()
            );
        }
    }

    if !concise {
        println!();
    }

    Ok(())
}

/// Preview the config that would be generated.
async fn host_preview(host_name: &str, tools: Vec<String>, concise: bool) -> ToolResult<()> {
    let host = McpHost::parse(host_name)?;

    let tools_to_show = if tools.is_empty() {
        get_installed_tools().await?
    } else {
        tools
    };

    let mut servers = serde_json::Map::new();
    for tool_ref in &tools_to_show {
        let server_name = tool_ref_to_server_name(tool_ref);
        let entry = if matches!(host, McpHost::Codex) {
            generate_codex_server_entry(tool_ref)
        } else {
            generate_server_entry(tool_ref, &host)
        };
        servers.insert(server_name, entry);
    }

    // Use the appropriate key for the host (mcpServers vs servers)
    let output = json!({
        host.server_key(): servers
    });

    if concise {
        println!("{}", serde_json::to_string(&output)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

/// Print config path for a host.
async fn host_path(host_name: &str) -> ToolResult<()> {
    let host = McpHost::parse(host_name)?;
    println!("{}", host.config_path()?.display());
    Ok(())
}
