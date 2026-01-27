//! Tool pack command handlers.

use crate::error::{ToolError, ToolResult};
use crate::pack::{PackError, PackOptions, pack_bundle};
use crate::validate::validate_manifest;
use colored::Colorize;
use std::path::PathBuf;

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
