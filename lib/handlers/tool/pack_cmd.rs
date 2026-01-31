//! Tool pack command handlers.

use crate::error::{ToolError, ToolResult};
use crate::mcpb::McpbManifest;
use crate::pack::{PackError, PackOptions, PackResult, pack_bundle, pack_bundle_for_platform};
use crate::styles::Spinner;
use crate::validate::validate_manifest;
use colored::Colorize;
use std::path::{Path, PathBuf};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Pack a tool into an .mcpb bundle.
pub async fn pack_mcpb(
    path: Option<String>,
    output: Option<String>,
    no_validate: bool,
    strict: bool,
    include_dotfiles: bool,
    verbose: bool,
    multi_platform: bool,
) -> ToolResult<()> {
    let dir = path
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    // Strict validation: treat warnings as errors
    if strict && !no_validate {
        let validation = validate_manifest(&dir);
        if !validation.is_strict_valid() {
            println!("  {} Validation failed (strict)\n", "✗".bright_red());

            for issue in validation.errors.iter().chain(validation.warnings.iter()) {
                println!(
                    "  {}: → {}",
                    format!("error[{}]", issue.code).bright_red().bold(),
                    issue.location.bold()
                );
                if let Some(help) = &issue.help {
                    println!("  · {}", issue.details.dimmed());
                    println!("  · {}: {}", "help".bright_green().dimmed(), help.dimmed());
                } else {
                    println!("  · {}", issue.details.dimmed());
                }
                println!();
            }

            let total = validation.errors.len() + validation.warnings.len();
            println!(
                "  {} {}",
                "✗".bright_red(),
                if total == 1 {
                    "1 error".to_string()
                } else {
                    format!("{} errors", total)
                }
            );
            println!("\n  Cannot pack with --strict. Fix errors and warnings, then retry.");
            std::process::exit(1);
        }
    }

    // Handle multi-platform packing
    if multi_platform {
        return pack_multi_platform(&dir, no_validate, include_dotfiles, verbose).await;
    }

    // Single bundle packing
    let options = PackOptions {
        output: output.map(PathBuf::from),
        validate: !no_validate,
        include_dotfiles,
        verbose,
    };

    match pack_bundle(&dir, &options) {
        Ok(result) => {
            print_pack_success(&result, !no_validate, verbose);
        }
        Err(e) => {
            return handle_pack_error(e);
        }
    }

    Ok(())
}

/// Pack bundles for each platform override + universal bundle.
async fn pack_multi_platform(
    dir: &Path,
    no_validate: bool,
    include_dotfiles: bool,
    verbose: bool,
) -> ToolResult<()> {
    // Load manifest to get platform overrides
    let manifest = McpbManifest::load(dir)
        .map_err(|e| ToolError::Generic(format!("Failed to load manifest: {}", e)))?;

    // Get platform overrides from _meta["store.tool.mcpb"] or server.mcp_config
    let platforms = get_platform_overrides(&manifest);

    if platforms.is_empty() {
        println!(
            "  {} No platform overrides found in manifest.",
            "⚠".bright_yellow()
        );
        println!("  Creating single universal bundle instead.");
        println!();

        // Fall back to single bundle
        let options = PackOptions {
            output: None,
            validate: !no_validate,
            include_dotfiles,
            verbose,
        };

        match pack_bundle(dir, &options) {
            Ok(result) => {
                print_pack_success(&result, !no_validate, verbose);
            }
            Err(e) => {
                return handle_pack_error(e);
            }
        }
        return Ok(());
    }

    let base_options = PackOptions {
        output: None,
        validate: !no_validate,
        include_dotfiles,
        verbose: false, // Suppress per-file output for parallel packing
    };

    // Pack all bundles in parallel using tokio::spawn
    let mut handles = Vec::new();

    // Pack platform-specific bundles
    for platform in platforms.clone() {
        let dir_clone = dir.to_path_buf();
        let opts = base_options.clone();
        let platform_clone = platform.clone();
        let handle = tokio::task::spawn_blocking(move || {
            pack_bundle_for_platform(&dir_clone, &opts, Some(&platform_clone))
        });
        handles.push((platform, handle));
    }

    // Pack universal bundle
    let dir_clone = dir.to_path_buf();
    let opts = base_options.clone();
    let universal_handle = tokio::task::spawn_blocking(move || pack_bundle(&dir_clone, &opts));

    // Wait for all packs to complete with spinner
    let bundle_count = platforms.len() + 1; // platforms + universal
    let spinner = Spinner::new(format!("Packing {} bundles", bundle_count));

    let mut results: Vec<(String, Result<PackResult, PackError>)> = Vec::new();
    for (platform, handle) in handles {
        let result = handle
            .await
            .map_err(|e| ToolError::Generic(format!("Task failed: {}", e)))?;
        results.push((platform, result));
    }

    let universal_result = universal_handle
        .await
        .map_err(|e| ToolError::Generic(format!("Task failed: {}", e)))?;

    spinner.succeed(Some("Bundles packed"));

    // Print results
    if !no_validate {
        println!("  {} Validation passed", "✓".bright_green());
    }

    let mut success_count = 0;
    let mut total_size = 0u64;

    for (platform, result) in &results {
        match result {
            Ok(pack_result) => {
                let path_display = pack_result.output_path.display().to_string();
                let colored_path = if pack_result.extension == "mcpbx" {
                    path_display.bright_yellow()
                } else {
                    path_display.bright_green()
                };
                println!(
                    "  {} Created {} ({}) [{}]",
                    "✓".bright_green(),
                    colored_path,
                    format_size(pack_result.compressed_size),
                    platform.bright_cyan()
                );
                success_count += 1;
                total_size += pack_result.compressed_size;
            }
            Err(e) => {
                println!(
                    "  {} Failed to pack for {}: {}",
                    "✗".bright_red(),
                    platform,
                    e
                );
            }
        }
    }

    // Print universal bundle result
    match universal_result {
        Ok(pack_result) => {
            let path_display = pack_result.output_path.display().to_string();
            let colored_path = if pack_result.extension == "mcpbx" {
                path_display.bright_yellow()
            } else {
                path_display.bright_green()
            };
            println!(
                "  {} Created {} ({}) [{}]",
                "✓".bright_green(),
                colored_path,
                format_size(pack_result.compressed_size),
                "universal".bright_cyan()
            );
            success_count += 1;
            total_size += pack_result.compressed_size;
        }
        Err(e) => {
            println!(
                "  {} Failed to pack universal bundle: {}",
                "✗".bright_red(),
                e
            );
        }
    }

    println!();
    println!(
        "  {} Created {} bundles (total: {})",
        "✓".bright_green(),
        success_count,
        format_size(total_size)
    );

    Ok(())
}

