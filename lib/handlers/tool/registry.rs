//! Registry command handlers.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcpb::McpbManifest;
use crate::pack::{PackOptions, compute_sha256, pack_bundle};
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use crate::resolver::FilePluginResolver;
use crate::styles::Spinner;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// Options for multi-artifact publishing.
#[derive(Debug, Clone, Default)]
pub struct MultiArtifactOptions {
    /// Platform-specific bundles to create (e.g., "darwin-arm64", "linux-x86_64").
    pub platforms: Vec<String>,
    /// Also create a universal bundle containing all platform binaries.
    pub include_universal: bool,
    /// Explicit artifact paths: platform -> path (e.g., "darwin-arm64" -> "./dist/darwin.mcpb").
    pub explicit_artifacts: HashMap<String, PathBuf>,
}

/// Version manifest for multi-artifact versions.
/// This is uploaded as `version.json` and becomes the main_file.
#[derive(Debug, Serialize)]
pub struct VersionManifest {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Map of platform key to artifact info.
    pub artifacts: HashMap<String, ArtifactEntry>,
}

/// Entry for a single artifact in the version manifest.
#[derive(Debug, Serialize)]
pub struct ArtifactEntry {
    /// Filename of the artifact.
    pub filename: String,
    /// Size in bytes.
    pub size: u64,
    /// Checksum with algorithm prefix (e.g., "sha256:abc123...").
    pub checksum: String,
}

/// Pre-flight information for a registry download.
#[allow(dead_code)]
struct RegistryPreflight {
    name: String,
    namespace: String,
    tool_name: String,
    version: String,
    download_size: u64,
    download_url: String,
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

/// Get the current platform identifier (e.g., "darwin-arm64", "linux-x64").
fn get_current_platform() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    // Map OS names
    let os_name = match os {
        "macos" => "darwin",
        "windows" => "win32",
        _ => os,
    };

    // Map architecture names
    let arch_name = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => arch,
    };

    format!("{}-{}", os_name, arch_name)
}

/// Download a tool from the registry.
/// Preflight info for download.
struct DownloadPreflight {
    namespace: String,
    tool_name: String,
    version: String,
    download_size: u64,
    download_url: String,
    output_path: PathBuf,
    #[allow(dead_code)]
    platform: Option<String>,
}

/// Run preflight for a download.
async fn preflight_download(
    name: &str,
    output_dir: Option<&Path>,
    platform: Option<&str>,
) -> Result<DownloadPreflight, String> {
    let plugin_ref = name
        .parse::<PluginRef>()
        .map_err(|e| format!("Invalid tool reference '{}': {}", name, e))?;

    if plugin_ref.namespace().is_none() {
        return Err(format!(
            "{}: missing namespace (use namespace/name format)",
            name
        ));
    }

    let namespace = plugin_ref.namespace().unwrap().to_string();
    let tool_name = plugin_ref.name().to_string();

    let client = RegistryClient::new();

    // Determine the version
    let version = if let Some(v) = plugin_ref.version_str() {
        v.to_string()
    } else {
        let artifact = client
            .get_artifact(&namespace, &tool_name)
            .await
            .map_err(|_| format!("Tool {}/{} not found in registry", namespace, tool_name))?;
        artifact
            .latest_version
            .ok_or_else(|| format!("No versions published for {}/{}", namespace, tool_name))?
            .version
    };

    // Get full version info
    let version_info = client
        .get_version(&namespace, &tool_name, &version)
        .await
        .map_err(|e| format!("Failed to fetch version info: {}", e))?;

    // Determine which bundle to download based on platform preference
    let (download_url, download_size, selected_platform, bundle_ext) =
        select_platform_bundle(&version_info, platform, &tool_name, &version)?;

    // Determine output path with correct extension
    let bundle_name = match &selected_platform {
        Some(p) => format!("{}@{}-{}.{}", tool_name, version, p, bundle_ext),
        None => format!("{}@{}.{}", tool_name, version, bundle_ext),
    };

    let output_path = match output_dir {
        Some(dir) => dir.join(&bundle_name),
        None => std::env::current_dir()
            .map_err(|e| format!("Failed to get current dir: {}", e))?
            .join(&bundle_name),
    };

    Ok(DownloadPreflight {
        namespace,
        tool_name,
        version,
        download_size,
        download_url,
        output_path,
        platform: selected_platform,
    })
}

