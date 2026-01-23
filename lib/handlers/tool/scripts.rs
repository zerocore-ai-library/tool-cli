//! Script execution handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use colored::Colorize;
use std::path::PathBuf;
use std::process::Command;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Run a script from manifest.json `_meta.store.tool.mcpb.scripts`
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

    // Extract script from _meta.store.tool.mcpb.scripts
    let script_cmd = manifest
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|r| r.get("scripts"))
        .and_then(|s| s.get(script_name))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            ToolError::Generic(format!(
                "Script '{}' not found in manifest.json\nDefine it in _meta.store.tool.mcpb.scripts or run it directly (e.g., `tool build`, `tool test`).",
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
        .and_then(|m| m.get("store.tool.mcpb"))
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
            println!("  Add scripts to _meta.store.tool.mcpb.scripts");
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
pub(super) fn resolve_target_dir(path: &Option<String>) -> ToolResult<PathBuf> {
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
