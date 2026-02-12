//! Tool installation command handlers.

use super::pack_cmd::format_size;
use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use crate::resolver::FilePluginResolver;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};

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

/// Result of download_and_install with size info.
struct InstallSuccess {
    namespace: String,
    tool_name: String,
    version: String,
    size: u64,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Get the current platform identifier (e.g., "darwin-arm64", "linux-x64").
pub fn get_current_platform() -> String {
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