/// Select the appropriate bundle based on platform preference.
/// Returns (download_url, size, selected_platform, extension).
fn select_platform_bundle(
    version_info: &crate::registry::VersionInfo,
    platform: Option<&str>,
    tool_name: &str,
    version: &str,
) -> Result<(String, u64, Option<String>, String), String> {
    let files = version_info.files.as_ref();

    // Helper to check if a filename is a platform-specific bundle
    fn is_platform_specific(filename: &str) -> bool {
        filename.contains("-darwin-")
            || filename.contains("-linux-")
            || filename.contains("-win32-")
    }

    // Helper to find universal bundle in files
    fn find_universal_bundle(
        files: &std::collections::HashMap<String, crate::registry::FileInfo>,
    ) -> Option<(&String, &crate::registry::FileInfo)> {
        files.iter().find(|(filename, _)| {
            (filename.ends_with(".mcpb") || filename.ends_with(".mcpbx"))
                && !is_platform_specific(filename)
        })
    }

    // If explicit "universal" requested, look for universal bundle in files first
    if platform == Some("universal") {
        if let Some(files) = files
            && let Some((filename, info)) = find_universal_bundle(files)
        {
            let ext = if filename.ends_with(".mcpbx") {
                "mcpbx"
            } else {
                "mcpb"
            };
            return Ok((info.url.clone(), info.size, None, ext.to_string()));
        }
        // Fall back to main_download_url only if it's actually a bundle
        if let Some(url) = &version_info.main_download_url
            && (url.ends_with(".mcpb") || url.ends_with(".mcpbx"))
        {
            let size = version_info.main_download_size.unwrap_or(0);
            let ext = if url.ends_with(".mcpbx") {
                "mcpbx"
            } else {
                "mcpb"
            };
            return Ok((url.clone(), size, None, ext.to_string()));
        }
        return Err(format!("No universal bundle for {}@{}", tool_name, version));
    }

    // Check if we have platform-specific files
    if let Some(files) = files {
        let current_platform = get_current_platform();
        let target_platform = platform.unwrap_or(&current_platform);

        // Generate platform variants (x64 <-> x86_64 aliasing)
        let platform_variants: Vec<String> = {
            let mut variants = vec![target_platform.to_string()];
            if target_platform.ends_with("-x64") {
                variants.push(target_platform.replace("-x64", "-x86_64"));
            } else if target_platform.ends_with("-x86_64") {
                variants.push(target_platform.replace("-x86_64", "-x64"));
            }
            variants
        };

        // Look for platform-specific bundle in files
        for (filename, info) in files {
            for variant in &platform_variants {
                if filename.contains(&format!("-{}", variant))
                    && (filename.ends_with(".mcpb") || filename.ends_with(".mcpbx"))
                {
                    let ext = if filename.ends_with(".mcpbx") {
                        "mcpbx"
                    } else {
                        "mcpb"
                    };
                    return Ok((
                        info.url.clone(),
                        info.size,
                        Some(variant.to_string()),
                        ext.to_string(),
                    ));
                }
            }
        }

        // If platform was explicitly requested but not found, error
        if platform.is_some() {
            return Err(format!(
                "Platform '{}' not available for {}@{}. Use --platform universal for universal bundle.",
                target_platform, tool_name, version
            ));
        }

        // Auto-detect: No platform match found, try universal bundle from files
        if let Some((filename, info)) = find_universal_bundle(files) {
            let ext = if filename.ends_with(".mcpbx") {
                "mcpbx"
            } else {
                "mcpb"
            };
            return Ok((info.url.clone(), info.size, None, ext.to_string()));
        }
    }

    // Fall back to main_download_url only if it's actually a bundle
    if let Some(url) = &version_info.main_download_url
        && (url.ends_with(".mcpb") || url.ends_with(".mcpbx"))
    {
        let size = version_info.main_download_size.unwrap_or(0);
        let ext = if url.ends_with(".mcpbx") {
            "mcpbx"
        } else {
            "mcpb"
        };
        return Ok((url.clone(), size, None, ext.to_string()));
    }

    Err(format!(
        "No download available for {}@{}",
        tool_name, version
    ))
}

