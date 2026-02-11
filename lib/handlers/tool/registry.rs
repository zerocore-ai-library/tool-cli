//! Registry command handlers.

use super::pack_cmd::format_size;
use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::format::format_description;
use crate::mcpb::McpbManifest;
use crate::pack::{PackError, PackOptions, compute_sha256, pack_bundle};
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use crate::resolver::FilePluginResolver;
use crate::styles::Spinner;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

/// Result of ensuring tools are installed.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct EnsureInstalledResult {
    /// Tools that were already installed locally.
    pub already_installed: Vec<String>,
    /// Tools that were auto-installed from the registry.
    pub auto_installed: Vec<String>,
    /// Tools that failed to install (tool_ref, error message).
    pub failed: Vec<(String, String)>,
}

/// Preflight result for ensuring tools are installed.
/// Contains information about what needs to be installed without actually installing.
#[derive(Debug, Default)]
pub struct EnsurePreflight {
    /// Tools that are already installed locally.
    pub already_installed: Vec<String>,
    /// Tools that need to be fetched from registry (tool_ref, preflight info).
    pub to_install: Vec<(String, RegistryPreflight)>,
    /// Non-namespaced tools that were not found locally.
    pub not_found_local: Vec<String>,
    /// Tools that failed preflight checks (tool_ref, error message).
    pub failed: Vec<(String, String)>,
}

/// Result of attempting to link a local tool.
#[derive(Debug, Clone)]
pub enum LinkResult {
    /// Successfully created a new symlink.
    Linked,
    /// Already linked to the same source.
    AlreadyLinked,
    /// A different source is already linked at the target path.
    Conflict(PathBuf),
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
#[derive(Debug)]
#[allow(dead_code)]
pub struct RegistryPreflight {
    /// Original tool reference.
    pub name: String,
    /// Namespace.
    pub namespace: String,
    /// Tool name.
    pub tool_name: String,
    /// Version to install.
    pub version: String,
    /// Download size in bytes.
    pub download_size: u64,
    /// Download URL.
    pub download_url: String,
    /// Target installation directory.
    pub target_dir: PathBuf,
    /// Temporary file path for download.
    pub temp_file: PathBuf,
}

/// Pre-flight information for a bundle file extraction.
#[derive(Debug)]
pub struct BundlePreflight {
    /// Original path to the bundle file.
    pub source_path: PathBuf,
    /// Display name (name@version).
    pub display_name: String,
    /// Number of entries in the bundle (for progress).
    pub entry_count: u64,
    /// Target installation directory.
    pub target_dir: PathBuf,
}

/// Result of pre-flight check.
enum PreflightResult {
    /// Ready for registry download
    Registry(RegistryPreflight),
    /// Ready for bundle extraction
    Bundle(BundlePreflight),
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
    let bundle = select_platform_bundle(&version_info, platform, &tool_name, &version)?;

    // Construct backend download URL for tracking
    let download_url = match &bundle.filename {
        Some(filename) => client.get_file_download_url(&namespace, &tool_name, &version, filename),
        None => client.get_download_url(&namespace, &tool_name, &version),
    };

    // Determine output path with correct extension
    let bundle_name = match &bundle.selected_platform {
        Some(p) => format!("{}@{}-{}.{}", tool_name, version, p, bundle.extension),
        None => format!("{}@{}.{}", tool_name, version, bundle.extension),
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
        download_size: bundle.size,
        download_url,
        output_path,
        platform: bundle.selected_platform,
    })
}

/// Result of platform bundle selection.
/// Contains info needed to construct backend download URL.
struct BundleSelection {
    /// Filename if downloading a specific file, None for main download
    filename: Option<String>,
    /// Download size in bytes
    size: u64,
    /// Selected platform (e.g., "darwin-arm64"), None for universal
    selected_platform: Option<String>,
    /// File extension (mcpb or mcpbx)
    extension: String,
}

