//! Registry command handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::mcpb::McpbManifest;
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use crate::resolver::FilePluginResolver;
use colored::Colorize;
use std::path::PathBuf;

use super::pack_cmd::format_size;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Download a tool from the registry.
pub async fn download_tool(name: &str, output: Option<&str>) -> ToolResult<()> {
    // Parse tool reference
    let plugin_ref = name
        .parse::<PluginRef>()
        .map_err(|e| ToolError::Generic(format!("Invalid tool reference '{}': {}", name, e)))?;

    if plugin_ref.namespace().is_none() {
        println!(
            "  {} Tool reference must include a namespace for registry fetch",
            "✗".bright_red()
        );
        println!(
            "    Example: {} or {}",
            "namespace/tool".bright_cyan(),
            "namespace/tool@1.0.0".bright_cyan()
        );
        return Ok(());
    }

    let namespace = plugin_ref.namespace().unwrap();
    let tool_name = plugin_ref.name();

    // Create registry client
    let client = RegistryClient::new();

    // Determine version - use specified or fetch latest
    let version = if let Some(v) = plugin_ref.version_str() {
        v.to_string()
    } else {
        println!(
            "  {} Resolving {}...",
            "→".bright_blue(),
            plugin_ref.to_string().bright_cyan()
        );

        let artifact = client.get_artifact(namespace, tool_name).await?;
        artifact.latest_version.map(|v| v.version).ok_or_else(|| {
            ToolError::Generic(format!(
                "No versions published for {}/{}",
                namespace, tool_name
            ))
        })?
    };

    // Determine output path
    let bundle_name = format!("{}@{}.mcpb", tool_name, version);
    let output_path = match output {
        Some(p) => {
            let path = PathBuf::from(p);
            if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            }
        }
        None => std::env::current_dir()?.join(&bundle_name),
    };

    // Create parent directory if it doesn't exist
    if let Some(parent) = output_path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }

    let download_ref = format!("{}/{}@{}", namespace, tool_name, version);
    println!(
        "  {} Downloading {}...",
        "→".bright_blue(),
        download_ref.bright_cyan()
    );

    let download_size = client
        .download_artifact(namespace, tool_name, &version, &output_path)
        .await?;

    println!(
        "  {} Downloaded {} ({})",
        "✓".bright_green(),
        output_path.display().to_string().dimmed(),
        format_size(download_size)
    );

    Ok(())
}

/// Add a tool from the registry.
pub async fn add_tool(name: &str) -> ToolResult<()> {
    let plugin_ref = name
        .parse::<PluginRef>()
        .map_err(|e| ToolError::Generic(format!("Invalid tool reference '{}': {}", name, e)))?;

    // Check if it has a namespace (required for registry fetch)
    if plugin_ref.namespace().is_none() {
        println!(
            "  {} Tool reference must include a namespace for registry fetch",
            "✗".bright_red()
        );
        println!(
            "    Example: {} or {}",
            "appcypher/filesystem".bright_cyan(),
            "myorg/mytool".bright_cyan()
        );
        return Ok(());
    }

    println!(
        "  {} Adding tool {} from registry...",
        "→".bright_blue(),
        name.bright_cyan()
    );

    // Create resolver with auto-install enabled
    let client = RegistryClient::new();
    let resolver = FilePluginResolver::default().with_auto_install(client);

    // Resolve will trigger auto-install if not found locally
    match resolver.resolve_tool(name).await? {
        Some(resolved) => {
            println!(
                "  {} Installed {} to {}",
                "✓".bright_green(),
                resolved.plugin_ref.to_string().bright_cyan(),
                resolved.path.parent().unwrap_or(&resolved.path).display()
            );
        }
        None => {
            println!(
                "  {} Tool {} not found in registry",
                "✗".bright_red(),
                name.bright_white().bold()
            );
        }
    }

    Ok(())
}

