//! Registry command handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcpb::McpbManifest;
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use crate::resolver::FilePluginResolver;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::Arc;

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

    // Determine version and get bundle size from artifact metadata
    println!(
        "  {} Resolving {}...",
        "→".bright_blue(),
        plugin_ref.to_string().bright_cyan()
    );

    let artifact = client.get_artifact(namespace, tool_name).await?;
    let version_info = artifact.latest_version.ok_or_else(|| {
        ToolError::Generic(format!(
            "No versions published for {}/{}",
            namespace, tool_name
        ))
    })?;

    let version = if let Some(v) = plugin_ref.version_str() {
        v.to_string()
    } else {
        version_info.version.clone()
    };

    // Get bundle size from version info (may be None for older versions)
    let bundle_size = version_info.bundle_size.unwrap_or(0);

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

    // Create progress bar with known bundle size
    let pb = ProgressBar::new(bundle_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("    [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap()
            .progress_chars("█░░"),
    );

    let pb_clone = pb.clone();
    let download_size = client
        .download_artifact_with_progress(
            namespace,
            tool_name,
            &version,
            &output_path,
            move |downloaded, total| {
                // Use Content-Length if available and bundle_size was 0
                if total > 0 && pb_clone.length() == Some(0) {
                    pb_clone.set_length(total);
                }
                pb_clone.set_position(downloaded);
            },
        )
        .await?;

    pb.finish_and_clear();
    println!(
        "  {} Downloaded {} ({})",
        "✓".bright_green(),
        output_path.display().to_string().dimmed(),
        format_size(download_size)
    );

    Ok(())
}

/// Add a tool from the registry or a local path.
pub async fn add_tool(name: &str) -> ToolResult<()> {
    use crate::constants::DEFAULT_TOOLS_PATH;

    // Check if this looks like a local path
    if is_local_path(name) {
        return install_local_tool(name).await;
    }

    let plugin_ref = name
        .parse::<PluginRef>()
        .map_err(|e| ToolError::Generic(format!("Invalid tool reference '{}': {}", name, e)))?;

    // Check if it has a namespace (required for registry fetch)
    let namespace = match plugin_ref.namespace() {
        Some(ns) => ns,
        None => {
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
    };

    let tool_name = plugin_ref.name();

    println!(
        "  {} Installing {} from registry...",
        "→".bright_blue(),
        name.bright_cyan()
    );

    // Get artifact details from registry
    let client = RegistryClient::new();
    let artifact = match client.get_artifact(namespace, tool_name).await {
        Ok(a) => a,
        Err(_) => {
            println!(
                "  {} Tool {} not found in registry",
                "✗".bright_red(),
                name.bright_white().bold()
            );
            return Ok(());
        }
    };

    // Get version info
    let version_info = artifact
        .latest_version
        .ok_or_else(|| ToolError::Generic(format!("No published version found for {}", name)))?;
    let version = &version_info.version;
    let bundle_size = version_info.bundle_size.unwrap_or(0);

    // Check if already installed
    let target_dir = DEFAULT_TOOLS_PATH
        .join(namespace)
        .join(format!("{}@{}", tool_name, version));

    if target_dir.join(MCPB_MANIFEST_FILE).exists() {
        println!(
            "  {} Already installed {}/{}@{}",
            "✓".bright_green(),
            namespace.bright_cyan(),
            tool_name.bright_cyan(),
            version.bright_cyan()
        );
        return Ok(());
    }

    // Create temp file for download
    let temp_file =
        std::env::temp_dir().join(format!("tool-{}-{}-{}.zip", namespace, tool_name, version));

    // Create progress bar
    let pb = ProgressBar::new(bundle_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("    [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap()
            .progress_chars("█░░"),
    );

    // Download with progress
    let pb_clone = pb.clone();
    client
        .download_artifact_with_progress(
            namespace,
            tool_name,
            version,
            &temp_file,
            move |downloaded, total| {
                if total > 0 && pb_clone.length() == Some(0) {
                    pb_clone.set_length(total);
                }
                pb_clone.set_position(downloaded);
            },
        )
        .await?;

    pb.finish_and_clear();

    // Create target directory
    tokio::fs::create_dir_all(&target_dir).await.map_err(|e| {
        ToolError::Generic(format!(
            "Failed to create tool directory {:?}: {}",
            target_dir, e
        ))
    })?;

    // Extract the bundle
    extract_bundle(&temp_file, &target_dir)?;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_file);

    println!(
        "  {} Installed {}/{}@{} to {}",
        "✓".bright_green(),
        namespace.bright_cyan(),
        tool_name.bright_cyan(),
        version.bright_cyan(),
        target_dir.display()
    );

    Ok(())
}

/// Extract a ZIP bundle to a directory.
fn extract_bundle(bundle_path: &std::path::Path, target_dir: &std::path::Path) -> ToolResult<()> {
    use std::io::Read;
    use zip::ZipArchive;

    let file = std::fs::File::open(bundle_path)
        .map_err(|e| ToolError::Generic(format!("Failed to open bundle: {}", e)))?;

    let mut archive = ZipArchive::new(file)
        .map_err(|e| ToolError::Generic(format!("Failed to read ZIP archive: {}", e)))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| ToolError::Generic(format!("Failed to read archive entry: {}", e)))?;

        let entry_path = entry
            .enclosed_name()
            .ok_or_else(|| ToolError::Generic("Invalid entry path in archive".into()))?;

        let dest_path = target_dir.join(entry_path);

        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolError::Generic(format!("Failed to create directory {:?}: {}", parent, e))
            })?;
        }

        #[cfg(unix)]
        let unix_mode = entry.unix_mode();

        if entry.is_dir() {
            std::fs::create_dir_all(&dest_path).map_err(|e| {
                ToolError::Generic(format!("Failed to create directory {:?}: {}", dest_path, e))
            })?;
        } else {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| ToolError::Generic(format!("Failed to read entry content: {}", e)))?;

            std::fs::write(&dest_path, &content).map_err(|e| {
                ToolError::Generic(format!("Failed to write file {:?}: {}", dest_path, e))
            })?;

            #[cfg(unix)]
            if let Some(mode) = unix_mode {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::Permissions::from_mode(mode);
                std::fs::set_permissions(&dest_path, permissions).map_err(|e| {
                    ToolError::Generic(format!(
                        "Failed to set permissions on {:?}: {}",
                        dest_path, e
                    ))
                })?;
            }
        }
    }

    Ok(())
}

