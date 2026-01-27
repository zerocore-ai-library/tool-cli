//! Tool validation command handlers.

use crate::error::ToolResult;
use crate::mcpb::McpbManifest;
use crate::validate::{ValidationResult, validate_manifest};
use colored::Colorize;
use std::path::PathBuf;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

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
    let is_mcpbx = McpbManifest::load(&dir)
        .map(|m| m.requires_mcpbx())
        .unwrap_or(false);

    if json_output {
        output_json(&result, format_name, is_mcpbx)?;
        return check_exit_status(&result, strict);
    }

    if quiet {
        output_quiet(&result);
    } else {
        output_full(&result, strict, format_name, is_mcpbx);
    }

    check_exit_status(&result, strict)
}

/// Output validation result as JSON.
fn output_json(result: &ValidationResult, format_name: &str, is_mcpbx: bool) -> ToolResult<()> {
    let output = serde_json::json!({
        "bundle_format": if is_mcpbx { "mcpbx" } else { "mcpb" },
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
fn output_full(result: &ValidationResult, strict: bool, format_name: &str, is_mcpbx: bool) {
    let format_display = if is_mcpbx {
        "mcpbx".bright_yellow()
    } else {
        "mcpb".bright_green()
    };
    println!("  Validating {} ({})\n", format_name.bold(), format_display);

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
