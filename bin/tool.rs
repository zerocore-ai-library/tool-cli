//! `tool` is the primary CLI binary.

use clap::{CommandFactory, Parser};
use colored::Colorize;
use tool_cli::handlers;
use tool_cli::tree::try_show_tree;
use tool_cli::{Cli, Command, SelfCommand, ToolError, ToolResult, self_update};
use tracing_subscriber::EnvFilter;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Initialize tracing - only enable when RUST_LOG is set.
    // This suppresses rmcp's tracing output by default.
    init_tracing();

    if let Err(e) = run().await {
        print_error(&e);
        std::process::exit(1);
    }
}

/// Print an error with appropriate formatting based on error type.
fn print_error(e: &ToolError) {
    println!();
    match e {
        ToolError::RegistryApi {
            code,
            message,
            status,
            ..
        } => {
            println!(
                "  {} {}",
                format!("error[{}]", code).bright_red().bold(),
                format!("(HTTP {})", status).dimmed()
            );
            println!();
            for line in message.split(", ") {
                println!("    {}", line);
            }
        }
        ToolError::EntryPointNotFound {
            entry_point,
            full_path,
            build_script,
            bundle_path,
        } => {
            println!("  {} Entry point not found", "error".bright_red().bold());
            println!();
            println!("    {}: {}", "Expected".dimmed(), entry_point);
            println!("    {}: {}", "Full path".dimmed(), full_path);
            println!("    {}: {}", "Bundle".dimmed(), bundle_path);
            if let Some(script) = build_script {
                println!();
                println!("    {}", "hint:".bright_blue().bold());
                println!("      Run build script first: {}", script.bright_white());
            }
        }
        ToolError::AmbiguousReference {
            requested,
            candidates,
            suggestion,
        } => {
            println!(
                "  {} Ambiguous reference '{}'",
                "error".bright_red().bold(),
                requested.bright_white()
            );
            println!();
            println!("    Found multiple matches:");
            for line in candidates.lines() {
                println!("    {}", line);
            }
            println!();
            println!("    {}: {}", "hint".bright_blue().bold(), suggestion);
        }
        ToolError::NotFound { kind, reference } => {
            println!(
                "  {} {} not found: {}",
                "error".bright_red().bold(),
                kind,
                reference.bright_white()
            );
        }
        ToolError::InvalidReference(msg) => {
            println!("  {} Invalid reference", "error".bright_red().bold());
            println!();
            println!("    {}", msg);
        }
        ToolError::AuthRequired { tool_ref } => {
            println!(
                "  {} OAuth authentication required",
                "error".bright_red().bold()
            );
            println!();
            println!(
                "    Tool '{}' requires OAuth authentication.",
                tool_ref.bright_white()
            );
            println!();
            println!(
                "    {}: Set {} environment variable",
                "hint".bright_blue().bold(),
                "CREDENTIALS_SECRET_KEY".bright_white()
            );
        }
        ToolError::OAuthNotConfigured => {
            println!("  {} OAuth not configured", "error".bright_red().bold());
            println!();
            println!(
                "    The {} environment variable is not set.",
                "CREDENTIALS_SECRET_KEY".bright_white()
            );
        }
        ToolError::ManifestNotFound(path) => {
            println!("  {} manifest.json not found", "error".bright_red().bold());
            println!();
            println!("    {}: {}", "Searched".dimmed(), path.display());
            println!();
            println!(
                "    {}: Run {} to create one",
                "hint".bright_blue().bold(),
                "tool init".bright_white()
            );
        }
        ToolError::ValidationFailed(result) => {
            println!("  {} Validation failed", "error".bright_red().bold());
            println!();
            for err in &result.errors {
                println!(
                    "    {} → {}",
                    format!("error[{}]", err.code).bright_red(),
                    err.location
                );
                println!("      {}", err.message);
            }
        }
        ToolError::Cancelled => {
            println!("  {} Operation cancelled", "✗".bright_red());
        }
        // For all other errors, use a consistent styled format
        _ => {
            let msg = e.to_string();
            // Check if it looks like a prefixed error (e.g., "IO error: ...")
            if let Some((prefix, rest)) = msg.split_once(": ") {
                if prefix.len() < 30 && !prefix.contains(' ') || prefix.ends_with("error") {
                    println!(
                        "  {} {}",
                        format!("error[{}]", prefix.to_lowercase().replace(" error", ""))
                            .bright_red()
                            .bold(),
                        rest.dimmed()
                    );
                } else {
                    println!("  {} {}", "error".bright_red().bold(), msg);
                }
            } else {
                println!("  {} {}", "error".bright_red().bold(), msg);
            }
        }
    }
    println!();
}

/// Initialize tracing. Only enables logging when RUST_LOG is set.
fn init_tracing() {
    // Check if RUST_LOG is set and non-empty
    let rust_log_set = std::env::var("RUST_LOG")
        .ok()
        .filter(|s| !s.is_empty())
        .is_some();

    // Only initialize tracing if RUST_LOG is set.
    // Without a subscriber, all tracing events (including from rmcp) are discarded.
    if !rust_log_set {
        return;
    }

    // Build filter from RUST_LOG, suppress rmcp logs unless explicitly included
    let base_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let env_filter = if std::env::var("RUST_LOG")
        .ok()
        .map(|s| s.contains("rmcp"))
        .unwrap_or(false)
    {
        base_filter
    } else {
        base_filter.add_directive("rmcp=off".parse().expect("valid directive"))
    };

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .without_time()
        .init();
}