/// Check if the input looks like a local path rather than a registry reference.
fn is_local_path(input: &str) -> bool {
    // Explicit path indicators
    input.starts_with('.')
        || input.starts_with('/')
        || input.starts_with('~')
        // Windows absolute paths
        || (input.len() >= 2 && input.chars().nth(1) == Some(':'))
        // Check if it's an existing directory with manifest.json
        || PathBuf::from(input).join(MCPB_MANIFEST_FILE).exists()
}

/// Install a tool from a local path by creating a symlink.
async fn install_local_tool(path: &str) -> ToolResult<()> {
    use crate::constants::DEFAULT_TOOLS_PATH;
    use crate::mcpb::McpbManifest;

    // Resolve the path
    let source_path = if path.starts_with('~') {
        dirs::home_dir()
            .ok_or_else(|| ToolError::Generic("Could not determine home directory".into()))?
            .join(&path[2..])
    } else {
        PathBuf::from(path)
    };

    let source_path = source_path
        .canonicalize()
        .map_err(|_| ToolError::Generic(format!("Path not found: {}", path)))?;

    // Check for manifest.json
    let manifest_path = source_path.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(ToolError::Generic(format!(
            "No {} found in {}. Run `tool init` first.",
            MCPB_MANIFEST_FILE,
            source_path.display()
        )));
    }

    // Load manifest to get name and version
    let manifest = McpbManifest::load(&source_path)?;
    let tool_name = manifest
        .name
        .as_ref()
        .ok_or_else(|| ToolError::Generic("manifest.json must include a name field".into()))?;
    let version = manifest.version.as_ref();

    // Build target directory name
    let target_name = match version {
        Some(v) => format!("{}@{}", tool_name, v),
        None => tool_name.clone(),
    };

    let target_path = DEFAULT_TOOLS_PATH.join(&target_name);

    println!(
        "  {} Linking {} from {}...",
        "→".bright_blue(),
        target_name.bright_cyan(),
        source_path.display().to_string().dimmed()
    );

    // Check if target already exists
    if target_path.exists() || target_path.is_symlink() {
        // Check if it's already linked to the same source
        if target_path.is_symlink()
            && let Ok(existing_target) = std::fs::read_link(&target_path)
            && existing_target == source_path
        {
            println!(
                "  {} Already linked {}",
                "✓".bright_green(),
                target_name.bright_cyan()
            );
            return Ok(());
        }

        // Remove existing (symlink or directory)
        if target_path.is_symlink() || target_path.is_file() {
            std::fs::remove_file(&target_path).map_err(|e| {
                ToolError::Generic(format!("Failed to remove existing link: {}", e))
            })?;
        } else {
            std::fs::remove_dir_all(&target_path).map_err(|e| {
                ToolError::Generic(format!("Failed to remove existing directory: {}", e))
            })?;
        }
    }

    // Ensure parent directory exists
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ToolError::Generic(format!("Failed to create tools directory: {}", e)))?;
    }

    // Create symlink
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_path, &target_path)
        .map_err(|e| ToolError::Generic(format!("Failed to create symlink: {}", e)))?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source_path, &target_path)
        .map_err(|e| ToolError::Generic(format!("Failed to create symlink: {}", e)))?;

    println!(
        "  {} Installed {} {}",
        "✓".bright_green(),
        target_name.bright_cyan(),
        "(linked)".dimmed()
    );

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

        if let Some(desc) = result
            .description
            .as_deref()
            .and_then(|d| format_description(d, false, ""))
        {
            println!("      {}", desc.dimmed());
        }
    }

    println!();
    println!(
        "    {} {}",
        "Install with:".dimmed(),
        "tool install <namespace>/<name>".bright_white()
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

    // Create progress bar for upload
    let pb = ProgressBar::new(bundle_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("    [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap()
            .progress_chars("█░░"),
    );

    // Upload to presigned URL with progress
    let pb_arc = Arc::new(pb);
    let pb_clone = Arc::clone(&pb_arc);
    client
        .upload_bundle_with_progress(&upload_info.upload_url, &bundle, move |bytes| {
            pb_clone.set_position(bytes);
        })
        .await?;

    pb_arc.finish_and_clear();
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