/// Remove an installed tool.
pub async fn remove_tool(name: &str) -> ToolResult<()> {
    use tokio::fs;

    let resolver = FilePluginResolver::default();

    // First, find the tool
    match resolver.resolve_tool(name).await? {
        Some(resolved) => {
            // Get the directory containing the tool
            let tool_dir = resolved
                .path
                .parent()
                .ok_or_else(|| ToolError::Generic("Failed to get tool directory".into()))?;

            println!(
                "  {} Removing tool {}...",
                "→".bright_blue(),
                resolved.plugin_ref.to_string().bright_cyan()
            );

            // Remove the directory
            fs::remove_dir_all(tool_dir).await.map_err(|e| {
                ToolError::Generic(format!("Failed to remove tool directory: {}", e))
            })?;

            println!(
                "  {} Removed {}",
                "✓".bright_green(),
                resolved.plugin_ref.to_string().bright_cyan()
            );
        }
        None => {
            println!(
                "  {} Tool {} not found",
                "✗".bright_red(),
                name.bright_white().bold()
            );
        }
    }

    Ok(())
}

/// Search for tools in the registry.
pub async fn search_tools(query: &str, concise: bool, no_header: bool) -> ToolResult<()> {
    let client = RegistryClient::new();

    if !concise {
        println!(
            "  {} Searching registry for tools matching: {}",
            "→".bright_blue(),
            query.bright_cyan()
        );
    }

    let results = client.search(query, Some(20)).await?;

    if results.is_empty() {
        if !concise {
            println!(
                "  {} No tools found matching: {}",
                "✗".bright_red(),
                query.bright_white().bold()
            );
        }
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
            let desc = result.description.as_deref().unwrap_or("");
            println!(
                "{}/{}{}\t{}\t{}",
                result.namespace,
                result.name,
                version_str,
                quote(desc),
                result.total_downloads
            );
        }
        return Ok(());
    }

    let label = if results.len() == 1 { "tool" } else { "tools" };
    println!(
        "\n  {} Found {} {}\n",
        "✓".bright_green(),
        results.len().to_string().bold(),
        label
    );

    for result in &results {
        let version_str = result
            .latest_version
            .as_ref()
            .map(|v| format!("@{}", v))
            .unwrap_or_default();

        println!(
            "    {}/{}{} {}",
            result.namespace.bright_blue(),
            result.name.bright_cyan(),
            version_str.dimmed(),
            format!("↓{}", result.total_downloads).dimmed()
        );

        if let Some(desc) = &result.description {
            println!("      {}", desc.dimmed());
        }
    }

    println!();
    println!(
        "    {} {}",
        "Install with:".dimmed(),
        "tool add <namespace>/<name>".bright_white()
    );

    Ok(())
}