/// Select the appropriate bundle based on platform preference.
/// Returns info needed to construct the backend download URL.
fn select_platform_bundle(
    version_info: &crate::registry::VersionInfo,
    platform: Option<&str>,
    tool_name: &str,
    version: &str,
) -> Result<BundleSelection, String> {
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
            return Ok(BundleSelection {
                filename: Some(filename.clone()),
                size: info.size,
                selected_platform: None,
                extension: ext.to_string(),
            });
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
            return Ok(BundleSelection {
                filename: None, // Use main download endpoint
                size,
                selected_platform: None,
                extension: ext.to_string(),
            });
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
                    return Ok(BundleSelection {
                        filename: Some(filename.clone()),
                        size: info.size,
                        selected_platform: Some(variant.to_string()),
                        extension: ext.to_string(),
                    });
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
            return Ok(BundleSelection {
                filename: Some(filename.clone()),
                size: info.size,
                selected_platform: None,
                extension: ext.to_string(),
            });
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
        return Ok(BundleSelection {
            filename: None, // Use main download endpoint
            size,
            selected_platform: None,
            extension: ext.to_string(),
        });
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
                    "  Downloaded {} {}",
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

    // Check if this is a bundle file (.mcpb or .mcpbx)
    if is_bundle_file(name) {
        return preflight_bundle_file(name);
    }

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
    let bundle = match select_platform_bundle(&version_info, platform, &tool_name, &version) {
        Ok(result) => result,
        Err(msg) => return PreflightResult::Failed(msg),
    };

    // Construct backend download URL for tracking
    let download_url = match &bundle.filename {
        Some(filename) => client.get_file_download_url(&namespace, &tool_name, &version, filename),
        None => client.get_download_url(&namespace, &tool_name, &version),
    };
    let download_size = bundle.size;

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
    let mut bundle_preflights = Vec::new();
    let mut local_count = 0usize;
    let mut installed_count = 0usize;
    let mut failed_count = 0usize;
    let mut bundle_installed = 0usize;
    let mut bundle_failed = 0usize;
    let mut already_installed = Vec::new();
    let mut failed = Vec::new();

    for (name, result) in names.iter().zip(preflight_results) {
        match result {
            PreflightResult::Registry(preflight) => {
                registry_preflights.push(preflight);
            }
            PreflightResult::Bundle(preflight) => {
                bundle_preflights.push(preflight);
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

    // Track counts for determining single-item display
    let registry_count = registry_preflights.len();
    let bundle_count = bundle_preflights.len();

    // Phase 2: Download and install registry packages
    if !registry_preflights.is_empty() {
        let count = registry_count;

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
                    installed_count += 1;
                }
                Err(msg) => {
                    pb.finish_and_clear();
                    println!("  {} Install failed: {}", "✗".bright_red(), msg);
                    failed_count += 1;
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
        }
    }

    // Phase 2b: Extract bundle files
    if !bundle_preflights.is_empty() {
        if bundle_count == 1 && registry_count == 0 && local_count == 0 {
            // Single bundle: simple output
            let preflight = bundle_preflights.remove(0);
            println!(
                "  {} Extracting {}",
                "→".bright_blue(),
                preflight.display_name.bright_cyan()
            );

            let pb = ProgressBar::new(preflight.entry_count);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("  [{bar:40.cyan/dim}] {pos}/{len} files")
                    .unwrap()
                    .progress_chars("█░░"),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            match extract_bundle_with_preflight(&preflight, pb.clone()) {
                Ok(()) => {
                    pb.finish_and_clear();
                    println!(
                        "  {} Installed {} {}",
                        "✓".bright_green(),
                        preflight.display_name.bright_cyan(),
                        "(extracted)".dimmed()
                    );
                    bundle_installed += 1;
                }
                Err(msg) => {
                    pb.finish_and_clear();
                    println!("  {} {}: {}", "✗".bright_red(), preflight.display_name, msg);
                    bundle_failed += 1;
                }
            }
        } else {
            // Multiple bundles: parallel extraction with multi-progress
            println!(
                "  {} Extracting {} {}",
                "→".bright_blue(),
                bundle_count.to_string().bright_cyan(),
                if bundle_count == 1 {
                    "bundle"
                } else {
                    "bundles"
                }
            );

            let mp = MultiProgress::new();
            let style = ProgressStyle::default_bar()
                .template("  {msg:<30} [{bar:25.cyan/dim}] {pos:>5}/{len:<5}")
                .unwrap()
                .progress_chars("█░░");

            // Create progress bars and spawn extraction tasks
            let handles: Vec<_> = bundle_preflights
                .into_iter()
                .map(|preflight| {
                    let pb = mp.add(ProgressBar::new(preflight.entry_count));
                    pb.set_style(style.clone());
                    pb.set_message(preflight.display_name.clone());
                    pb.enable_steady_tick(std::time::Duration::from_millis(100));

                    let display_name = preflight.display_name.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = extract_bundle_with_preflight(&preflight, pb.clone());
                        pb.finish_and_clear();
                        (display_name, result)
                    })
                })
                .collect();

            // Wait for all extractions to complete
            let results = futures_util::future::join_all(handles).await;

            // Print results
            for result in results {
                match result {
                    Ok((name, Ok(()))) => {
                        println!(
                            "  {} Installed {} {}",
                            "✓".bright_green(),
                            name.bright_cyan(),
                            "(extracted)".dimmed()
                        );
                        bundle_installed += 1;
                    }
                    Ok((name, Err(msg))) => {
                        println!("  {} {}: {}", "✗".bright_red(), name, msg);
                        bundle_failed += 1;
                    }
                    Err(_) => {
                        println!("  {} Extraction task panicked", "✗".bright_red());
                        bundle_failed += 1;
                    }
                }
            }
        }

        failed.extend(std::iter::repeat_n(
            ("bundle".to_string(), "extraction failed".to_string()),
            bundle_failed,
        ));
    }

    // Combined summary at the end
    let total_installed = installed_count + local_count;
    let total_failed = failed_count + bundle_failed;
    let has_any = total_installed > 0
        || bundle_installed > 0
        || total_failed > 0
        || !already_installed.is_empty();

    if has_any {
        println!();
        let mut parts = Vec::new();

        if total_installed > 0 {
            parts.push(format!(
                "Installed {} {}",
                total_installed.to_string().bright_green(),
                if total_installed == 1 {
                    "package"
                } else {
                    "packages"
                }
            ));
        }

        if bundle_installed > 0 {
            parts.push(format!(
                "extracted {} {}",
                bundle_installed.to_string().bright_green(),
                if bundle_installed == 1 {
                    "bundle"
                } else {
                    "bundles"
                }
            ));
        }

        if !already_installed.is_empty() {
            parts.push(format!(
                "{} already installed",
                already_installed.len().to_string().bright_cyan()
            ));
        }

        if total_failed > 0 {
            parts.push(format!("{} failed", total_failed.to_string().bright_red()));
        }

        println!("  {}", parts.join(", "));
    }

    Ok(())
}

