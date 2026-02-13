//! Registry search command handlers.

use crate::error::ToolResult;
use crate::format::format_description;
use crate::registry::RegistryClient;
use crate::styles::Spinner;
use colored::Colorize;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Search for tools in the registry.
pub async fn search_tools(query: &str, concise: bool, no_header: bool) -> ToolResult<()> {
    let client = RegistryClient::new();

    let results = if concise {
        client.search(query, Some(20)).await?
    } else {
        let spinner = Spinner::with_indent(format!("Searching for \"{}\"", query), 2);
        match client.search(query, Some(20)).await {
            Ok(results) => {
                if results.is_empty() {
                    spinner.fail(Some(&format!("No tools found matching: {}", query)));
                } else {
                    spinner.succeed(Some(&format!("Found {} tool(s)", results.len())));
                }
                results
            }
            Err(e) => {
                spinner.fail(Some("Search failed"));
                return Err(e);
            }
        }
    };

    if results.is_empty() {
        return Ok(());
    }

    // Concise output: Header + TSV format
    if concise {
        use crate::concise::quote;
        if !no_header {
            println!("#ref\tdescription\tdownloads");
        }
        for result in &results {
            let version_str = result
                .latest_version
                .as_ref()
                .map(|v| format!("@{}", v))
                .unwrap_or_default();
            let desc = result
                .description
                .as_deref()
                .and_then(|d| format_description(d, false, ""))
                .unwrap_or_default();
            println!(
                "{}/{}{}\t{}\t{}",
                result.namespace,
                result.name,
                version_str,
                quote(&desc),
                result.total_downloads
            );
        }
        return Ok(());
    }

    println!();
    for result in &results {
        let version_str = result
            .latest_version
            .as_ref()
            .map(|v| format!("@{}", v))
            .unwrap_or_default();

        println!(
            "  {}/{}{} {}",
            result.namespace.bright_blue(),
            result.name.bright_cyan(),
            version_str.dimmed(),
            format!("↓{}", result.total_downloads).dimmed()
        );

        if let Some(desc) = result
            .description
            .as_deref()
            .and_then(|d| format_description(d, false, ""))
        {
            println!("  · {}", desc.dimmed());
        }
        println!();
    }

    let install_ref = if results.len() == 1 {
        format!("{}/{}", results[0].namespace, results[0].name)
    } else {
        "<namespace>/<name>".to_string()
    };
    println!(
        "  · {} {}",
        "Install with:".dimmed(),
        format!("tool install {}", install_ref).bright_white()
    );

    Ok(())
}