/// Download multiple tools from the registry.
pub async fn download_tools(
    names: &[String],
    output: Option<&str>,
    platform: Option<&str>,
) -> ToolResult<()> {
    use futures_util::future::join_all;

    let is_single = names.len() == 1;

    // Resolve output directory
    let output_dir = match output {
        Some(p) => {
            let path = PathBuf::from(p);
            let abs_path = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()?.join(path)
            };
            // Create directory if it doesn't exist
            if !abs_path.exists() {
                std::fs::create_dir_all(&abs_path)?;
            }
            Some(abs_path)
        }
        None => None,
    };

    // Phase 1: Resolve
    if is_single {
        println!(
            "  {} Resolving {}",
            "→".bright_blue(),
            names[0].bright_cyan()
        );
    } else {
        println!(
            "  {} Resolving {} packages",
            "→".bright_blue(),
            names.len().to_string().bright_cyan()
        );
    }

    let preflight_futures: Vec<_> = names
        .iter()
        .map(|name| preflight_download(name, output_dir.as_deref(), platform))
        .collect();
    let preflight_results = join_all(preflight_futures).await;

    // Separate successes from failures
    let mut preflights = Vec::new();
    let mut failed = Vec::new();

    for (name, result) in names.iter().zip(preflight_results) {
        match result {
            Ok(pf) => preflights.push(pf),
            Err(msg) => failed.push((name.clone(), msg)),
        }
    }

    // Print preflight failures
    for (name, msg) in &failed {
        println!("  {} {}: {}", "✗".bright_red(), name, msg);
    }

    // Phase 2: Download
    if !preflights.is_empty() {
        let client = RegistryClient::new();

        if is_single && preflights.len() == 1 {
            // Single package: match original format
            let pf = preflights.remove(0);
            println!(
                "  {} Downloading {}/{}@{}",
                "→".bright_blue(),
                pf.namespace.bright_cyan(),
                pf.tool_name.bright_cyan(),
                pf.version.bright_cyan()
            );

            let pb = ProgressBar::new(pf.download_size);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("  [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
                    .unwrap()
                    .progress_chars("█░░"),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            match client
                .download_from_url_with_progress_pb(&pf.download_url, &pf.output_path, &pb)
                .await
            {
                Ok(size) => {
                    pb.finish_and_clear();
                    let path_str = pf.output_path.display().to_string();
                    let colored_path = if path_str.ends_with(".mcpbx") {
                        path_str.bright_yellow()
                    } else {
                        path_str.bright_green()
                    };
                    println!(
                        "  {} Downloaded {} ({})",
                        "✓".bright_green(),
                        colored_path,
                        format_size(size)
                    );
                }
                Err(e) => {
                    pb.finish_and_clear();
                    println!("  {} Download failed: {}", "✗".bright_red(), e);
                }
            }
        } else {
            // Multiple packages: parallel download
            let count = preflights.len();
            println!(
                "  {} Downloading {} packages",
                "→".bright_blue(),
                count.to_string().bright_cyan()
            );

            let mp = MultiProgress::new();
            let style = ProgressStyle::default_bar()
                .template("  {msg:<30} [{bar:25.cyan/dim}] {bytes:>10}/{total_bytes:<10}")
                .unwrap()
                .progress_chars("█░░");

            let handles: Vec<_> = preflights
                .into_iter()
                .map(|pf| {
                    let pb = mp.add(ProgressBar::new(pf.download_size));
                    pb.set_style(style.clone());
                    pb.set_message(format!("{}/{}", pf.namespace, pf.tool_name));
                    pb.enable_steady_tick(std::time::Duration::from_millis(100));

                    let client = RegistryClient::new();
                    tokio::spawn(async move {
                        let result = client
                            .download_from_url_with_progress_pb(
                                &pf.download_url,
                                &pf.output_path,
                                &pb,
                            )
                            .await;
                        pb.finish_and_clear();
                        (pf, result)
                    })
                })
                .collect();

            let results = join_all(handles).await;

            // Print results
            let mut downloaded_count = 0usize;
            let mut failed_count = failed.len();

            for result in results {
                match result {
                    Ok((pf, Ok(size))) => {
                        let path_str = pf.output_path.display().to_string();
                        let colored_path = if path_str.ends_with(".mcpbx") {
                            path_str.bright_yellow()
                        } else {
                            path_str.bright_green()
                        };
                        println!(
                            "  {} Downloaded {} ({})",
                            "✓".bright_green(),
                            colored_path,
                            format_size(size)
                        );
                        downloaded_count += 1;
                    }
                    Ok((pf, Err(e))) => {
                        println!(
                            "  {} {}/{}: {}",
                            "✗".bright_red(),
                            pf.namespace,
                            pf.tool_name,
                            e
                        );
                        failed_count += 1;
                    }
                    Err(_) => {
                        println!("  {} Task panicked", "✗".bright_red());
                        failed_count += 1;
                    }
                }
            }

            // Summary
            println!();
            if downloaded_count > 0 {
                print!(
                    "Downloaded {} {}",
                    downloaded_count.to_string().bright_green(),
                    if downloaded_count == 1 {
                        "package"
                    } else {
                        "packages"
                    }
                );
            }
            if failed_count > 0 {
                if downloaded_count > 0 {
                    print!(", ");
                }
                print!("{} failed", failed_count.to_string().bright_red());
            }
            println!();
        }
    }

    Ok(())
}

/// Run pre-flight checks for a tool (validation, metadata fetch, already-installed check).
async fn preflight_tool(name: &str, platform: Option<&str>) -> PreflightResult {
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

    // Get latest version string
    let version = match &artifact.latest_version {
        Some(v) => v.version.clone(),
        None => {
            return PreflightResult::Failed(format!("No published version for {}", name));
        }
    };

    // Fetch full version info (includes download URL)
    let version_info = match client.get_version(&namespace, &tool_name, &version).await {
        Ok(v) => v,
        Err(e) => {
            return PreflightResult::Failed(format!("Failed to fetch version info: {}", e));
        }
    };

    // Select the appropriate platform bundle
    let (download_url, download_size, _selected_platform, _ext) =
        match select_platform_bundle(&version_info, platform, &tool_name, &version) {
            Ok(result) => result,
            Err(msg) => return PreflightResult::Failed(msg),
        };

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
        download_size,
        download_url,
        target_dir,
        temp_file,
    })
}

/// Result of download_and_install with size info.
struct InstallSuccess {
    namespace: String,
    tool_name: String,
    version: String,
    size: u64,
}

/// Download and install a tool with a progress bar.
/// Returns the install result and size on success.
async fn download_and_install(
    preflight: RegistryPreflight,
    pb: ProgressBar,
) -> Result<InstallSuccess, String> {
    let client = RegistryClient::new();

    // Download from CDN URL with progress
    let size = client
        .download_from_url_with_progress_pb(&preflight.download_url, &preflight.temp_file, &pb)
        .await
        .map_err(|e| format!("Failed to download: {}", e))?;

    // Create target directory
    tokio::fs::create_dir_all(&preflight.target_dir)
        .await
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    // Extract the bundle
    extract_bundle(&preflight.temp_file, &preflight.target_dir)
        .map_err(|e| format!("Failed to extract: {}", e))?;

    // Clean up temp file
    let _ = std::fs::remove_file(&preflight.temp_file);

    Ok(InstallSuccess {
        namespace: preflight.namespace,
        tool_name: preflight.tool_name,
        version: preflight.version,
        size,
    })
}