/// Check which tools need to be installed (preflight phase, no side effects).
///
/// For each tool, this checks if it's already installed locally or needs to be
/// fetched from the registry. Returns preflight information without downloading.
pub async fn preflight_ensure(
    names: &[String],
    platform: Option<&str>,
) -> ToolResult<EnsurePreflight> {
    use futures_util::future::join_all;

    let resolver = FilePluginResolver::default();
    let mut result = EnsurePreflight::default();

    // Phase 1: Check which tools are already installed vs need fetching
    let mut to_check: Vec<String> = Vec::new();

    for name in names {
        // Check if already installed
        if let Ok(Some(_)) = resolver.resolve_tool(name).await {
            result.already_installed.push(name.clone());
            continue;
        }

        // Not installed - check if it has a namespace (can be fetched from registry)
        let plugin_ref = match name.parse::<PluginRef>() {
            Ok(p) => p,
            Err(_) => {
                result.not_found_local.push(name.clone());
                continue;
            }
        };

        if plugin_ref.namespace().is_some() {
            // Has namespace - can try to fetch from registry
            to_check.push(name.clone());
        } else {
            // No namespace - can't fetch, report as not found
            result.not_found_local.push(name.clone());
        }
    }

    // If nothing to check, return early
    if to_check.is_empty() {
        return Ok(result);
    }

    // Phase 2: Run preflight for tools to install (no output here - just gather info)
    let preflight_futures: Vec<_> = to_check
        .iter()
        .map(|name| preflight_tool(name, platform))
        .collect();
    let preflight_results = join_all(preflight_futures).await;

    // Separate successful preflights from failures
    for (name, preflight_result) in to_check.iter().zip(preflight_results) {
        match preflight_result {
            PreflightResult::Registry(preflight) => {
                result.to_install.push((name.clone(), preflight));
            }
            PreflightResult::AlreadyInstalled => {
                // Race condition: installed between check and preflight
                result.already_installed.push(name.clone());
            }
            PreflightResult::Local(_) | PreflightResult::Bundle(_) => {
                // Shouldn't happen since we're only processing namespaced tools
                result.already_installed.push(name.clone());
            }
            PreflightResult::Failed(msg) => {
                result.failed.push((name.clone(), msg));
            }
        }
    }

    Ok(result)
}

