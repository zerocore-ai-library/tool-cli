//! Registry command handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcpb::McpbManifest;
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use crate::resolver::FilePluginResolver;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinHandle;

use super::pack_cmd::format_size;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Result of a single tool installation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum InstallResult {
    /// Successfully installed from registry
    InstalledRegistry,
    /// Successfully linked from local path
    InstalledLocal,
    /// Tool was already installed
    AlreadyInstalled,
    /// Installation failed with error message
    Failed(String),
}

/// Result of a single tool uninstallation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum UninstallResult {
    /// Successfully removed
    Removed,
    /// Tool not found
    NotFound,
    /// Removal failed
    Failed(String),
}

/// Pre-flight information for a registry download.
#[allow(dead_code)]
struct RegistryPreflight {
    name: String,
    namespace: String,
    tool_name: String,
    version: String,
    bundle_size: u64,
    target_dir: PathBuf,
    temp_file: PathBuf,
}

/// Result of pre-flight check.
enum PreflightResult {
    /// Ready for registry download
    Registry(RegistryPreflight),
    /// Local install (already handled)
    Local(InstallResult),
    /// Already installed
    AlreadyInstalled,
    /// Pre-flight failed
    Failed(String),
}

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

    // Use bundle_format from API response, default to "mcpb" for older versions
    let ext = version_info.bundle_format.as_deref().unwrap_or("mcpb");

    // Determine output path
    let bundle_name = format!("{}@{}.{}", tool_name, version, ext);
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
    let path_str = output_path.display().to_string();
    let colored_path = if path_str.ends_with(".mcpbx") {
        path_str.bright_yellow()
    } else {
        path_str.bright_green()
    };
    println!(
        "  {} Downloaded {} ({})",
        "✓".bright_green(),
        colored_path,
        format_size(download_size)
    );

    Ok(())
}

/// Run pre-flight checks for a tool (validation, metadata fetch, already-installed check).
async fn preflight_tool(name: &str) -> PreflightResult {
    use crate::constants::DEFAULT_TOOLS_PATH;

    // Check if this looks like a local path
    if is_local_path(name) {
        return PreflightResult::Local(install_local_tool(name).await);
    }

    let plugin_ref = match name.parse::<PluginRef>() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("Invalid tool reference '{}': {}", name, e);
            return PreflightResult::Failed(msg);
        }
    };

    // Check if it has a namespace (required for registry fetch)
    let namespace = match plugin_ref.namespace() {
        Some(ns) => ns.to_string(),
        None => {
            return PreflightResult::Failed(format!(
                "{}: missing namespace (use namespace/name format)",
                name
            ));
        }
    };

    let tool_name = plugin_ref.name().to_string();

    // Get artifact details from registry
    let client = RegistryClient::new();
    let artifact = match client.get_artifact(&namespace, &tool_name).await {
        Ok(a) => a,
        Err(_) => {
            return PreflightResult::Failed(format!("Tool {} not found in registry", name));
        }
    };

    // Get version info
    let version_info = match artifact.latest_version {
        Some(v) => v,
        None => {
            return PreflightResult::Failed(format!("No published version for {}", name));
        }
    };
    let version = version_info.version.clone();
    let bundle_size = version_info.bundle_size.unwrap_or(0);

    // Check if already installed
    let target_dir = DEFAULT_TOOLS_PATH
        .join(&namespace)
        .join(format!("{}@{}", tool_name, version));

    if target_dir.join(MCPB_MANIFEST_FILE).exists() {
        return PreflightResult::AlreadyInstalled;
    }

    // Create temp file path for download
    let temp_file =
        std::env::temp_dir().join(format!("tool-{}-{}-{}.zip", namespace, tool_name, version));

    PreflightResult::Registry(RegistryPreflight {
        name: name.to_string(),
        namespace,
        tool_name,
        version,
        bundle_size,
        target_dir,
        temp_file,
    })
}