async fn run() -> ToolResult<()> {
    // Check for --tree before parsing (avoids required argument errors)
    if let Some(tree) = try_show_tree(&Cli::command()) {
        println!("{}", tree);
        return Ok(());
    }

    let cli = Cli::parse();

    match cli.command {
        Command::Detect {
            path,
            entry,
            transport,
            name,
            verify,
            yes,
        } => {
            handlers::detect_mcpb(
                path,
                false,
                entry,
                transport,
                name,
                false,
                cli.concise,
                cli.no_header,
                verify,
                yes,
            )
            .await
        }

        Command::Init {
            path,
            name,
            server_type,
            description,
            author,
            license,
            http,
            reference,
            yes,
            package_manager,
            entry,
            transport,
            force,
            verify,
        } => {
            handlers::init_mcpb(
                path,
                name,
                server_type,
                description,
                author,
                license,
                http,
                reference,
                yes,
                package_manager,
                entry,
                transport,
                force,
                verify,
            )
            .await
        }

        Command::Validate {
            path,
            strict,
            json,
            quiet,
        } => handlers::validate_mcpb(path, strict, json, quiet).await,

        Command::Pack {
            path,
            output,
            no_validate,
            strict,
            include_dotfiles,
            verbose,
        } => {
            handlers::pack_mcpb(path, output, no_validate, strict, include_dotfiles, verbose).await
        }

        Command::Run {
            tool,
            expose,
            port,
            host,
            config,
            config_file,
            no_save,
            yes,
            verbose,
        } => {
            handlers::tool_run(
                tool,
                expose,
                port,
                host,
                config,
                config_file,
                no_save,
                yes,
                verbose,
            )
            .await
        }

        Command::Info {
            tool,
            methods,
            input_only,
            output_only,
            description_only,
            tools,
            prompts,
            resources,
            all,
            json,
            config,
            config_file,
            no_save,
            yes,
            verbose,
            level,
        } => {
            handlers::tool_info(
                tool,
                methods,
                input_only,
                output_only,
                description_only,
                tools,
                prompts,
                resources,
                all,
                json,
                config,
                config_file,
                no_save,
                yes,
                verbose,
                cli.concise,
                cli.no_header,
                level,
            )
            .await
        }

        Command::Call {
            tool,
            method,
            param,
            args,
            config,
            config_file,
            no_save,
            yes,
            verbose,
            json,
        } => {
            handlers::tool_call(
                tool,
                method,
                param,
                args,
                config,
                config_file,
                no_save,
                yes,
                verbose,
                json,
                cli.concise,
            )
            .await
        }

        Command::Config(cmd) => handlers::config_tool(cmd, cli.concise, cli.no_header).await,

        Command::Host(cmd) => handlers::handle_host_command(cmd, cli.concise, cli.no_header).await,

        Command::List { filter, json, full } => {
            handlers::list_tools(filter.as_deref(), json, full, cli.concise, cli.no_header).await
        }

        Command::Download { name, output } => {
            handlers::download_tool(&name, output.as_deref()).await
        }

        Command::Install { names } => handlers::add_tools(&names).await,

        Command::Uninstall { names } => handlers::remove_tools(&names).await,

        Command::Search { query } => {
            handlers::search_tools(&query, cli.concise, cli.no_header).await
        }

        Command::Publish {
            path,
            dry_run,
            strict,
        } => handlers::publish_mcpb(path.as_deref().unwrap_or("."), dry_run, strict).await,

        Command::Login { token } => handlers::auth_login(token.as_deref()).await,

        Command::Logout => handlers::auth_logout().await,

        Command::Whoami => handlers::auth_status(cli.concise, cli.no_header).await,

        Command::Grep {
            pattern,
            tool,
            method,
            input_only,
            output_only,
            name_only,
            description_only,
            ignore_case,
            list_only,
            json,
            level,
        } => {
            handlers::grep_tool(
                &pattern,
                tool,
                method,
                input_only,
                output_only,
                name_only,
                description_only,
                ignore_case,
                list_only,
                json,
                cli.concise,
                cli.no_header,
                level,
            )
            .await
        }

        Command::SelfCmd(subcmd) => match subcmd {
            SelfCommand::Update { check, version } => {
                if check {
                    let result = self_update::check_for_update().await?;
                    println!();
                    if result.update_available {
                        println!(
                            "  {} Update available: {} → {}",
                            "✓".bright_green(),
                            result.current.dimmed(),
                            result.latest.bright_cyan()
                        );
                        println!();
                        println!("  Run {} to update", "tool self update".bright_cyan());
                    } else {
                        println!(
                            "  {} Already up to date ({})",
                            "✓".bright_green(),
                            result.current.bright_cyan()
                        );
                    }
                    println!();
                    Ok(())
                } else {
                    self_update::self_update(version.as_deref()).await
                }
            }
            SelfCommand::Uninstall { yes } => self_update::self_uninstall(yes).await,
        },

        Command::External(args) => handlers::run_external_script(args).await,
    }
}