/// Install multiple tools from the registry or local paths.
///
/// If `platform` is specified, it will be used to select a platform-specific
/// artifact when installing multi-artifact versions. Use "universal" to
/// explicitly select the universal bundle.
pub async fn add_tools(names: &[String], platform: Option<&str>) -> ToolResult<()> {
    use futures_util::future::join_all;

    // Phase 1: Run preflight checks
    let is_single = names.len() == 1;

    if is_single {
        println!(
            "  {} Resolving {}",
            "→".bright_blue(),
            names[0].bright_cyan()
        );
    } else {
        println!(
            "  {} Resolving {} packages",
            "→".bright_blue(),
            names.len().to_string().bright_cyan()
        );
    }

    let preflight_futures: Vec<_> = names
        .iter()
        .map(|name| preflight_tool(name, platform))
        .collect();
    let preflight_results = join_all(preflight_futures).await;

    // Separate registry downloads from immediate results
    let mut registry_preflights = Vec::new();
    let mut local_count = 0usize;
    let mut already_installed = Vec::new();
    let mut failed = Vec::new();

    for (name, result) in names.iter().zip(preflight_results) {
        match result {
            PreflightResult::Registry(preflight) => {
                registry_preflights.push(preflight);
            }
            PreflightResult::Local(install_result) => match install_result {
                InstallResult::InstalledLocal => local_count += 1,
                InstallResult::AlreadyInstalled => already_installed.push(name.clone()),
                InstallResult::Failed(msg) => failed.push((name.clone(), msg)),
                _ => {}
            },
            PreflightResult::AlreadyInstalled => {
                already_installed.push(name.clone());
            }
            PreflightResult::Failed(msg) => {
                failed.push((name.clone(), msg));
            }
        }
    }

    // Print already installed (non-error)
    for name in &already_installed {
        println!(
            "  {} Already installed {}",
            "✓".bright_green(),
            name.bright_cyan()
        );
    }

    // Print preflight failures
    for (name, msg) in &failed {
        println!("  {} {}: {}", "✗".bright_red(), name, msg);
    }

    // Phase 2: Download and install registry packages
    if !registry_preflights.is_empty() {
        let count = registry_preflights.len();

        if is_single {
            // Single package: match download format exactly
            let preflight = registry_preflights.remove(0);
            println!(
                "  {} Downloading {}/{}@{}",
                "→".bright_blue(),
                preflight.namespace.bright_cyan(),
                preflight.tool_name.bright_cyan(),
                preflight.version.bright_cyan()
            );

            let pb = ProgressBar::new(preflight.download_size);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("  [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
                    .unwrap()
                    .progress_chars("█░░"),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            match download_and_install(preflight, pb.clone()).await {
                Ok(success) => {
                    pb.finish_and_clear();
                    println!(
                        "  {} Installed {}/{}@{} ({})",
                        "✓".bright_green(),
                        success.namespace.bright_cyan(),
                        success.tool_name.bright_cyan(),
                        success.version.bright_cyan(),
                        format_size(success.size)
                    );
                }
                Err(msg) => {
                    pb.finish_and_clear();
                    println!("  {} Install failed: {}", "✗".bright_red(), msg);
                }
            }
        } else {
            // Multiple packages: parallel download with multi-progress
            println!(
                "  {} Downloading {} packages",
                "→".bright_blue(),
                count.to_string().bright_cyan()
            );

            let mp = MultiProgress::new();
            let style = ProgressStyle::default_bar()
                .template("  {msg:<30} [{bar:25.cyan/dim}] {bytes:>10}/{total_bytes:<10}")
                .unwrap()
                .progress_chars("█░░");

            // Create progress bars and spawn download tasks
            let handles: Vec<_> = registry_preflights
                .into_iter()
                .map(|preflight| {
                    let pb = mp.add(ProgressBar::new(preflight.download_size));
                    pb.set_style(style.clone());
                    pb.set_message(format!("{}/{}", preflight.namespace, preflight.tool_name));
                    pb.enable_steady_tick(std::time::Duration::from_millis(100));

                    tokio::spawn(async move {
                        let result = download_and_install(preflight, pb.clone()).await;
                        pb.finish_and_clear();
                        result
                    })
                })
                .collect();

            // Wait for all downloads to complete
            let results = join_all(handles).await;

            // Print results
            let mut installed_count = 0usize;
            let mut failed_count = 0usize;

            for result in results {
                match result {
                    Ok(Ok(success)) => {
                        println!(
                            "  {} Installed {}/{}@{} ({})",
                            "✓".bright_green(),
                            success.namespace.bright_cyan(),
                            success.tool_name.bright_cyan(),
                            success.version.bright_cyan(),
                            format_size(success.size)
                        );
                        installed_count += 1;
                    }
                    Ok(Err(msg)) => {
                        println!("  {} {}", "✗".bright_red(), msg);
                        failed_count += 1;
                    }
                    Err(_) => {
                        println!("  {} Task panicked", "✗".bright_red());
                        failed_count += 1;
                    }
                }
            }

            // Summary line for multiple packages
            let total = installed_count + local_count;
            if total > 0 || failed_count > 0 || !already_installed.is_empty() {
                println!();
                if total > 0 {
                    print!(
                        "Installed {} {}",
                        total.to_string().bright_green(),
                        if total == 1 { "package" } else { "packages" }
                    );
                }
                if !already_installed.is_empty() {
                    if total > 0 {
                        print!(", ");
                    }
                    print!(
                        "{} already installed",
                        already_installed.len().to_string().bright_cyan()
                    );
                }
                if failed_count > 0 {
                    if total > 0 || !already_installed.is_empty() {
                        print!(", ");
                    }
                    print!("{} failed", failed_count.to_string().bright_red());
                }
                println!();
            }
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
        "  {} Linking {} from {}",
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
    }

    println!();
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

/// Publish a tool to the registry.
pub async fn publish_mcpb(
    path: &str,
    dry_run: bool,
    strict: bool,
    multi_platform: bool,
    prebuilt_artifacts: HashMap<String, PathBuf>,
) -> ToolResult<()> {
    use crate::handlers::auth::{get_registry_token, load_credentials};
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
            "  {} Dry run: validating tool {}/{}",
            "→".bright_blue(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    } else {
        println!(
            "  {} Publishing tool {}/{}",
            "→".bright_blue(),
            namespace.bright_blue(),
            tool_name.bright_cyan()
        );
    }

    println!("  · {}: {}", "Version".dimmed(), version.bright_white());
    println!(
        "  · {}: {}",
        "Source".dimmed(),
        dir.display().to_string().dimmed()
    );
    if let Some(desc) = description {
        println!("  · {}: {}", "Description".dimmed(), desc.dimmed());
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
                println!("  · {}", issue.details.dimmed());
                if let Some(help) = &issue.help {
                    println!("  · {}: {}", "help".bright_green().dimmed(), help.dimmed());
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

    // Check if we should use multi-platform mode
    let use_multi_platform = multi_platform || !prebuilt_artifacts.is_empty();

    if use_multi_platform {
        // Build multi-artifact options
        let options = if !prebuilt_artifacts.is_empty() {
            // Use prebuilt artifacts
            MultiArtifactOptions {
                platforms: prebuilt_artifacts.keys().cloned().collect(),
                include_universal: prebuilt_artifacts.contains_key("universal"),
                explicit_artifacts: prebuilt_artifacts,
            }
        } else {
            // Auto-detect platforms from manifest
            let platforms = detect_available_platforms(&manifest);
            if platforms.is_empty() {
                println!(
                    "  {} No platform overrides found in manifest.",
                    "⚠".bright_yellow()
                );
                println!("  Publishing as single universal bundle instead.");
                // Fall through to single-artifact mode
                MultiArtifactOptions::default()
            } else {
                MultiArtifactOptions {
                    platforms,
                    include_universal: true, // Always include universal bundle
                    explicit_artifacts: HashMap::new(),
                }
            }
        };

        // Only use multi-artifact if we have platforms or explicit artifacts
        if !options.platforms.is_empty() || !options.explicit_artifacts.is_empty() {
            return publish_multi_artifact_impl(
                &dir,
                &manifest,
                &manifest_content,
                &namespace,
                tool_name,
                version,
                description,
                options,
                dry_run,
                token,
            )
            .await;
        }
    }

    // Single-artifact mode (original logic)
    // Bundle the tool
    println!();
    let spinner = Spinner::new("Creating bundle");

    let pack_options = PackOptions {
        validate: true,
        output: None,
        verbose: false,
        include_dotfiles: false,
    };
    let pack_result = match pack_bundle(&dir, &pack_options) {
        Ok(result) => {
            spinner.succeed(Some("Bundle created"));
            result
        }
        Err(e) => {
            spinner.fail(None);
            return Err(ToolError::Generic(format!("Pack failed: {}", e)));
        }
    };

    // Read the bundle
    let bundle = std::fs::read(&pack_result.output_path)
        .map_err(|e| ToolError::Generic(format!("Failed to read bundle: {}", e)))?;
    let bundle_size = bundle.len() as u64;
    println!("  · Size: {}", format_size(bundle_size).bright_white());

    // Read icon if present
    let icon_data = if let Some(ref icon_path) = pack_result.icon_path {
        match std::fs::read(icon_path) {
            Ok(bytes) => {
                println!(
                    "  · Icon: {}",
                    format_size(bytes.len() as u64).bright_white()
                );
                Some(bytes)
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Clean up temp bundle and icon
    let _ = std::fs::remove_file(&pack_result.output_path);
    if let Some(ref icon_path) = pack_result.icon_path {
        let _ = std::fs::remove_file(icon_path);
    }

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
    println!();
    let spinner = Spinner::new(format!("Checking registry ({})", client.registry_url()));
    let artifact_exists = match client.artifact_exists(&namespace, tool_name).await {
        Ok(exists) => {
            spinner.succeed(Some("Registry checked"));
            exists
        }
        Err(e) => {
            spinner.fail(None);
            return Err(e);
        }
    };

    if !artifact_exists {
        let spinner = Spinner::new("Creating artifact entry");
        match client
            .create_artifact(&namespace, tool_name, description)
            .await
        {
            Ok(()) => {
                spinner.succeed(Some(&format!("Created {}/{}", namespace, tool_name)));
            }
            Err(e) => {
                spinner.fail(None);
                return Err(e);
            }
        }
    }

    // Compute SHA-256 for bundle
    let mut hasher = Sha256::new();
    hasher.update(&bundle);
    let sha256 = format!("{:x}", hasher.finalize());

    // Build file specs for upload
    let file_name = format!("{}.{}", tool_name, manifest.bundle_extension());
    let mut files = vec![crate::registry::FileSpec {
        name: file_name.clone(),
        size: bundle_size as i64,
        sha256: sha256.clone(),
    }];

    // Add icon to upload if present
    let icon_filename = if let Some(ref icon_bytes) = icon_data {
        // Use the original icon filename from manifest
        manifest.icon.as_ref().map(|icon_name| {
            let mut icon_hasher = Sha256::new();
            icon_hasher.update(icon_bytes);
            let icon_sha256 = format!("{:x}", icon_hasher.finalize());

            let filename = icon_name.clone();
            files.push(crate::registry::FileSpec {
                name: filename.clone(),
                size: icon_bytes.len() as i64,
                sha256: icon_sha256,
            });
            filename
        })
    } else {
        None
    };

    let file_count = files.len();
    println!(
        "\n  {} Uploading {} file{}",
        "→".bright_blue(),
        file_count,
        if file_count > 1 { "s" } else { "" }
    );
    let upload_info = client
        .init_upload(&namespace, tool_name, version, files)
        .await?;

    // Upload bundle
    let bundle_target = upload_info
        .uploads
        .iter()
        .find(|t| t.name == file_name)
        .ok_or_else(|| ToolError::Generic("No upload target for bundle".into()))?;

    let pb = ProgressBar::new(bundle_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap()
            .progress_chars("█░░"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let pb_arc = Arc::new(pb);
    let pb_clone = Arc::clone(&pb_arc);
    client
        .upload_bundle_with_progress(
            &bundle_target.upload_url,
            &bundle,
            &bundle_target.content_type,
            move |bytes| {
                pb_clone.set_position(bytes);
            },
        )
        .await?;
    pb_arc.finish_and_clear();

    // Upload icon if present
    if let (Some(icon_bytes), Some(icon_name)) = (icon_data, icon_filename) {
        let icon_target = upload_info
            .uploads
            .iter()
            .find(|t| t.name == icon_name)
            .ok_or_else(|| ToolError::Generic("No upload target for icon".into()))?;

        let icon_pb = ProgressBar::new(icon_bytes.len() as u64);
        icon_pb.set_style(
            ProgressStyle::default_bar()
                .template("  [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
                .unwrap()
                .progress_chars("█░░"),
        );
        icon_pb.enable_steady_tick(std::time::Duration::from_millis(100));

        let icon_pb_arc = Arc::new(icon_pb);
        let icon_pb_clone = Arc::clone(&icon_pb_arc);
        client
            .upload_bundle_with_progress(
                &icon_target.upload_url,
                &icon_bytes,
                &icon_target.content_type,
                move |bytes| {
                    icon_pb_clone.set_position(bytes);
                },
            )
            .await?;
        icon_pb_arc.finish_and_clear();
    }

    println!("  {} Upload complete", "✓".bright_green());

    // Publish the version
    println!();
    let spinner = Spinner::new("Publishing version");

    let manifest_json: serde_json::Value = match serde_json::from_str(&manifest_content) {
        Ok(json) => json,
        Err(e) => {
            spinner.fail(Some("Publishing failed"));
            return Err(e.into());
        }
    };

    // Derive icon_url from manifest.icon + uploaded files
    let icon_url = manifest_json
        .get("icon")
        .and_then(|icon| icon.as_str())
        .and_then(|icon_filename| {
            upload_info
                .uploads
                .iter()
                .find(|target| target.name == icon_filename)
                .map(|target| target.cdn_url.clone())
        });

    let result = match client
        .publish_version(
            &namespace,
            tool_name,
            &upload_info.upload_id,
            version,
            &file_name,
            manifest_json,
            description,
            icon_url,
        )
        .await
    {
        Ok(result) => {
            spinner.succeed(Some("Version published"));
            result
        }
        Err(e) => {
            spinner.fail(Some("Publishing failed"));
            return Err(e);
        }
    };

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
        "  · {}/plugins/{}/{}",
        client.registry_url(),
        namespace,
        tool_name
    );

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: Multi-Artifact Publishing
//--------------------------------------------------------------------------------------------------

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

/// Detect available platforms from manifest's platform_overrides.
/// Only returns valid OS-arch platforms (e.g., "darwin-arm64"), not OS-only (e.g., "darwin").
fn detect_available_platforms(manifest: &McpbManifest) -> Vec<String> {
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
    }

    // Note: We don't fall back to server.mcp_config.platform_overrides here
    // because those are typically OS-only keys (darwin, linux, win32) meant for
    // runtime resolution, not for creating separate bundles.

    // Deduplicate
    platforms.sort();
    platforms.dedup();
    platforms
}

/// Implementation of multi-artifact publish.
#[allow(clippy::too_many_arguments)]
async fn publish_multi_artifact_impl(
    dir: &Path,
    manifest: &McpbManifest,
    manifest_content: &str,
    namespace: &str,
    tool_name: &str,
    version: &str,
    description: Option<&str>,
    options: MultiArtifactOptions,
    dry_run: bool,
    token: Option<String>,
) -> ToolResult<()> {
    println!();
    println!(
        "  {} Multi-artifact publish: {} platforms{}",
        "→".bright_blue(),
        options.platforms.len().to_string().bright_cyan(),
        if options.include_universal {
            " + universal"
        } else {
            ""
        }
    );

    // Collect all files to upload
    let mut files_to_upload: Vec<(String, Vec<u8>, String)> = Vec::new(); // (name, bytes, checksum)
    let mut version_manifest_artifacts: HashMap<String, ArtifactEntry> = HashMap::new();

    // Process explicit artifacts or pack bundles
    if !options.explicit_artifacts.is_empty() {
        // Use explicit artifact files
        for (platform, path) in &options.explicit_artifacts {
            let bytes = std::fs::read(path).map_err(|e| {
                ToolError::Generic(format!("Failed to read {}: {}", path.display(), e))
            })?;
            let checksum = compute_sha256(&bytes);
            let filename = path
                .file_name()
                .ok_or_else(|| ToolError::Generic(format!("Invalid path: {}", path.display())))?
                .to_string_lossy()
                .to_string();

            println!(
                "  · {}: {} ({})",
                platform.bright_cyan(),
                filename,
                format_size(bytes.len() as u64)
            );

            version_manifest_artifacts.insert(
                platform.clone(),
                ArtifactEntry {
                    filename: filename.clone(),
                    size: bytes.len() as u64,
                    checksum: format!("sha256:{}", checksum),
                },
            );
            files_to_upload.push((filename, bytes, checksum));
        }
    } else {
        // Pack bundles for each platform in parallel
        use crate::pack::pack_bundle_for_platform;

        let pack_options = PackOptions {
            validate: true,
            output: None,
            verbose: false,
            include_dotfiles: false,
        };

        // Create pack tasks for all platforms
        let mut pack_handles = Vec::new();
        for platform in &options.platforms {
            let dir_clone = dir.to_path_buf();
            let opts = pack_options.clone();
            let platform_clone = platform.clone();
            let handle = tokio::task::spawn_blocking(move || {
                (
                    platform_clone.clone(),
                    pack_bundle_for_platform(&dir_clone, &opts, Some(&platform_clone)),
                )
            });
            pack_handles.push(handle);
        }

        // Also pack universal bundle if requested
        let universal_handle = if options.include_universal {
            let dir_clone = dir.to_path_buf();
            let opts = pack_options.clone();
            Some(tokio::task::spawn_blocking(move || {
                pack_bundle(&dir_clone, &opts)
            }))
        } else {
            None
        };

        // Wait for all packs to complete with spinner
        let spinner = Spinner::new("Packing bundles");
        let pack_results = futures_util::future::join_all(pack_handles).await;
        let universal_result = match universal_handle {
            Some(h) => Some(h.await),
            None => None,
        };
        spinner.succeed(Some("Bundles packed"));

        // Process pack results
        let mut icon_info: Option<(PathBuf, Vec<u8>, String)> = None;

        for result in pack_results {
            let (platform, pack_result) =
                result.map_err(|e| ToolError::Generic(format!("Pack task failed: {}", e)))?;
            match pack_result {
                Ok(pack_result) => {
                    let bundle_bytes = std::fs::read(&pack_result.output_path)
                        .map_err(|e| ToolError::Generic(format!("Failed to read bundle: {}", e)))?;
                    let bundle_checksum = compute_sha256(&bundle_bytes);
                    let bundle_filename = pack_result
                        .output_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();

                    println!(
                        "  · {}: {} ({})",
                        platform.bright_cyan(),
                        bundle_filename,
                        format_size(bundle_bytes.len() as u64)
                    );

                    version_manifest_artifacts.insert(
                        platform,
                        ArtifactEntry {
                            filename: bundle_filename.clone(),
                            size: bundle_bytes.len() as u64,
                            checksum: format!("sha256:{}", bundle_checksum),
                        },
                    );
                    files_to_upload.push((bundle_filename, bundle_bytes, bundle_checksum));

                    // Keep track of icon from first successful pack
                    if icon_info.is_none()
                        && let Some(icon_path) = &pack_result.icon_path
                        && let Ok(icon_bytes) = std::fs::read(icon_path)
                    {
                        let icon_checksum = compute_sha256(&icon_bytes);
                        icon_info = Some((icon_path.clone(), icon_bytes, icon_checksum));
                    }

                    let _ = std::fs::remove_file(&pack_result.output_path);
                    if let Some(icon_path) = &pack_result.icon_path {
                        let _ = std::fs::remove_file(icon_path);
                    }
                }
                Err(e) => {
                    return Err(ToolError::Generic(format!(
                        "Pack failed for {}: {}",
                        platform, e
                    )));
                }
            }
        }

        // Process universal bundle
        if let Some(result) = universal_result {
            let pack_result =
                result.map_err(|e| ToolError::Generic(format!("Pack task failed: {}", e)))?;
            match pack_result {
                Ok(pack_result) => {
                    let bundle_bytes = std::fs::read(&pack_result.output_path)
                        .map_err(|e| ToolError::Generic(format!("Failed to read bundle: {}", e)))?;
                    let bundle_checksum = compute_sha256(&bundle_bytes);
                    let bundle_filename = pack_result
                        .output_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();

                    println!(
                        "  · {}: {} ({})",
                        "universal".bright_cyan(),
                        bundle_filename,
                        format_size(bundle_bytes.len() as u64)
                    );

                    version_manifest_artifacts.insert(
                        "universal".to_string(),
                        ArtifactEntry {
                            filename: bundle_filename.clone(),
                            size: bundle_bytes.len() as u64,
                            checksum: format!("sha256:{}", bundle_checksum),
                        },
                    );
                    files_to_upload.push((bundle_filename, bundle_bytes, bundle_checksum));

                    // Use icon from universal bundle if not already set
                    if icon_info.is_none()
                        && let Some(icon_path) = &pack_result.icon_path
                        && let Ok(icon_bytes) = std::fs::read(icon_path)
                    {
                        let icon_checksum = compute_sha256(&icon_bytes);
                        icon_info = Some((icon_path.clone(), icon_bytes, icon_checksum));
                    }

                    let _ = std::fs::remove_file(&pack_result.output_path);
                    if let Some(icon_path) = &pack_result.icon_path {
                        let _ = std::fs::remove_file(icon_path);
                    }
                }
                Err(e) => {
                    return Err(ToolError::Generic(format!(
                        "Pack failed for universal: {}",
                        e
                    )));
                }
            }
        }

        // Add icon if found
        if let Some((_icon_path, icon_bytes, icon_checksum)) = icon_info
            && let Some(icon_name) = &manifest.icon
        {
            let icon_filename = icon_name.clone();
            println!(
                "  · icon: {} ({})",
                icon_filename,
                format_size(icon_bytes.len() as u64)
            );
            files_to_upload.push((icon_filename, icon_bytes, icon_checksum));
        }
    }

    // Generate version.json manifest
    let version_manifest = VersionManifest {
        name: tool_name.to_string(),
        version: version.to_string(),
        artifacts: version_manifest_artifacts,
    };
    let version_json = serde_json::to_string_pretty(&version_manifest)
        .map_err(|e| ToolError::Generic(format!("Failed to serialize version manifest: {}", e)))?;
    let version_json_bytes = version_json.as_bytes().to_vec();
    let version_json_checksum = compute_sha256(&version_json_bytes);

    println!(
        "  · version.json ({})",
        format_size(version_json_bytes.len() as u64)
    );

    files_to_upload.insert(
        0,
        (
            "version.json".to_string(),
            version_json_bytes,
            version_json_checksum,
        ),
    );

    if dry_run {
        println!(
            "\n  {} Dry run complete. Would upload {} files for {}/{}@{}",
            "✓".bright_green(),
            files_to_upload.len(),
            namespace.bright_blue(),
            tool_name.bright_cyan(),
            version.bright_white()
        );
        return Ok(());
    }

    // Create registry client with auth
    let token = token.ok_or_else(|| ToolError::Generic("Authentication required".into()))?;
    let client = RegistryClient::new().with_auth_token(&token);

    // Check if artifact exists
    println!();
    let spinner = Spinner::new(format!("Checking registry ({})", client.registry_url()));
    let artifact_exists = match client.artifact_exists(namespace, tool_name).await {
        Ok(exists) => {
            spinner.succeed(Some("Registry checked"));
            exists
        }
        Err(e) => {
            spinner.fail(None);
            return Err(e);
        }
    };

    if !artifact_exists {
        let spinner = Spinner::new("Creating artifact entry");
        match client
            .create_artifact(namespace, tool_name, description)
            .await
        {
            Ok(()) => {
                spinner.succeed(Some(&format!("Created {}/{}", namespace, tool_name)));
            }
            Err(e) => {
                spinner.fail(None);
                return Err(e);
            }
        }
    }

    // Build file specs for upload
    let file_specs: Vec<crate::registry::FileSpec> = files_to_upload
        .iter()
        .map(|(name, bytes, checksum)| crate::registry::FileSpec {
            name: name.clone(),
            size: bytes.len() as i64,
            sha256: checksum.clone(),
        })
        .collect();

    // Initiate upload
    println!(
        "\n  {} Uploading {} files in parallel",
        "→".bright_blue(),
        files_to_upload.len()
    );

    let upload_info = client
        .init_upload(namespace, tool_name, version, file_specs)
        .await?;

    // Upload all files in parallel
    let mp = MultiProgress::new();
    let style = ProgressStyle::default_bar()
        .template("  {msg:<25} [{bar:25.cyan/dim}] {bytes:>10}/{total_bytes:<10}")
        .unwrap()
        .progress_chars("█░░");

    let upload_handles: Vec<_> = files_to_upload
        .into_iter()
        .map(|(name, bytes, _checksum)| {
            let upload_target = upload_info.uploads.iter().find(|t| t.name == name).cloned();

            let pb = mp.add(ProgressBar::new(bytes.len() as u64));
            pb.set_style(style.clone());
            pb.set_message(name.clone());
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            let client = client.clone();
            tokio::spawn(async move {
                let upload_target = match upload_target {
                    Some(t) => t,
                    None => return Err(format!("No upload target for file: {}", name)),
                };

                let pb_arc = Arc::new(pb);
                let pb_clone = Arc::clone(&pb_arc);
                let result = client
                    .upload_bundle_with_progress(
                        &upload_target.upload_url,
                        &bytes,
                        &upload_target.content_type,
                        move |uploaded| {
                            pb_clone.set_position(uploaded);
                        },
                    )
                    .await;

                pb_arc.finish_and_clear();
                result.map_err(|e| format!("Upload failed for {}: {}", name, e))
            })
        })
        .collect();

    // Wait for all uploads to complete
    let upload_results = futures_util::future::join_all(upload_handles).await;

    // Check for failures
    for result in upload_results {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(ToolError::Generic(e)),
            Err(e) => return Err(ToolError::Generic(format!("Upload task failed: {}", e))),
        }
    }

    println!("  {} Upload complete", "✓".bright_green());

    // Publish the version with version.json as main_file
    println!();
    let spinner = Spinner::new("Publishing version");

    let manifest_json: serde_json::Value = serde_json::from_str(manifest_content)
        .map_err(|e| ToolError::Generic(format!("Failed to parse manifest: {}", e)))?;

    // Derive icon_url from manifest.icon + uploaded files
    let icon_url = manifest_json
        .get("icon")
        .and_then(|icon| icon.as_str())
        .and_then(|icon_filename| {
            upload_info
                .uploads
                .iter()
                .find(|target| target.name == icon_filename)
                .map(|target| target.cdn_url.clone())
        });

    let result = match client
        .publish_version(
            namespace,
            tool_name,
            &upload_info.upload_id,
            version,
            "version.json",
            manifest_json,
            description,
            icon_url,
        )
        .await
    {
        Ok(result) => {
            spinner.succeed(Some("Version published"));
            result
        }
        Err(e) => {
            spinner.fail(Some("Publishing failed"));
            return Err(e);
        }
    };

    println!(
        "\n  {} Published {}/{}@{} ({} artifacts)",
        "✓".bright_green(),
        namespace.bright_blue(),
        tool_name.bright_cyan(),
        result.version.bright_white(),
        version_manifest.artifacts.len()
    );
    println!(
        "  · {}/plugins/{}/{}",
        client.registry_url(),
        namespace,
        tool_name
    );

    Ok(())
}
