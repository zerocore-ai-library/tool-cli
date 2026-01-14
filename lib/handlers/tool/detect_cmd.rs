//! Tool detection command handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::detect::{DetectOptions, DetectorRegistry};
use crate::error::{ToolError, ToolResult};
use crate::mcpb::McpbTransport;
use colored::Colorize;
use std::path::PathBuf;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Detect an existing MCP server project and generate MCPB scaffolding.
pub async fn detect_mcpb(
    path: String,
    write: bool,
    entry: Option<String>,
    transport: Option<String>,
    name: Option<String>,
    force: bool,
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
                "(inferred)".bright_yellow()
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