/// Download and install a tool with a progress bar.
async fn download_and_install(preflight: RegistryPreflight, pb: ProgressBar) -> InstallResult {
    let client = RegistryClient::new();

    // Style for finished state (clean, aligned)
    let finish_style = ProgressStyle::default_bar().template("  {msg}").unwrap();

    // Download with progress
    let pb_clone = pb.clone();
    if let Err(e) = client
        .download_artifact_with_progress(
            &preflight.namespace,
            &preflight.tool_name,
            &preflight.version,
            &preflight.temp_file,
            move |downloaded, total| {
                if total > 0 && pb_clone.length() == Some(0) {
                    pb_clone.set_length(total);
                }
                pb_clone.set_position(downloaded);
            },
        )
        .await
    {
        pb.set_style(finish_style);
        pb.finish_with_message(format!(
            "{} {:<30} Failed to download",
            "✗".bright_red(),
            format!("{}/{}", preflight.namespace, preflight.tool_name)
        ));
        return InstallResult::Failed(format!("Failed to download: {}", e));
    }

    pb.set_message(format!("{}/{}", preflight.namespace, preflight.tool_name));

    // Create target directory
    if let Err(e) = tokio::fs::create_dir_all(&preflight.target_dir).await {
        pb.set_style(finish_style);
        pb.finish_with_message(format!(
            "{} {:<30} Failed to create directory",
            "✗".bright_red(),
            format!("{}/{}", preflight.namespace, preflight.tool_name)
        ));
        return InstallResult::Failed(format!("Failed to create directory: {}", e));
    }

    // Extract the bundle
    if let Err(e) = extract_bundle(&preflight.temp_file, &preflight.target_dir) {
        pb.set_style(finish_style);
        pb.finish_with_message(format!(
            "{} {:<30} Failed to extract",
            "✗".bright_red(),
            format!("{}/{}", preflight.namespace, preflight.tool_name)
        ));
        return InstallResult::Failed(format!("Failed to extract: {}", e));
    }

    // Clean up temp file
    let _ = std::fs::remove_file(&preflight.temp_file);

    // Determine format display
    let is_mcpbx = McpbManifest::load(&preflight.target_dir)
        .map(|m| m.requires_mcpbx())
        .unwrap_or(false);
    let format_tag = if is_mcpbx { "mcpbx" } else { "mcpb" };
    let format_display = if is_mcpbx {
        format_tag.bright_yellow()
    } else {
        format_tag.bright_green()
    };

    pb.set_style(finish_style);
    pb.finish_with_message(format!(
        "{} {}/{}@{} ({})",
        "✓".bright_green(),
        preflight.namespace.bright_cyan(),
        preflight.tool_name.bright_cyan(),
        preflight.version.bright_cyan(),
        format_display
    ));

    InstallResult::InstalledRegistry
}

