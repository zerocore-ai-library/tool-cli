//! `tool` is the primary CLI binary.

use clap::{CommandFactory, Parser};
use tool_cli::handlers;
use tool_cli::tree::try_show_tree;
use tool_cli::{Cli, Command, ToolError, ToolResult};
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
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
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
        } => handlers::detect_mcpb(path, false, entry, transport, name, false).await,

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
            include_dotfiles,
            verbose,
        } => handlers::pack_mcpb(path, output, no_validate, include_dotfiles, verbose).await,

        Command::Run {
            script,
            path,
            list,
            args,
        } => {
            if list {
                // When --list is used, script arg (if any) is actually the path
                let effective_path = script.or(path);
                handlers::list_scripts(effective_path).await
            } else if let Some(script) = script {
                handlers::run_script(&script, path, args).await
            } else {
                Err(ToolError::Generic(
                    "Script name required. Use --list to see available scripts.".into(),
                ))
            }
        }

        Command::External(args) => handlers::run_external_script(args).await,

        Command::Info {
            tool,
            tools,
            prompts,
            resources,
            all,
            json,
            config,
            config_file,
            verbose,
        } => {
            handlers::tool_info(
                tool,
                tools,
                prompts,
                resources,
                all,
                json,
                config,
                config_file,
                verbose,
                cli.concise,
                cli.no_header,
            )
            .await
        }

        Command::Call {
            tool,
            method,
            param,
            config,
            config_file,
            verbose,
        } => {
            handlers::tool_call(
                tool,
                method,
                param,
                config,
                config_file,
                verbose,
                cli.concise,
            )
            .await
        }

        Command::List { filter, json } => {
            handlers::list_tools(filter.as_deref(), json, cli.concise, cli.no_header).await
        }

        Command::Download { name, output } => {
            handlers::download_tool(&name, output.as_deref()).await
        }

        Command::Install { name } => handlers::add_tool(&name).await,

        Command::Uninstall { name } => handlers::remove_tool(&name).await,

        Command::Search { query } => {
            handlers::search_tools(&query, cli.concise, cli.no_header).await
        }

        Command::Publish { path, dry_run } => {
            handlers::publish_mcpb(path.as_deref().unwrap_or("."), dry_run).await
        }

        Command::Login { token } => handlers::auth_login(token.as_deref()).await,

        Command::Logout => handlers::auth_logout().await,

        Command::Whoami => handlers::auth_status(cli.concise, cli.no_header).await,

        Command::Grep {
            pattern,
            tool,
            name_only,
            description_only,
            params_only,
            ignore_case,
            list_only,
            json,
        } => {
            handlers::grep_tool(
                &pattern,
                tool,
                name_only,
                description_only,
                params_only,
                ignore_case,
                list_only,
                json,
                cli.concise,
                cli.no_header,
            )
            .await
        }
    }
}