/// Check if a platform key is a valid OS-arch format (e.g., "darwin-arm64", "linux-x64").
/// OS-only keys like "darwin", "linux", "win32" are invalid for multi-platform packing.
fn is_valid_os_arch_platform(platform: &str) -> bool {
    // Valid formats: {os}-{arch} where os is darwin/linux/win32 and arch is arm64/x64/x86_64
    let valid_patterns = [
        "darwin-arm64",
        "darwin-x64",
        "darwin-x86_64",
        "linux-arm64",
        "linux-x64",
        "linux-x86_64",
        "win32-arm64",
        "win32-x64",
        "win32-x86_64",
    ];
    valid_patterns.contains(&platform)
}

/// Get platform overrides from manifest.
/// Checks _meta["store.tool.mcpb"].mcp_config.platform_overrides first,
/// then falls back to server.mcp_config.platform_overrides.
/// Only returns valid OS-arch platforms (e.g., "darwin-arm64"), not OS-only (e.g., "darwin").
fn get_platform_overrides(manifest: &McpbManifest) -> Vec<String> {
    let mut platforms = Vec::new();

    // Check _meta.store.tool.mcpb.mcp_config.platform_overrides
    if let Some(meta) = &manifest.meta
        && let Some(store_meta) = meta.get("store.tool.mcpb")
        && let Some(mcp_config) = store_meta.get("mcp_config")
        && let Some(overrides) = mcp_config.get("platform_overrides")
        && let Some(obj) = overrides.as_object()
    {
        // Only include valid OS-arch platforms
        platforms.extend(obj.keys().filter(|k| is_valid_os_arch_platform(k)).cloned());
        if !platforms.is_empty() {
            platforms.sort();
            platforms.dedup();
            return platforms;
        }
    }

    // Fall back to server.mcp_config.platform_overrides
    // These are typically OS-only, so we don't use them for multi-platform packing
    // (they're meant for runtime resolution, not for creating separate bundles)

    platforms.sort();
    platforms.dedup();
    platforms
}

/// Print success message for a pack result.
fn print_pack_success(result: &PackResult, validated: bool, verbose: bool) {
    if validated {
        println!("  {} Validation passed", "✓".bright_green());
    }

    if verbose {
        for ignored in &result.ignored_files {
            println!(
                "  {} {} {}",
                "-".dimmed(),
                ignored.dimmed(),
                "(ignored)".dimmed()
            );
        }
    }

    let path_display = result.output_path.display().to_string();
    let colored_path = if result.extension == "mcpbx" {
        path_display.bright_yellow()
    } else {
        path_display.bright_green()
    };
    println!(
        "  {} Created {} ({})",
        "✓".bright_green(),
        colored_path,
        format_size(result.compressed_size)
    );
    println!(
        "  · Files: {}, Compressed: {} (from {})",
        result.file_count,
        format_size(result.compressed_size),
        format_size(result.total_size)
    );
}

/// Handle pack errors with appropriate output.
fn handle_pack_error(e: PackError) -> ToolResult<()> {
    match e {
        PackError::ValidationFailed(validation) => {
            println!("  {} Validation failed\n", "✗".bright_red());

            for error in &validation.errors {
                println!(
                    "  {}: → {}",
                    format!("error[{}]", error.code).bright_red().bold(),
                    error.location.bold()
                );
                if let Some(help) = &error.help {
                    println!("  · {}", error.details.dimmed());
                    println!("  · {}: {}", "help".bright_green().dimmed(), help.dimmed());
                } else {
                    println!("  · {}", error.details.dimmed());
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
        PackError::ManifestNotFound(path) => {
            println!(
                "  {}: manifest.json not found in {}",
                "error".bright_red().bold(),
                path.display()
            );
            println!("  Run `tool init` to create one.");
            std::process::exit(1);
        }
        e => Err(ToolError::Generic(format!("Pack failed: {}", e))),
    }
}

/// Format byte size.
pub(super) fn format_size(bytes: u64) -> String {
    if bytes < 1_000 {
        format!("{} B", bytes)
    } else if bytes < 1_000_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}