/// Install multiple tools from the registry or local paths.
pub async fn add_tools(names: &[String]) -> ToolResult<()> {
    use futures_util::future::join_all;

    let mut registry_count = 0usize;
    let mut local_count = 0usize;
    let mut already_count = 0usize;
    let mut failed_count = 0usize;

    // Phase 1: Run preflight checks concurrently
    let preflight_futures: Vec<_> = names.iter().map(|name| preflight_tool(name)).collect();
    let preflight_results = join_all(preflight_futures).await;

    // Separate registry downloads from immediate results
    let mut registry_preflights = Vec::new();
    let mut immediate_messages = Vec::new();

    for (name, result) in names.iter().zip(preflight_results) {
        match result {
            PreflightResult::Registry(preflight) => {
                registry_preflights.push(preflight);
            }
            PreflightResult::Local(install_result) => match install_result {
                InstallResult::InstalledLocal => local_count += 1,
                InstallResult::AlreadyInstalled => already_count += 1,
                InstallResult::Failed(msg) => {
                    immediate_messages.push(format!("  {} {}: {}", "✗".bright_red(), name, msg));
                    failed_count += 1;
                }
                _ => {}
            },
            PreflightResult::AlreadyInstalled => {
                immediate_messages.push(format!(
                    "  {} Already installed {}",
                    "✓".bright_green(),
                    name.bright_cyan()
                ));
                already_count += 1;
            }
            PreflightResult::Failed(msg) => {
                immediate_messages.push(format!("  {} {}", "✗".bright_red(), msg));
                failed_count += 1;
            }
        }
    }

    // Print immediate messages (local installs, already installed, preflight failures)
    for msg in &immediate_messages {
        println!("{}", msg);
    }

    // Phase 2: Download registry packages in parallel with progress bars
    if !registry_preflights.is_empty() {
        let mp = MultiProgress::new();
        let style = ProgressStyle::default_bar()
            .template(
                "  {msg:<30} [{bar:25.cyan/dim}] {bytes:>10}/{total_bytes:<10} {bytes_per_sec:>12}",
            )
            .unwrap()
            .progress_chars("█░░");

        // Create progress bars and spawn download tasks
        let handles: Vec<JoinHandle<InstallResult>> = registry_preflights
            .into_iter()
            .map(|preflight| {
                let pb = mp.add(ProgressBar::new(preflight.bundle_size));
                pb.set_style(style.clone());
                pb.set_message(format!("{}/{}", preflight.namespace, preflight.tool_name));

                tokio::spawn(async move { download_and_install(preflight, pb).await })
            })
            .collect();

        // Wait for all downloads to complete
        let results = join_all(handles).await;

        for result in results {
            match result {
                Ok(InstallResult::InstalledRegistry) => registry_count += 1,
                Ok(InstallResult::Failed(_)) => failed_count += 1,
                Err(_) => failed_count += 1, // Task panicked
                _ => {}
            }
        }
    }

    // Print summary if multiple packages were requested
    if names.len() > 1 {
        println!();
        let total_installed = registry_count + local_count;
        let mut parts = Vec::new();

        if registry_count > 0 {
            parts.push(format!("{} from registry", registry_count));
        }
        if local_count > 0 {
            parts.push(format!("{} linked", local_count));
        }

        if total_installed > 0 {
            let details = if parts.is_empty() {
                String::new()
            } else {
                format!(" ({})", parts.join(", "))
            };
            println!(
                "Installed {} {}{details}",
                total_installed.to_string().bright_green(),
                if total_installed == 1 {
                    "package"
                } else {
                    "packages"
                }
            );
        }

        if already_count > 0 {
            println!(
                "Skipped {} already installed",
                already_count.to_string().bright_cyan()
            );
        }

        if failed_count > 0 {
            println!(
                "Failed {} {}",
                failed_count.to_string().bright_red(),
                if failed_count == 1 {
                    "package"
                } else {
                    "packages"
                }
            );
        }
    }

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
async fn install_local_tool(path: &str) -> InstallResult {
    use crate::constants::DEFAULT_TOOLS_PATH;
    use crate::mcpb::McpbManifest;

    // Resolve the path
    let source_path = if path.starts_with('~') {
        match dirs::home_dir() {
            Some(home) => home.join(&path[2..]),
            None => {
                let msg = "Could not determine home directory".to_string();
                println!("  {} {}", "✗".bright_red(), msg);
                return InstallResult::Failed(msg);
            }
        }
    } else {
        PathBuf::from(path)
    };

    let source_path = match source_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let msg = format!("Path not found: {}", path);
            println!("  {} {}", "✗".bright_red(), msg);
            return InstallResult::Failed(msg);
        }
    };

    // Check for manifest.json
    let manifest_path = source_path.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        let msg = format!(
            "No {} found in {}. Run `tool init` first.",
            MCPB_MANIFEST_FILE,
            source_path.display()
        );
        println!("  {} {}", "✗".bright_red(), msg);
        return InstallResult::Failed(msg);
    }

    // Load manifest to get name and version
    let manifest = match McpbManifest::load(&source_path) {
        Ok(m) => m,
        Err(e) => {
            let msg = format!("Failed to load manifest: {}", e);
            println!("  {} {}", "✗".bright_red(), msg);
            return InstallResult::Failed(msg);
        }
    };
    let tool_name = match manifest.name.as_ref() {
        Some(n) => n,
        None => {
            let msg = "manifest.json must include a name field".to_string();
            println!("  {} {}", "✗".bright_red(), msg);
            return InstallResult::Failed(msg);
        }
    };
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
            return InstallResult::AlreadyInstalled;
        }

        // Remove existing (symlink or directory)
        if target_path.is_symlink() || target_path.is_file() {
            if let Err(e) = std::fs::remove_file(&target_path) {
                let msg = format!("Failed to remove existing link: {}", e);
                println!("  {} {}", "✗".bright_red(), msg);
                return InstallResult::Failed(msg);
            }
        } else if let Err(e) = std::fs::remove_dir_all(&target_path) {
            let msg = format!("Failed to remove existing directory: {}", e);
            println!("  {} {}", "✗".bright_red(), msg);
            return InstallResult::Failed(msg);
        }
    }

    // Ensure parent directory exists
    if let Some(parent) = target_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        let msg = format!("Failed to create tools directory: {}", e);
        println!("  {} {}", "✗".bright_red(), msg);
        return InstallResult::Failed(msg);
    }

    // Create symlink
    #[cfg(unix)]
    if let Err(e) = std::os::unix::fs::symlink(&source_path, &target_path) {
        let msg = format!("Failed to create symlink: {}", e);
        println!("  {} {}", "✗".bright_red(), msg);
        return InstallResult::Failed(msg);
    }

    #[cfg(windows)]
    if let Err(e) = std::os::windows::fs::symlink_dir(&source_path, &target_path) {
        let msg = format!("Failed to create symlink: {}", e);
        println!("  {} {}", "✗".bright_red(), msg);
        return InstallResult::Failed(msg);
    }

    println!(
        "  {} Installed {} {}",
        "✓".bright_green(),
        target_name.bright_cyan(),
        "(linked)".dimmed()
    );

    InstallResult::InstalledLocal
}