/// Execute the install based on preflight results.
///
/// Downloads and installs tools that were identified in the preflight phase.
pub async fn execute_ensure(preflight: EnsurePreflight) -> ToolResult<EnsureInstalledResult> {
    use futures_util::future::join_all;

    let mut result = EnsureInstalledResult {
        already_installed: preflight.already_installed,
        auto_installed: Vec::new(),
        failed: preflight
            .failed
            .into_iter()
            .chain(
                preflight
                    .not_found_local
                    .into_iter()
                    .map(|t| (t.clone(), format!("Tool '{}' not found locally", t))),
            )
            .collect(),
    };

    // If nothing to install, return early
    if preflight.to_install.is_empty() {
        return Ok(result);
    }

    let is_single = preflight.to_install.len() == 1;

    if is_single {
        println!(
            "  {} Fetching {} from registry...",
            "→".bright_blue(),
            preflight.to_install[0].0.bright_cyan()
        );
    } else {
        println!(
            "  {} Fetching {} tools from registry...",
            "→".bright_blue(),
            preflight.to_install.len().to_string().bright_cyan()
        );
    }

    let mut registry_preflights = preflight.to_install;

    if is_single && registry_preflights.len() == 1 {
        // Single package: show progress bar
        let (name, preflight) = registry_preflights.remove(0);

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
                result.auto_installed.push(name);
            }
            Err(msg) => {
                pb.finish_and_clear();
                result.failed.push((name, msg));
            }
        }
    } else {
        // Multiple packages: parallel download with multi-progress
        let mp = MultiProgress::new();
        let style = ProgressStyle::default_bar()
            .template("  {msg:<30} [{bar:25.cyan/dim}] {bytes:>10}/{total_bytes:<10}")
            .unwrap()
            .progress_chars("█░░");

        let handles: Vec<_> = registry_preflights
            .into_iter()
            .map(|(name, preflight)| {
                let pb = mp.add(ProgressBar::new(preflight.download_size));
                pb.set_style(style.clone());
                pb.set_message(format!("{}/{}", preflight.namespace, preflight.tool_name));
                pb.enable_steady_tick(std::time::Duration::from_millis(100));

                tokio::spawn(async move {
                    let install_result = download_and_install(preflight, pb.clone()).await;
                    pb.finish_and_clear();
                    (name, install_result)
                })
            })
            .collect();

        let download_results = join_all(handles).await;

        for task_result in download_results {
            match task_result {
                Ok((name, Ok(success))) => {
                    println!(
                        "  {} Installed {}/{}@{} ({})",
                        "✓".bright_green(),
                        success.namespace.bright_cyan(),
                        success.tool_name.bright_cyan(),
                        success.version.bright_cyan(),
                        format_size(success.size)
                    );
                    result.auto_installed.push(name);
                }
                Ok((name, Err(msg))) => {
                    result.failed.push((name, msg));
                }
                Err(e) => {
                    result
                        .failed
                        .push(("unknown".to_string(), format!("Task panicked: {}", e)));
                }
            }
        }
    }

    Ok(result)
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

/// Extract a bundle file using preflight info, with progress bar.
fn extract_bundle_with_preflight(
    preflight: &BundlePreflight,
    pb: ProgressBar,
) -> Result<(), String> {
    use std::io::Read;
    use zip::ZipArchive;

    // Create target directory
    std::fs::create_dir_all(&preflight.target_dir)
        .map_err(|e| format!("Failed to create target directory: {}", e))?;

    // Open the bundle
    let file = std::fs::File::open(&preflight.source_path)
        .map_err(|e| format!("Failed to open bundle: {}", e))?;

    let mut archive =
        ZipArchive::new(file).map_err(|e| format!("Failed to read ZIP archive: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read archive entry: {}", e))?;

        let entry_path = entry
            .enclosed_name()
            .ok_or_else(|| "Invalid entry path in archive".to_string())?;

        let dest_path = preflight.target_dir.join(entry_path);

        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
        }

        #[cfg(unix)]
        let unix_mode = entry.unix_mode();

        if entry.is_dir() {
            std::fs::create_dir_all(&dest_path)
                .map_err(|e| format!("Failed to create directory {:?}: {}", dest_path, e))?;
        } else {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| format!("Failed to read entry content: {}", e))?;

            std::fs::write(&dest_path, &content)
                .map_err(|e| format!("Failed to write file {:?}: {}", dest_path, e))?;

            #[cfg(unix)]
            if let Some(mode) = unix_mode {
                use std::os::unix::fs::PermissionsExt;
                let permissions = std::fs::Permissions::from_mode(mode);
                std::fs::set_permissions(&dest_path, permissions)
                    .map_err(|e| format!("Failed to set permissions on {:?}: {}", dest_path, e))?;
            }
        }

        pb.inc(1);
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

/// Check if the input looks like a bundle file (.mcpb or .mcpbx).
fn is_bundle_file(input: &str) -> bool {
    use crate::constants::{MCPB_EXT, MCPBX_EXT};

    let lower = input.to_lowercase();
    lower.ends_with(&format!(".{}", MCPB_EXT)) || lower.ends_with(&format!(".{}", MCPBX_EXT))
}