/// Publish a tool to the registry.
pub async fn publish_mcpb(path: &str, dry_run: bool) -> ToolResult<()> {
    use crate::handlers::auth::{get_registry_token, load_credentials};
    use crate::pack::{PackOptions, pack_bundle};
    use sha2::{Digest, Sha256};

    // Resolve the directory
    let dir = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| ToolError::Generic(format!("Directory not found: {}", path)))?;

    // Check manifest exists
    let manifest_path = dir.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(ToolError::Generic(format!(
            "manifest.json not found in {}. Run `tool init` first.",
            dir.display()
        )));
    }

    // Read manifest
    let manifest_content = std::fs::read_to_string(&manifest_path)?;
    let manifest: McpbManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| ToolError::Generic(format!("Failed to parse manifest.json: {}", e)))?;

    let tool_name = manifest
        .name
        .as_ref()
        .ok_or_else(|| ToolError::Generic("manifest.json must include a name field".into()))?;

    let version = manifest
        .version
        .as_ref()
        .ok_or_else(|| ToolError::Generic("manifest.json must include a version field".into()))?;

    // Validate version is semver
    if semver::Version::parse(version).is_err() {
        return Err(ToolError::Generic(format!(
            "Version '{}' is not valid semver (expected format: x.y.z)",
            version
        )));
    }

    // Get authenticated user
    let (namespace, token) = if dry_run {
        let creds = load_credentials().await?.map(|c| (c.username, c.token));
        match creds {
            Some((username, token)) => (username, Some(token)),
            None => ("<your-username>".to_string(), None),
        }
    } else {
        let token = get_registry_token().await?.ok_or_else(|| {
            ToolError::Generic("Authentication required. Run `tool login` first.".into())
        })?;
        let client = RegistryClient::new().with_auth_token(&token);
        let user = client.validate_token().await?;
        (user.username, Some(token))
    };

    let description = manifest.description.as_deref();

    if dry_run {
        println!(
            "  {} Dry run: validating tool {}/{}...",
            "→".bright_blue(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    } else {
        println!(
            "  {} Publishing tool {}/{}...",
            "→".bright_blue(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    }

    println!("    {}: {}", "Version".dimmed(), version.bright_white());
    println!(
        "    {}: {}",
        "Source".dimmed(),
        dir.display().to_string().dimmed()
    );
    if let Some(desc) = description {
        println!("    {}: {}", "Description".dimmed(), desc.dimmed());
    }

    // Bundle the tool
    println!("\n    {} Creating bundle...", "→".bright_blue());
    let pack_options = PackOptions {
        validate: true,
        output: None,
        verbose: false,
        include_dotfiles: false,
    };
    let pack_result = pack_bundle(&dir, &pack_options)
        .map_err(|e| ToolError::Generic(format!("Pack failed: {}", e)))?;

    // Read the bundle
    let bundle = std::fs::read(&pack_result.output_path)
        .map_err(|e| ToolError::Generic(format!("Failed to read bundle: {}", e)))?;
    let bundle_size = bundle.len() as u64;
    println!(
        "    Bundle size: {}",
        format_size(bundle_size).bright_white()
    );

    // Clean up temp bundle
    let _ = std::fs::remove_file(&pack_result.output_path);

    if dry_run {
        println!(
            "\n  {} Dry run complete. Would publish {}/{}@{}",
            "✓".bright_green(),
            namespace.bright_blue(),
            tool_name.bright_cyan(),
            version.bright_white()
        );
        return Ok(());
    }

    // Create registry client with auth
    let token = token.unwrap();
    let client = RegistryClient::new().with_auth_token(&token);

    // Check if artifact exists
    println!(
        "\n    {} Checking registry ({})...",
        "→".bright_blue(),
        client.registry_url().bright_white()
    );
    if !client.artifact_exists(&namespace, tool_name).await? {
        println!("    Creating new artifact entry...");
        client
            .create_artifact(&namespace, tool_name, description)
            .await?;
        println!(
            "    {} Created {}/{}",
            "✓".bright_green(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    }

    // Compute SHA-256
    let mut hasher = Sha256::new();
    hasher.update(&bundle);
    let sha256 = format!("{:x}", hasher.finalize());

    // Initiate upload
    println!("\n    {} Uploading bundle...", "→".bright_blue());
    let upload_info = client
        .init_upload(&namespace, tool_name, version, bundle_size, &sha256)
        .await?;

    // Upload to presigned URL
    client
        .upload_bundle(&upload_info.upload_url, &bundle)
        .await?;
    println!("    {} Upload complete", "✓".bright_green());

    // Publish the version
    println!("\n    {} Publishing version...", "→".bright_blue());

    let manifest_json: serde_json::Value = serde_json::from_str(&manifest_content)?;

    let result = client
        .publish_version(
            &namespace,
            tool_name,
            &upload_info.upload_id,
            version,
            manifest_json,
            description,
        )
        .await?;

    println!(
        "\n  {} Published {}/{}@{}",
        "✓".bright_green(),
        namespace.bright_blue(),
        tool_name.bright_cyan(),
        result.version.bright_white()
    );
    println!(
        "    {}/plugins/{}/{}",
        client.registry_url(),
        namespace,
        tool_name
    );

    Ok(())
}