/// Remove a single installed tool.
async fn remove_tool(name: &str) -> (String, UninstallResult) {
    use tokio::fs;

    let resolver = FilePluginResolver::default();

    // First, find the tool
    let resolved = match resolver.resolve_tool(name).await {
        Ok(Some(r)) => r,
        Ok(None) => return (name.to_string(), UninstallResult::NotFound),
        Err(e) => return (name.to_string(), UninstallResult::Failed(e.to_string())),
    };

    // Get the directory containing the tool
    let tool_dir = match resolved.path.parent() {
        Some(d) => d,
        None => {
            return (
                name.to_string(),
                UninstallResult::Failed("Failed to get tool directory".into()),
            );
        }
    };

    // Remove the directory
    if let Err(e) = fs::remove_dir_all(tool_dir).await {
        return (
            resolved.plugin_ref.to_string(),
            UninstallResult::Failed(format!("Failed to remove: {}", e)),
        );
    }

    (resolved.plugin_ref.to_string(), UninstallResult::Removed)
}

/// Remove multiple installed tools.
pub async fn remove_tools(names: &[String]) -> ToolResult<()> {
    use futures_util::future::join_all;

    // Run all removals concurrently
    let futures: Vec<_> = names.iter().map(|name| remove_tool(name)).collect();
    let results = join_all(futures).await;

    let mut removed_count = 0usize;
    let mut not_found_count = 0usize;
    let mut failed_count = 0usize;

    // Print results
    for (tool_name, result) in &results {
        match result {
            UninstallResult::Removed => {
                println!(
                    "  {} Removed {}",
                    "✓".bright_green(),
                    tool_name.bright_cyan()
                );
                removed_count += 1;
            }
            UninstallResult::NotFound => {
                println!(
                    "  {} Tool {} not found",
                    "✗".bright_red(),
                    tool_name.bright_white().bold()
                );
                not_found_count += 1;
            }
            UninstallResult::Failed(msg) => {
                println!("  {} {}: {}", "✗".bright_red(), tool_name, msg);
                failed_count += 1;
            }
        }
    }

    // Print summary if multiple tools were requested
    if names.len() > 1 {
        println!();
        if removed_count > 0 {
            println!(
                "Removed {} {}",
                removed_count.to_string().bright_green(),
                if removed_count == 1 {
                    "package"
                } else {
                    "packages"
                }
            );
        }
        if not_found_count > 0 {
            println!("Not found: {}", not_found_count.to_string().bright_yellow());
        }
        if failed_count > 0 {
            println!("Failed: {}", failed_count.to_string().bright_red());
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
pub async fn publish_mcpb(path: &str, dry_run: bool, strict: bool) -> ToolResult<()> {
    use crate::handlers::auth::{get_registry_token, load_credentials};
    use crate::pack::{PackOptions, pack_bundle};
    use crate::validate::validate_manifest;
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

    // Strict validation: treat warnings as errors
    if strict {
        let validation = validate_manifest(&dir);
        if !validation.is_strict_valid() {
            println!();
            let total = validation.errors.len() + validation.warnings.len();
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
            println!(
                "  {} {}",
                "✗".bright_red(),
                if total == 1 {
                    "1 error".to_string()
                } else {
                    format!("{} errors", total)
                }
            );
            println!("\n  Cannot publish with --strict. Fix errors and warnings, then retry.");
            std::process::exit(1);
        }
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
    let bundle_format = manifest.bundle_extension();
    println!(
        "\n    {} Uploading bundle ({})...",
        "→".bright_blue(),
        bundle_format
    );
    let upload_info = client
        .init_upload(
            &namespace,
            tool_name,
            version,
            bundle_size,
            &sha256,
            bundle_format,
        )
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

    let format_display = if manifest.requires_mcpbx() {
        "mcpbx".bright_yellow()
    } else {
        "mcpb".bright_green()
    };
    println!(
        "\n  {} Published {}/{}@{} ({})",
        "✓".bright_green(),
        namespace.bright_blue(),
        tool_name.bright_cyan(),
        result.version.bright_white(),
        format_display
    );
    println!(
        "    {}/plugins/{}/{}",
        client.registry_url(),
        namespace,
        tool_name
    );

    Ok(())
}