/// Pre-flight check for a bundle file. Validates the bundle and returns metadata.
fn preflight_bundle_file(path: &str) -> PreflightResult {
    use crate::constants::DEFAULT_TOOLS_PATH;
    use crate::mcpb::McpbManifest;
    use std::io::Read;
    use zip::ZipArchive;

    // Resolve the path
    let source_path = if path.starts_with('~') {
        match dirs::home_dir() {
            Some(home) => home.join(&path[2..]),
            None => {
                return PreflightResult::Failed("Could not determine home directory".to_string());
            }
        }
    } else {
        PathBuf::from(path)
    };

    let source_path = match source_path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            return PreflightResult::Failed(format!("Bundle file not found: {}", path));
        }
    };

    // Open the bundle as a ZIP archive
    let file = match std::fs::File::open(&source_path) {
        Ok(f) => f,
        Err(e) => {
            return PreflightResult::Failed(format!("Failed to open bundle: {}", e));
        }
    };

    let mut archive = match ZipArchive::new(file) {
        Ok(a) => a,
        Err(e) => {
            return PreflightResult::Failed(format!("Failed to read bundle (invalid ZIP): {}", e));
        }
    };

    let entry_count = archive.len() as u64;

    // Read manifest.json from inside the bundle
    let manifest: McpbManifest = {
        let mut manifest_entry = match archive.by_name(MCPB_MANIFEST_FILE) {
            Ok(entry) => entry,
            Err(_) => {
                return PreflightResult::Failed(format!("Bundle missing {}", MCPB_MANIFEST_FILE));
            }
        };

        let mut contents = String::new();
        if let Err(e) = manifest_entry.read_to_string(&mut contents) {
            return PreflightResult::Failed(format!("Failed to read manifest from bundle: {}", e));
        }

        match serde_json::from_str(&contents) {
            Ok(m) => m,
            Err(e) => {
                return PreflightResult::Failed(format!("Failed to parse manifest: {}", e));
            }
        }
    };

    let tool_name = match manifest.name.as_ref() {
        Some(n) => n.clone(),
        None => {
            return PreflightResult::Failed("manifest.json must include a name field".to_string());
        }
    };
    let version = manifest.version.clone();

    // Build target directory name (unnamespaced)
    let display_name = match &version {
        Some(v) => format!("{}@{}", tool_name, v),
        None => tool_name.clone(),
    };

    let target_dir = DEFAULT_TOOLS_PATH.join(&display_name);

    // Check if already installed
    if target_dir.join(MCPB_MANIFEST_FILE).exists() {
        return PreflightResult::AlreadyInstalled;
    }

    PreflightResult::Bundle(BundlePreflight {
        source_path,
        display_name,
        entry_count,
        target_dir,
    })
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
//--------------------------------------------------------------------------------------------------
// Functions: Local Tool Linking
//--------------------------------------------------------------------------------------------------

/// Link a local tool by creating a symlink in the tools directory.
///
/// Does NOT handle conflict resolution — returns `LinkResult::Conflict`
/// with the existing target path if a different source is already linked.
pub fn link_local_tool(
    source_path: &Path,
    tool_name: &str,
    version: Option<&str>,
) -> ToolResult<LinkResult> {
    use crate::constants::DEFAULT_TOOLS_PATH;

    let target_name = match version {
        Some(v) => format!("{}@{}", tool_name, v),
        None => tool_name.to_string(),
    };

    let target_path = DEFAULT_TOOLS_PATH.join(&target_name);

    // Check if target already exists
    if target_path.exists() || target_path.is_symlink() {
        if target_path.is_symlink()
            && let Ok(existing_target) = std::fs::read_link(&target_path)
        {
            if existing_target == source_path {
                return Ok(LinkResult::AlreadyLinked);
            }
            return Ok(LinkResult::Conflict(existing_target));
        }
        return Ok(LinkResult::Conflict(target_path));
    }

    // Ensure parent directory exists
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ToolError::Generic(format!("Failed to create tools directory: {}", e)))?;
    }

    create_symlink(source_path, &target_path)?;

    Ok(LinkResult::Linked)
}

/// Force-link a local tool by removing any existing target and creating a new symlink.
pub fn link_local_tool_force(
    source_path: &Path,
    tool_name: &str,
    version: Option<&str>,
) -> ToolResult<()> {
    use crate::constants::DEFAULT_TOOLS_PATH;

    let target_name = match version {
        Some(v) => format!("{}@{}", tool_name, v),
        None => tool_name.to_string(),
    };

    let target_path = DEFAULT_TOOLS_PATH.join(&target_name);

    // Remove existing if present
    if target_path.exists() || target_path.is_symlink() {
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

    create_symlink(source_path, &target_path)?;

    Ok(())
}

/// Create a symlink (platform-specific).
fn create_symlink(source: &Path, target: &Path) -> ToolResult<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(source, target)
        .map_err(|e| ToolError::Generic(format!("Failed to create symlink: {}", e)))?;

    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(source, target)
        .map_err(|e| ToolError::Generic(format!("Failed to create symlink: {}", e)))?;

    Ok(())
}

/// Remove a single installed tool.
async fn remove_tool(name: &str) -> (String, UninstallResult) {
    use crate::constants::DEFAULT_TOOLS_PATH;
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

    // Clean up empty parent namespace directory if applicable
    if let Some(parent_dir) = tool_dir.parent() {
        // Only clean up if the parent is not the root tools directory
        if parent_dir != DEFAULT_TOOLS_PATH.as_path() {
            // Check if the parent directory is now empty
            let is_empty = std::fs::read_dir(parent_dir)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);

            if is_empty {
                // Remove the empty namespace directory
                let _ = std::fs::remove_dir(parent_dir);
            }
        }
    }

    (resolved.plugin_ref.to_string(), UninstallResult::Removed)
}

/// Remove multiple installed tools.
pub async fn remove_tools(names: &[String], all: bool, yes: bool) -> ToolResult<()> {
    use futures_util::future::join_all;

    let resolver = FilePluginResolver::default();

    // Get list of tools to remove and orphaned entries
    let (tools_to_remove, orphans) = if all {
        if !names.is_empty() {
            return Err(ToolError::Generic(
                "Cannot specify tool names with --all".into(),
            ));
        }
        let installed = resolver.list_tools().await?;
        let orphans = resolver.list_orphaned_entries()?;

        if installed.is_empty() && orphans.is_empty() {
            println!("\n  No tools installed.\n");
            return Ok(());
        }
        (
            installed.into_iter().map(|t| t.to_string()).collect(),
            orphans,
        )
    } else {
        if names.is_empty() {
            return Err(ToolError::Generic(
                "No tools specified. Use --all to remove all tools.".into(),
            ));
        }
        (names.to_vec(), Vec::new())
    };

    let total_items = tools_to_remove.len() + orphans.len();

    // Confirm if --all and not --yes
    if all && !yes && total_items > 0 {
        println!();
        if !tools_to_remove.is_empty() {
            println!(
                "  {} This will uninstall {} tool(s)",
                "!".bright_yellow(),
                tools_to_remove.len()
            );
        }
        if !orphans.is_empty() {
            println!(
                "  {} This will clean up {} orphaned {}",
                "!".bright_yellow(),
                orphans.len(),
                if orphans.len() == 1 {
                    "entry"
                } else {
                    "entries"
                }
            );
        }
        println!();
        print!("  Continue? [y/N] ");
        io::stdout().flush().ok();

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| ToolError::Generic(format!("Failed to read input: {}", e)))?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!();
            println!("  {} Cancelled", "✗".bright_red());
            println!();
            return Ok(());
        }
        println!();
    }

    let mut removed_count = 0usize;
    let mut not_found_count = 0usize;
    let mut failed_count = 0usize;
    let mut orphans_cleaned = 0usize;

    // Remove tools
    if !tools_to_remove.is_empty() {
        let futures: Vec<_> = tools_to_remove
            .iter()
            .map(|name| remove_tool(name))
            .collect();
        let results = join_all(futures).await;

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
    }

    // Clean up orphaned entries
    for orphan_path in &orphans {
        let display_name = orphan_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| orphan_path.display().to_string());

        let result = if orphan_path.is_symlink() {
            // Remove broken symlink
            std::fs::remove_file(orphan_path)
        } else {
            // Remove directory
            std::fs::remove_dir_all(orphan_path)
        };

        match result {
            Ok(()) => {
                println!(
                    "  {} Cleaned up {}",
                    "✓".bright_green(),
                    display_name.bright_yellow()
                );
                orphans_cleaned += 1;
            }
            Err(e) => {
                println!(
                    "  {} Failed to clean up {}: {}",
                    "✗".bright_red(),
                    display_name,
                    e
                );
                failed_count += 1;
            }
        }
    }

    // Print summary if multiple items were processed
    if total_items > 1 {
        println!();
        if removed_count > 0 {
            println!(
                "  Removed {} {}",
                removed_count.to_string().bright_green(),
                if removed_count == 1 {
                    "package"
                } else {
                    "packages"
                }
            );
        }
        if orphans_cleaned > 0 {
            println!(
                "  Cleaned up {} orphaned {}",
                orphans_cleaned.to_string().bright_green(),
                if orphans_cleaned == 1 {
                    "entry"
                } else {
                    "entries"
                }
            );
        }
        if not_found_count > 0 {
            println!(
                "  Not found: {}",
                not_found_count.to_string().bright_yellow()
            );
        }
        if failed_count > 0 {
            println!("  Failed: {}", failed_count.to_string().bright_red());
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
        extract_icon: true,
        on_progress: None,
    };
    let pack_result = match pack_bundle(&dir, &pack_options) {
        Ok(result) => {
            spinner.succeed(Some("Bundle created"));
            result
        }
        Err(e) => {
            spinner.fail(None);
            return Err(match e {
                PackError::ValidationFailed(result) => ToolError::ValidationFailed(result),
                e => ToolError::Generic(format!("Pack failed: {}", e)),
            });
        }
    };

    // Read the bundle
    let bundle = std::fs::read(&pack_result.output_path)
        .map_err(|e| ToolError::Generic(format!("Failed to read bundle: {}", e)))?;
    let bundle_size = bundle.len() as u64;
    println!("  · Size: {}", format_size(bundle_size).bright_white());

    // Report icons if present
    if !pack_result.icons.is_empty() {
        let total_icon_size: u64 = pack_result.icons.iter().map(|i| i.bytes.len() as u64).sum();
        println!(
            "  · Icons: {} ({} file{})",
            format_size(total_icon_size).bright_white(),
            pack_result.icons.len(),
            if pack_result.icons.len() > 1 { "s" } else { "" }
        );
    }

    // Clean up temp bundle (icons are in memory, no files to clean)
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
        let categories = manifest.categories();
        match client
            .create_artifact(&namespace, tool_name, description, categories)
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

    // Add all icons to upload
    for icon in &pack_result.icons {
        files.push(crate::registry::FileSpec {
            name: icon.name.clone(),
            size: icon.bytes.len() as i64,
            sha256: icon.checksum.clone(),
        });
    }

    let file_count = files.len();
    println!(
        "\n  {} Uploading {} file{} in parallel",
        "→".bright_blue(),
        file_count,
        if file_count > 1 { "s" } else { "" }
    );
    let upload_info = client
        .init_upload(&namespace, tool_name, version, files)
        .await?;

    // Build list of files to upload
    let mut files_to_upload: Vec<(String, Vec<u8>)> = vec![(file_name.clone(), bundle)];
    for icon in &pack_result.icons {
        files_to_upload.push((icon.name.clone(), icon.bytes.clone()));
    }

    // Upload all files in parallel
    let mp = MultiProgress::new();
    let style = ProgressStyle::default_bar()
        .template("  {msg:<25} [{bar:25.cyan/dim}] {bytes:>10}/{total_bytes:<10}")
        .unwrap()
        .progress_chars("█░░");

    let upload_handles: Vec<_> = files_to_upload
        .into_iter()
        .map(|(name, bytes)| {
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

    // Derive icons array with CDN URLs
    let icons: Option<Vec<crate::registry::IconInfo>> = {
        let mut icons_list = Vec::new();

        // Build from manifest.icons array (preserves order, size, theme)
        if let Some(manifest_icons) = manifest_json.get("icons").and_then(|v| v.as_array()) {
            for icon in manifest_icons {
                let src = match icon.get("src").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => continue,
                };
                let cdn_url = upload_info
                    .uploads
                    .iter()
                    .find(|t| t.name == src)
                    .map(|t| t.cdn_url.clone());

                if let Some(url) = cdn_url {
                    icons_list.push(crate::registry::IconInfo {
                        src: url,
                        size: icon.get("size").and_then(|v| v.as_str()).map(String::from),
                        theme: icon.get("theme").and_then(|v| v.as_str()).map(String::from),
                    });
                }
            }
        }

        // Handle legacy `icon` field - prepend as primary if not already in list
        if let Some(icon_filename) = manifest_json.get("icon").and_then(|v| v.as_str()) {
            let already_in_list = icons_list
                .iter()
                .any(|i| i.src.ends_with(&format!("/{}", icon_filename)));

            if !already_in_list
                && let Some(target) = upload_info.uploads.iter().find(|t| t.name == icon_filename)
            {
                icons_list.insert(
                    0,
                    crate::registry::IconInfo {
                        src: target.cdn_url.clone(),
                        size: None,
                        theme: None,
                    },
                );
            }
        }

        if icons_list.is_empty() {
            None
        } else {
            Some(icons_list)
        }
    };

    let result = match client
        .publish_version(
            &namespace,
            tool_name,
            &upload_info.upload_id,
            version,
            &file_name,
            manifest_json,
            description,
            icons,
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
        let mut icons_extracted = false;
        let mut canonical_identity_hash: Option<String> = None;
        let mut canonical_bundle_path: Option<String> = None;

        for (platform, path) in &options.explicit_artifacts {
            let bytes = std::fs::read(path).map_err(|e| {
                ToolError::Generic(format!("Failed to read {}: {}", path.display(), e))
            })?;

            // Validate bundle: must be a valid ZIP with manifest.json
            let (bundle_manifest, manifest_bytes) =
                match crate::pack::read_manifest_from_bundle(&bytes) {
                    Ok(result) => result,
                    Err(e) => {
                        return Err(ToolError::Generic(format!(
                            "Invalid bundle {}: {}",
                            path.display(),
                            e
                        )));
                    }
                };

            // Compute identity hash for validation
            let identity_hash = crate::pack::compute_manifest_identity_hash(&manifest_bytes)
                .map_err(|e| {
                    ToolError::Generic(format!(
                        "Failed to compute identity hash for {}: {}",
                        path.display(),
                        e
                    ))
                })?;

            // Validate consistency: all bundles must have matching identity
            if let Some(ref canonical_hash) = canonical_identity_hash {
                if &identity_hash != canonical_hash {
                    let bundle_name = bundle_manifest.name.clone().unwrap_or_default();
                    let bundle_version = bundle_manifest.version.clone().unwrap_or_default();
                    return Err(ToolError::Generic(format!(
                        "Bundle mismatch: {} ({}@{}) has different identity than {}\n\
                         Critical fields (name, version, tools, user_config, etc.) must match across all bundles.",
                        path.display(),
                        bundle_name,
                        bundle_version,
                        canonical_bundle_path.as_deref().unwrap_or("first bundle")
                    )));
                }
            } else {
                canonical_identity_hash = Some(identity_hash);
                canonical_bundle_path = Some(path.display().to_string());
            }

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

            // Extract icons from the first bundle (all bundles should have the same icons)
            if !icons_extracted {
                match crate::pack::extract_icons_from_bundle(&bytes) {
                    Ok(icons) => {
                        if !icons.is_empty() {
                            let total_icon_size: u64 =
                                icons.iter().map(|i| i.bytes.len() as u64).sum();
                            println!(
                                "  · icons: {} ({} file{})",
                                format_size(total_icon_size),
                                icons.len(),
                                if icons.len() > 1 { "s" } else { "" }
                            );
                            for icon in icons {
                                files_to_upload.push((
                                    icon.name.clone(),
                                    icon.bytes.clone(),
                                    icon.checksum.clone(),
                                ));
                            }
                        }
                        icons_extracted = true;
                    }
                    Err(e) => {
                        // Log warning but continue - icons are optional
                        eprintln!(
                            "  {} Warning: Failed to extract icons from bundle: {}",
                            "⚠".yellow(),
                            e
                        );
                    }
                }
            }

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
            extract_icon: true,
            on_progress: None,
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
        let mut icons_info: Option<Vec<crate::pack::ExtractedIcon>> = None;

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

                    // Keep track of icons from first successful pack
                    if icons_info.is_none() && !pack_result.icons.is_empty() {
                        icons_info = Some(pack_result.icons.clone());
                    }

                    let _ = std::fs::remove_file(&pack_result.output_path);
                }
                Err(e) => {
                    return Err(match e {
                        PackError::ValidationFailed(result) => ToolError::ValidationFailed(result),
                        e => ToolError::Generic(format!("Pack failed for {}: {}", platform, e)),
                    });
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

                    // Use icons from universal bundle if not already set
                    if icons_info.is_none() && !pack_result.icons.is_empty() {
                        icons_info = Some(pack_result.icons.clone());
                    }

                    let _ = std::fs::remove_file(&pack_result.output_path);
                }
                Err(e) => {
                    return Err(match e {
                        PackError::ValidationFailed(result) => ToolError::ValidationFailed(result),
                        e => ToolError::Generic(format!("Pack failed for universal: {}", e)),
                    });
                }
            }
        }

        // Add all icons if found
        if let Some(ref icons) = icons_info {
            let total_icon_size: u64 = icons.iter().map(|i| i.bytes.len() as u64).sum();
            println!(
                "  · icons: {} ({} file{})",
                format_size(total_icon_size),
                icons.len(),
                if icons.len() > 1 { "s" } else { "" }
            );
            for icon in icons {
                files_to_upload.push((
                    icon.name.clone(),
                    icon.bytes.clone(),
                    icon.checksum.clone(),
                ));
            }
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
        let categories = manifest.categories();
        match client
            .create_artifact(namespace, tool_name, description, categories)
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

    // Derive icons array with CDN URLs
    let icons: Option<Vec<crate::registry::IconInfo>> = {
        let mut icons_list = Vec::new();

        // Build from manifest.icons array (preserves order, size, theme)
        if let Some(manifest_icons) = manifest_json.get("icons").and_then(|v| v.as_array()) {
            for icon in manifest_icons {
                let src = match icon.get("src").and_then(|v| v.as_str()) {
                    Some(s) => s,
                    None => continue,
                };
                let cdn_url = upload_info
                    .uploads
                    .iter()
                    .find(|t| t.name == src)
                    .map(|t| t.cdn_url.clone());

                if let Some(url) = cdn_url {
                    icons_list.push(crate::registry::IconInfo {
                        src: url,
                        size: icon.get("size").and_then(|v| v.as_str()).map(String::from),
                        theme: icon.get("theme").and_then(|v| v.as_str()).map(String::from),
                    });
                }
            }
        }

        // Handle legacy `icon` field - prepend as primary if not already in list
        if let Some(icon_filename) = manifest_json.get("icon").and_then(|v| v.as_str()) {
            let already_in_list = icons_list
                .iter()
                .any(|i| i.src.ends_with(&format!("/{}", icon_filename)));

            if !already_in_list
                && let Some(target) = upload_info.uploads.iter().find(|t| t.name == icon_filename)
            {
                icons_list.insert(
                    0,
                    crate::registry::IconInfo {
                        src: target.cdn_url.clone(),
                        size: None,
                        theme: None,
                    },
                );
            }
        }

        if icons_list.is_empty() {
            None
        } else {
            Some(icons_list)
        }
    };

    let result = match client
        .publish_version(
            namespace,
            tool_name,
            &upload_info.upload_id,
            version,
            "version.json",
            manifest_json,
            description,
            icons,
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
