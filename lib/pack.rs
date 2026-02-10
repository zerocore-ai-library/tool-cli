//! MCPB bundle packing.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::mcpb::McpbManifest;
use crate::validate::{ValidationResult, validate_manifest};
use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tar::Builder;
use thiserror::Error;
use walkdir::WalkDir;
use zip::DateTime as ZipDateTime;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Progress event emitted during packing.
#[derive(Debug, Clone)]
pub enum PackProgress {
    /// Starting to pack, with total file count.
    Started { total_files: usize },
    /// A file was added to the bundle.
    FileAdded { path: String, current: usize },
    /// Packing completed.
    Finished,
}

/// Callback type for progress events.
pub type ProgressCallback = Arc<dyn Fn(PackProgress) + Send + Sync>;

/// Error types for pack operations.
#[derive(Debug, Error)]
pub enum PackError {
    /// Validation failed before packing.
    #[error("validation failed")]
    ValidationFailed(ValidationResult),

    /// IO error during packing.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parsing error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Zip error.
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// Walkdir error.
    #[error("walkdir error: {0}")]
    WalkDir(#[from] walkdir::Error),

    /// Path strip error.
    #[error("path error: {0}")]
    StripPrefix(#[from] std::path::StripPrefixError),

    /// Ignore pattern error.
    #[error("ignore pattern error: {0}")]
    Ignore(#[from] ignore::Error),

    /// Manifest not found.
    #[error("manifest.json not found in {0}")]
    ManifestNotFound(PathBuf),
}

/// Options for packing.
#[derive(Clone)]
pub struct PackOptions {
    /// Output file path.
    pub output: Option<PathBuf>,

    /// Whether to validate before packing.
    pub validate: bool,

    /// Show files being added.
    pub verbose: bool,

    /// Whether to extract icon as a separate file (for registry upload).
    pub extract_icon: bool,

    /// Progress callback for reporting packing progress.
    pub on_progress: Option<ProgressCallback>,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            output: None,
            validate: true,
            verbose: false,
            extract_icon: false,
            on_progress: None,
        }
    }
}

impl std::fmt::Debug for PackOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackOptions")
            .field("output", &self.output)
            .field("validate", &self.validate)
            .field("verbose", &self.verbose)
            .field("extract_icon", &self.extract_icon)
            .field("on_progress", &self.on_progress.is_some())
            .finish()
    }
}

/// Extracted icon information.
#[derive(Debug, Clone)]
pub struct ExtractedIcon {
    /// Original filename from manifest (e.g., "icon.png", "icon-dark.png").
    pub name: String,

    /// Icon file bytes.
    pub bytes: Vec<u8>,

    /// SHA-256 checksum of the icon file.
    pub checksum: String,

    /// Icon size specification from manifest (e.g., "32x32").
    pub size: Option<String>,

    /// Theme variant from manifest (e.g., "light", "dark").
    pub theme: Option<String>,
}

/// Result of packing operation.
#[derive(Debug)]
pub struct PackResult {
    /// Path to the created bundle file.
    pub output_path: PathBuf,

    /// Number of files included.
    pub file_count: usize,

    /// Total uncompressed size in bytes.
    pub total_size: u64,

    /// Compressed size in bytes.
    pub compressed_size: u64,

    /// Files that were ignored.
    pub ignored_files: Vec<String>,

    /// Bundle format extension (`"mcpb"` or `"mcpbx"`).
    pub extension: String,

    /// SHA-256 checksum of the bundle.
    pub checksum: String,

    /// Extracted icons from manifest (if extract_icon was enabled).
    pub icons: Vec<ExtractedIcon>,
}

/// Options for collecting bundle files.
#[derive(Debug, Clone, Default)]
pub struct CollectOptions {
    /// Track ignored files for verbose output.
    pub track_ignored: bool,
}

/// A file entry collected for bundling.
#[derive(Debug)]
pub struct BundleEntry {
    /// Relative path within the bundle.
    pub relative_path: String,

    /// Whether this is a directory.
    pub is_dir: bool,

    /// File contents (empty for directories).
    pub contents: Vec<u8>,

    /// File modification time.
    pub modified: Option<std::time::SystemTime>,
}

/// Result of collecting bundle files.
#[derive(Debug)]
pub struct CollectResult {
    /// Files to include in the bundle.
    pub entries: Vec<BundleEntry>,

    /// Files that were ignored.
    pub ignored_files: Vec<String>,

    /// Total uncompressed size in bytes.
    pub total_size: u64,
}

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Built-in ignore patterns (cannot be overridden).
const BUILTIN_IGNORES: &[&str] = &[".git", "*.mcpb", "*.mcpbx"];

/// Default ignore patterns (can be overridden with !pattern in .mcpbignore).
const DEFAULT_IGNORES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    ".idea/",
    ".vscode/",
    "*.swp",
    "*.swo",
    ".mcpbignore",
    ".venv/",
];

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Pack a directory into an MCPB bundle.
pub fn pack_bundle(dir: &Path, options: &PackOptions) -> Result<PackResult, PackError> {
    // 1. Check manifest exists
    let manifest_path = dir.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(PackError::ManifestNotFound(dir.to_path_buf()));
    }

    // 2. Validate first (unless skipped)
    if options.validate {
        let validation = validate_manifest(dir);
        if !validation.is_valid() {
            return Err(PackError::ValidationFailed(validation));
        }
    }

    // 3. Read manifest for name/version
    let manifest: McpbManifest = serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;

    let name = manifest.name.as_deref().unwrap_or("bundle");
    let version = manifest.version.as_deref().unwrap_or("0.0.0");
    let ext = manifest.bundle_extension();

    // 4. Determine output path (inside the project directory)
    let output_path = options
        .output
        .clone()
        .unwrap_or_else(|| dir.join(format!("{}-{}.{}", name, version, ext)));

    // 5. Build ignore matcher
    let ignore_matcher = build_ignore_matcher(dir)?;

    // 6. Collect all files first (for progress reporting)
    let mut entries_to_add: Vec<(PathBuf, String, bool)> = Vec::new();
    let mut ignored_files = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| !is_builtin_ignored(e.path(), dir))
    {
        let entry = entry?;
        let path = entry.path();

        if path == dir {
            continue;
        }

        let relative_path = path.strip_prefix(dir)?;
        let path_str = relative_path.to_string_lossy().to_string();
        let is_dir = entry.file_type().is_dir();

        if ignore_matcher
            .matched_path_or_any_parents(relative_path, is_dir)
            .is_ignore()
        {
            if options.verbose {
                ignored_files.push(path_str);
            }
            continue;
        }

        entries_to_add.push((path.to_path_buf(), path_str, is_dir));
    }

    // Count only files (not directories)
    let total_files = entries_to_add
        .iter()
        .filter(|(_, _, is_dir)| !is_dir)
        .count();

    // Emit started event
    if let Some(ref cb) = options.on_progress {
        cb(PackProgress::Started { total_files });
    }

    // 7. Create zip archive
    let file = File::create(&output_path)?;
    let mut zip = ZipWriter::new(file);

    let zip_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut file_count = 0;
    let mut total_size = 0u64;

    // 8. Add files to archive with progress
    for (path, path_str, is_dir) in entries_to_add {
        let file_options = if let Ok(metadata) = std::fs::metadata(&path) {
            let mut opts = zip_options;

            if let Ok(modified) = metadata.modified()
                && let Some(dt) = system_time_to_zip_datetime(modified)
            {
                opts = opts.last_modified_time(dt);
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = metadata.permissions().mode();
                opts = opts.unix_permissions(mode);
            }

            opts
        } else {
            zip_options
        };

        if is_dir {
            let dir_path = format!("{}/", path_str);
            zip.add_directory(&dir_path, file_options)?;
        } else {
            let mut file = File::open(&path)?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;

            total_size += contents.len() as u64;
            file_count += 1;

            zip.start_file(&path_str, file_options)?;
            zip.write_all(&contents)?;

            // Emit progress
            if let Some(ref cb) = options.on_progress {
                cb(PackProgress::FileAdded {
                    path: path_str,
                    current: file_count,
                });
            }
        }
    }

    zip.finish()?;

    // Emit finished event
    if let Some(ref cb) = options.on_progress {
        cb(PackProgress::Finished);
    }

    let compressed_size = std::fs::metadata(&output_path)?.len();

    // Compute SHA-256 checksum of the bundle
    let bundle_bytes = std::fs::read(&output_path)?;
    let checksum = compute_sha256(&bundle_bytes);

    // Extract icons if requested (for registry upload)
    let icons = if options.extract_icon {
        extract_icons(dir, &manifest)?
    } else {
        Vec::new()
    };

    Ok(PackResult {
        output_path,
        file_count,
        total_size,
        compressed_size,
        ignored_files,
        extension: ext.to_string(),
        checksum,
        icons,
    })
}

/// Pack a directory into an MCPB bundle for a specific platform.
///
/// This creates a bundle with the manifest modified to contain only the
/// platform-specific overrides for the given platform. The bundle filename
/// includes the platform identifier (e.g., `tool-1.0.0-darwin-arm64.mcpb`).
pub fn pack_bundle_for_platform(
    dir: &Path,
    options: &PackOptions,
    platform: Option<&str>,
) -> Result<PackResult, PackError> {
    // 1. Check manifest exists
    let manifest_path = dir.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(PackError::ManifestNotFound(dir.to_path_buf()));
    }

    // 2. Validate first (unless skipped)
    if options.validate {
        let validation = validate_manifest(dir);
        if !validation.is_valid() {
            return Err(PackError::ValidationFailed(validation));
        }
    }

    // 3. Read and potentially modify manifest for platform
    let manifest_content = std::fs::read_to_string(&manifest_path)?;
    let mut manifest_json: serde_json::Value = serde_json::from_str(&manifest_content)?;

    // Modify manifest to contain only the specific platform's mcp_config
    if let Some(platform_key) = platform {
        modify_manifest_for_platform(&mut manifest_json, platform_key);
    }

    let manifest: McpbManifest = serde_json::from_value(manifest_json.clone())?;
    let name = manifest.name.as_deref().unwrap_or("bundle");
    let version = manifest.version.as_deref().unwrap_or("0.0.0");
    let ext = manifest.bundle_extension();

    // 4. Determine output path with platform suffix
    let output_filename = match platform {
        Some(p) => format!("{}-{}-{}.{}", name, version, p, ext),
        None => format!("{}-{}.{}", name, version, ext),
    };
    let output_path = options
        .output
        .clone()
        .unwrap_or_else(|| dir.join(&output_filename));

    // 5. Build ignore matcher
    let ignore_matcher = build_ignore_matcher(dir)?;

    // 6. Get platform-specific binary paths for filtering
    let (all_binary_paths, target_binary_path) = if platform.is_some() {
        let manifest_for_paths = serde_json::from_str::<serde_json::Value>(&manifest_content)?;
        let all_paths = get_all_platform_binary_paths(&manifest_for_paths);
        let target_path = platform.and_then(|p| get_platform_binary_path(&manifest_for_paths, p));
        (all_paths, target_path)
    } else {
        (Vec::new(), None)
    };

    // 7. Collect all files first (for progress reporting)
    let mut entries_to_add: Vec<(PathBuf, String, bool)> = Vec::new();
    let mut ignored_files = Vec::new();

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| !is_builtin_ignored(e.path(), dir))
    {
        let entry = entry?;
        let path = entry.path();

        if path == dir {
            continue;
        }

        let relative_path = path.strip_prefix(dir)?;
        let path_str = relative_path.to_string_lossy().to_string();
        let is_dir = entry.file_type().is_dir();

        if ignore_matcher
            .matched_path_or_any_parents(relative_path, is_dir)
            .is_ignore()
        {
            if options.verbose {
                ignored_files.push(path_str);
            }
            continue;
        }

        // Skip binaries for other platforms when packing platform-specific bundle
        if platform.is_some()
            && !is_dir
            && should_exclude_for_platform(
                &path_str,
                platform.unwrap_or_default(),
                &all_binary_paths,
                target_binary_path.as_deref(),
            )
        {
            if options.verbose {
                ignored_files.push(format!("{} (other platform binary)", path_str));
            }
            continue;
        }

        entries_to_add.push((path.to_path_buf(), path_str, is_dir));
    }

    // Count only files (not directories)
    let total_files = entries_to_add
        .iter()
        .filter(|(_, _, is_dir)| !is_dir)
        .count();

    // Emit started event
    if let Some(ref cb) = options.on_progress {
        cb(PackProgress::Started { total_files });
    }

    // 8. Create zip archive
    let file = File::create(&output_path)?;
    let mut zip = ZipWriter::new(file);

    let zip_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut file_count = 0;
    let mut total_size = 0u64;

    // 9. Add files to archive with progress
    for (path, path_str, is_dir) in entries_to_add {
        let file_options = if let Ok(metadata) = std::fs::metadata(&path) {
            let mut opts = zip_options;
            if let Ok(modified) = metadata.modified()
                && let Some(dt) = system_time_to_zip_datetime(modified)
            {
                opts = opts.last_modified_time(dt);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = metadata.permissions().mode();
                opts = opts.unix_permissions(mode);
            }
            opts
        } else {
            zip_options
        };

        if is_dir {
            let dir_path = format!("{}/", path_str);
            zip.add_directory(&dir_path, file_options)?;
        } else {
            // For manifest.json, use the modified content
            let contents = if path_str == MCPB_MANIFEST_FILE {
                serde_json::to_vec_pretty(&manifest_json)?
            } else {
                let mut file = File::open(&path)?;
                let mut contents = Vec::new();
                file.read_to_end(&mut contents)?;
                contents
            };

            total_size += contents.len() as u64;
            file_count += 1;

            zip.start_file(&path_str, file_options)?;
            zip.write_all(&contents)?;

            // Emit progress
            if let Some(ref cb) = options.on_progress {
                cb(PackProgress::FileAdded {
                    path: path_str,
                    current: file_count,
                });
            }
        }
    }

    zip.finish()?;

    // Emit finished event
    if let Some(ref cb) = options.on_progress {
        cb(PackProgress::Finished);
    }

    let compressed_size = std::fs::metadata(&output_path)?.len();
    let bundle_bytes = std::fs::read(&output_path)?;
    let checksum = compute_sha256(&bundle_bytes);

    // Extract icons if requested (for registry upload)
    let icons = if options.extract_icon {
        extract_icons(dir, &manifest)?
    } else {
        Vec::new()
    };

    Ok(PackResult {
        output_path,
        file_count,
        total_size,
        compressed_size,
        ignored_files,
        extension: ext.to_string(),
        checksum,
        icons,
    })
}

/// Extract the binary path for a specific platform from the manifest.
/// Returns the path relative to the bundle root (e.g., "dist/system-darwin-arm64").
///
/// Resolution order per mcpbx.md:
/// 1. _meta["store.tool.mcpb"].mcp_config.platform_overrides["{os}-{arch}"] (exact match)
/// 2. server.mcp_config.platform_overrides["{os}"] (os-only fallback)
/// 3. server.mcp_config.command (base config)
fn get_platform_binary_path(manifest: &serde_json::Value, platform: &str) -> Option<String> {
    // Helper to extract path from command string (removes ${__dirname}/ prefix)
    fn extract_path_from_command(command: &str) -> String {
        command
            .replace("${__dirname}/", "")
            .replace("${__dirname}", "")
    }

    // 1. Check _meta["store.tool.mcpb"].mcp_config.platform_overrides[platform]
    if let Some(command) = manifest
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|o| o.get(platform))
        .and_then(|p| p.get("command"))
        .and_then(|c| c.as_str())
    {
        return Some(extract_path_from_command(command));
    }

    // 2. Check server.mcp_config.platform_overrides[os] (os-only fallback)
    let os = platform.split('-').next().unwrap_or(platform);
    if let Some(command) = manifest
        .get("server")
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|o| o.get(os))
        .and_then(|p| p.get("command"))
        .and_then(|c| c.as_str())
    {
        return Some(extract_path_from_command(command));
    }

    // 3. Fall back to base command
    if let Some(command) = manifest
        .get("server")
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("command"))
        .and_then(|c| c.as_str())
    {
        return Some(extract_path_from_command(command));
    }

    None
}

/// Get all binary paths from platform overrides (to know what to exclude).
fn get_all_platform_binary_paths(manifest: &serde_json::Value) -> Vec<String> {
    let mut paths = Vec::new();

    fn extract_path_from_command(command: &str) -> String {
        command
            .replace("${__dirname}/", "")
            .replace("${__dirname}", "")
    }

    // Collect from _meta["store.tool.mcpb"].mcp_config.platform_overrides
    if let Some(overrides) = manifest
        .get("_meta")
        .and_then(|m| m.get("store.tool.mcpb"))
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|o| o.as_object())
    {
        for (_platform, config) in overrides {
            if let Some(command) = config.get("command").and_then(|c| c.as_str()) {
                paths.push(extract_path_from_command(command));
            }
        }
    }

    // Collect from server.mcp_config.platform_overrides
    if let Some(overrides) = manifest
        .get("server")
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|o| o.as_object())
    {
        for (_platform, config) in overrides {
            if let Some(command) = config.get("command").and_then(|c| c.as_str()) {
                paths.push(extract_path_from_command(command));
            }
        }
    }

    // Collect base command
    if let Some(command) = manifest
        .get("server")
        .and_then(|s| s.get("mcp_config"))
        .and_then(|c| c.get("command"))
        .and_then(|c| c.as_str())
    {
        paths.push(extract_path_from_command(command));
    }

    paths.sort();
    paths.dedup();
    paths
}

/// Modify manifest JSON to contain only the specified platform's mcp_config.
///
/// This:
/// 1. Applies the platform-specific config to mcp_config
/// 2. Removes all platform_overrides
/// 3. Updates entry_point to the platform-specific binary
/// 4. Updates compatibility.platforms to only this platform
fn modify_manifest_for_platform(manifest: &mut serde_json::Value, platform: &str) {
    // Get the platform binary path first (before we modify anything)
    let platform_binary = get_platform_binary_path(manifest, platform);

    // Extract OS from platform (e.g., "darwin" from "darwin-arm64")
    let os = platform.split('-').next().unwrap_or(platform);

    // 1. Apply _meta["store.tool.mcpb"].mcp_config.platform_overrides config
    if let Some(meta) = manifest.get_mut("_meta")
        && let Some(store_meta) = meta.get_mut("store.tool.mcpb")
    {
        // Apply platform config to mcp_config
        if let Some(mcp_config) = store_meta.get_mut("mcp_config") {
            let platform_config = mcp_config
                .get("platform_overrides")
                .and_then(|o| o.get(platform))
                .cloned();
            if let Some(config) = platform_config {
                apply_platform_config(mcp_config, &config);
            }
            // Remove platform_overrides
            if let Some(obj) = mcp_config.as_object_mut() {
                obj.remove("platform_overrides");
            }
        }

        // Update _meta compatibility.platforms to only this platform
        if let Some(compat) = store_meta.get_mut("compatibility")
            && let Some(obj) = compat.as_object_mut()
        {
            obj.insert("platforms".to_string(), serde_json::json!([platform]));
        }
    }

    // 2. Apply server.mcp_config.platform_overrides config (os-level fallback)
    if let Some(server) = manifest.get_mut("server") {
        // Update entry_point to the platform-specific binary
        if let Some(binary_path) = &platform_binary
            && let Some(obj) = server.as_object_mut()
        {
            obj.insert("entry_point".to_string(), serde_json::json!(binary_path));
        }

        if let Some(mcp_config) = server.get_mut("mcp_config") {
            // First try exact platform match, then os-only fallback
            let platform_config = mcp_config
                .get("platform_overrides")
                .and_then(|o| o.get(platform).or_else(|| o.get(os)))
                .cloned();
            if let Some(config) = platform_config {
                apply_platform_config(mcp_config, &config);
            }
            // Remove platform_overrides
            if let Some(obj) = mcp_config.as_object_mut() {
                obj.remove("platform_overrides");
            }
        }
    }

    // 3. Update root compatibility.platforms to only the OS (for spec compliance)
    if let Some(compat) = manifest.get_mut("compatibility")
        && let Some(obj) = compat.as_object_mut()
    {
        obj.insert("platforms".to_string(), serde_json::json!([os]));
    }
}

/// Apply platform-specific config values to the mcp_config.
fn apply_platform_config(mcp_config: &mut serde_json::Value, platform_config: &serde_json::Value) {
    if let (Some(base), Some(override_obj)) =
        (mcp_config.as_object_mut(), platform_config.as_object())
    {
        for (key, value) in override_obj {
            base.insert(key.clone(), value.clone());
        }
    }
}

/// Check if a file path should be excluded for a platform-specific bundle.
/// Returns true if the file is a binary for a DIFFERENT platform.
fn should_exclude_for_platform(
    file_path: &str,
    _target_platform: &str,
    all_binary_paths: &[String],
    target_binary_path: Option<&str>,
) -> bool {
    // If this file matches the target platform's binary, include it
    if let Some(target) = target_binary_path
        && file_path == target
    {
        return false; // Don't exclude
    }

    // If this file is one of the platform binaries but NOT our target, exclude it
    for binary_path in all_binary_paths {
        if file_path == binary_path
            && let Some(target) = target_binary_path
            && binary_path != target
        {
            return true; // Exclude - it's a different platform's binary
        }
    }

    false // Include by default
}

/// Collect files from a directory for bundling, applying ignore patterns.
///
/// This is the shared logic used by both `pack_bundle` (zip) and `create_tool_bundle` (tar.gz).
pub fn collect_bundle_files(
    dir: &Path,
    options: &CollectOptions,
) -> Result<CollectResult, PackError> {
    let ignore_matcher = build_ignore_matcher(dir)?;

    let mut entries = Vec::new();
    let mut ignored_files = Vec::new();
    let mut total_size = 0u64;

    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| !is_builtin_ignored(e.path(), dir))
    {
        let entry = entry?;
        let path = entry.path();

        // Skip the root directory itself
        if path == dir {
            continue;
        }

        let relative_path = path.strip_prefix(dir)?;
        let path_str = relative_path.to_string_lossy().to_string();
        let is_dir = entry.file_type().is_dir();

        // Check if should be ignored by .mcpbignore patterns
        if ignore_matcher
            .matched_path_or_any_parents(relative_path, is_dir)
            .is_ignore()
        {
            if options.track_ignored {
                ignored_files.push(path_str);
            }
            continue;
        }

        let modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());

        if is_dir {
            entries.push(BundleEntry {
                relative_path: path_str,
                is_dir: true,
                contents: Vec::new(),
                modified,
            });
        } else {
            let mut file = File::open(path)?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;

            total_size += contents.len() as u64;

            entries.push(BundleEntry {
                relative_path: path_str,
                is_dir: false,
                contents,
                modified,
            });
        }
    }

    Ok(CollectResult {
        entries,
        ignored_files,
        total_size,
    })
}

/// Create a tar.gz bundle from a tool directory for registry upload.
///
/// This applies the same filtering as `pack_bundle` (.mcpbignore, builtin ignores)
/// but outputs tar.gz bytes suitable for registry upload.
pub fn create_tool_bundle(dir: &Path) -> Result<Vec<u8>, PackError> {
    // 1. Check manifest exists
    let manifest_path = dir.join(MCPB_MANIFEST_FILE);
    if !manifest_path.exists() {
        return Err(PackError::ManifestNotFound(dir.to_path_buf()));
    }

    // 2. Validate manifest
    let validation = validate_manifest(dir);
    if !validation.is_valid() {
        return Err(PackError::ValidationFailed(validation));
    }

    // 3. Collect files
    let collect_result = collect_bundle_files(dir, &CollectOptions::default())?;

    // 4. Create tar.gz archive
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());

    {
        let mut builder = Builder::new(&mut encoder);

        for entry in collect_result.entries {
            if entry.is_dir {
                // Skip directories - tar will create them automatically
                continue;
            }

            let mut header = tar::Header::new_gnu();
            header
                .set_path(&entry.relative_path)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            header.set_size(entry.contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();

            builder
                .append(&header, entry.contents.as_slice())
                .map_err(std::io::Error::other)?;
        }

        builder.finish().map_err(std::io::Error::other)?;
    }

    let compressed = encoder.finish().map_err(std::io::Error::other)?;

    Ok(compressed)
}

/// Build gitignore-style matcher from default patterns and .mcpbignore.
fn build_ignore_matcher(dir: &Path) -> Result<Gitignore, PackError> {
    let mut builder = GitignoreBuilder::new(dir);

    // Add default patterns
    for pattern in DEFAULT_IGNORES {
        builder.add_line(None, pattern)?;
    }

    // Add .mcpbignore if it exists
    let mcpbignore = dir.join(".mcpbignore");
    if mcpbignore.exists() {
        builder.add(&mcpbignore);
    }

    Ok(builder.build()?)
}

/// Check if a path matches builtin ignore patterns (cannot be overridden).
fn is_builtin_ignored(path: &Path, base: &Path) -> bool {
    let relative = match path.strip_prefix(base) {
        Ok(r) => r,
        Err(_) => return false,
    };

    for component in relative.components() {
        let component_str = component.as_os_str().to_string_lossy();

        // Check each builtin pattern
        for pattern in BUILTIN_IGNORES {
            if let Some(suffix) = pattern.strip_prefix('*') {
                // Glob pattern like *.mcpb
                if component_str.ends_with(suffix) {
                    return true;
                }
            } else if component_str == *pattern {
                return true;
            }
        }
    }

    false
}

/// Convert SystemTime to zip DateTime, preserving file modification times.
fn system_time_to_zip_datetime(time: std::time::SystemTime) -> Option<ZipDateTime> {
    let duration = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    let secs = duration.as_secs();

    // Convert Unix timestamp to date/time components
    // Using a simple algorithm for UTC time
    let days = (secs / 86400) as i64;
    let time_of_day = secs % 86400;

    let hours = (time_of_day / 3600) as u8;
    let minutes = ((time_of_day % 3600) / 60) as u8;
    let seconds = (time_of_day % 60) as u8;

    // Calculate year, month, day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days + 719468); // Days since year 0

    // ZIP format only supports years 1980-2107
    if !(1980..=2107).contains(&year) {
        return None;
    }

    ZipDateTime::from_date_and_time(year as u16, month as u8, day as u8, hours, minutes, seconds)
        .ok()
}

/// Convert days since year 0 to (year, month, day).
/// Based on the algorithm from Howard Hinnant's date library.
fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let era = if days >= 0 {
        days / 146097
    } else {
        (days - 146096) / 146097
    };
    let doe = (days - era * 146097) as u32; // Day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // Year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m, d)
}

/// Compute SHA-256 checksum of data and return as hex string.
pub fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Extract all icons from manifest.
///
/// Processes both the legacy `icon` field and the `icons` array.
/// The `icon` field is prepended as the primary icon if not already in the array.
fn extract_icons(dir: &Path, manifest: &McpbManifest) -> Result<Vec<ExtractedIcon>, PackError> {
    let mut extracted = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Process `icons` array first (preserves order, size, theme)
    if let Some(ref icons) = manifest.icons {
        for icon in icons {
            // Skip duplicates
            if seen.contains(&icon.src) {
                continue;
            }

            // Skip https:// URLs (can't extract remote icons)
            if icon.src.starts_with("https://") {
                continue;
            }

            let icon_path = dir.join(&icon.src);
            if icon_path.exists() {
                let bytes = std::fs::read(&icon_path)?;
                let checksum = compute_sha256(&bytes);
                extracted.push(ExtractedIcon {
                    name: icon.src.clone(),
                    bytes,
                    checksum,
                    size: icon.size.clone(),
                    theme: icon.theme.clone(),
                });
                seen.insert(icon.src.clone());
            }
        }
    }

    // 2. Process legacy `icon` field (prepend as primary if not already in list)
    if let Some(ref icon_name) = manifest.icon {
        // Skip if already processed from icons array
        if !seen.contains(icon_name) {
            // Skip https:// URLs
            if !icon_name.starts_with("https://") {
                let icon_path = dir.join(icon_name);
                if icon_path.exists() {
                    let bytes = std::fs::read(&icon_path)?;
                    let checksum = compute_sha256(&bytes);
                    // Prepend as primary icon
                    extracted.insert(
                        0,
                        ExtractedIcon {
                            name: icon_name.clone(),
                            bytes,
                            checksum,
                            size: None,
                            theme: None,
                        },
                    );
                }
            }
        }
    }

    Ok(extracted)
}

/// Read manifest.json from an MCPB bundle (ZIP file).
///
/// Returns the parsed manifest and the raw manifest JSON bytes.
pub fn read_manifest_from_bundle(
    bundle_bytes: &[u8],
) -> Result<(McpbManifest, Vec<u8>), PackError> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let cursor = Cursor::new(bundle_bytes);
    let mut archive = ZipArchive::new(cursor)?;

    // Find and read manifest.json
    let mut manifest_entry = archive.by_name(MCPB_MANIFEST_FILE)?;

    let mut manifest_bytes = Vec::new();
    manifest_entry.read_to_end(&mut manifest_bytes)?;

    let manifest: McpbManifest = serde_json::from_slice(&manifest_bytes)?;

    Ok((manifest, manifest_bytes))
}

/// Extract icons from an MCPB bundle (ZIP file).
///
/// Reads the manifest from the bundle and extracts all referenced icons.
/// Processes both the legacy `icon` field and the `icons` array.
pub fn extract_icons_from_bundle(bundle_bytes: &[u8]) -> Result<Vec<ExtractedIcon>, PackError> {
    use std::io::Cursor;
    use zip::ZipArchive;

    let (manifest, _) = read_manifest_from_bundle(bundle_bytes)?;

    let cursor = Cursor::new(bundle_bytes);
    let mut archive: ZipArchive<Cursor<&[u8]>> = ZipArchive::new(cursor)?;

    let mut extracted = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Process `icons` array first (preserves order, size, theme)
    if let Some(ref icons) = manifest.icons {
        for icon in icons {
            // Skip duplicates
            if seen.contains(&icon.src) {
                continue;
            }

            // Skip https:// URLs (can't extract remote icons)
            if icon.src.starts_with("https://") {
                continue;
            }

            // Try to read the icon from the archive
            if let Ok(mut entry) = archive.by_name(&icon.src) {
                let mut bytes = Vec::new();
                if entry.read_to_end(&mut bytes).is_ok() {
                    let checksum = compute_sha256(&bytes);
                    extracted.push(ExtractedIcon {
                        name: icon.src.clone(),
                        bytes,
                        checksum,
                        size: icon.size.clone(),
                        theme: icon.theme.clone(),
                    });
                    seen.insert(icon.src.clone());
                }
            }
        }
    }

    // 2. Process legacy `icon` field (prepend as primary if not already in list)
    if let Some(ref icon_name) = manifest.icon {
        // Skip if already processed from icons array
        if !seen.contains(icon_name) {
            // Skip https:// URLs
            if !icon_name.starts_with("https://") {
                // Need to reopen archive since we consumed it above
                let cursor = Cursor::new(bundle_bytes);
                if let Ok(mut archive) = ZipArchive::new(cursor)
                    && let Ok(mut entry) = archive.by_name(icon_name)
                {
                    let mut bytes: Vec<u8> = Vec::new();
                    if entry.read_to_end(&mut bytes).is_ok() {
                        let checksum = compute_sha256(&bytes);
                        // Prepend as primary icon
                        extracted.insert(
                            0,
                            ExtractedIcon {
                                name: icon_name.clone(),
                                bytes,
                                checksum,
                                size: None,
                                theme: None,
                            },
                        );
                    }
                }
            }
        }
    }

    Ok(extracted)
}

/// Compute identity hash from manifest JSON bytes.
///
/// Extracts critical fields that must match across all platform bundles,
/// normalizes them, and returns a SHA-256 hash. Platform-specific fields
/// are excluded from the hash.
///
/// Critical fields: name, version, manifest_version, description, author,
/// server.type, tools, prompts, user_config, system_config, icon, icons,
/// license, keywords, and static_responses from _meta.
pub fn compute_manifest_identity_hash(manifest_bytes: &[u8]) -> Result<String, PackError> {
    let manifest_json: serde_json::Value = serde_json::from_slice(manifest_bytes)?;

    // Extract critical fields into a normalized structure
    let mut identity = serde_json::Map::new();

    // Core identity fields
    if let Some(v) = manifest_json.get("name") {
        identity.insert("name".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("version") {
        identity.insert("version".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("manifest_version") {
        identity.insert("manifest_version".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("description") {
        identity.insert("description".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("display_name") {
        identity.insert("display_name".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("long_description") {
        identity.insert("long_description".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("author") {
        identity.insert("author".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("license") {
        identity.insert("license".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("keywords") {
        identity.insert("keywords".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("homepage") {
        identity.insert("homepage".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("repository") {
        identity.insert("repository".to_string(), v.clone());
    }

    // Server type (must match, but not entry_point or mcp_config)
    if let Some(server) = manifest_json.get("server") {
        if let Some(v) = server.get("type") {
            identity.insert("server.type".to_string(), v.clone());
        }
        if let Some(v) = server.get("transport") {
            identity.insert("server.transport".to_string(), v.clone());
        }
    }

    // Capabilities
    if let Some(v) = manifest_json.get("tools") {
        identity.insert("tools".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("tools_generated") {
        identity.insert("tools_generated".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("prompts") {
        identity.insert("prompts".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("prompts_generated") {
        identity.insert("prompts_generated".to_string(), v.clone());
    }

    // Configuration schemas
    if let Some(v) = manifest_json.get("user_config") {
        identity.insert("user_config".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("system_config") {
        identity.insert("system_config".to_string(), v.clone());
    }

    // Icons
    if let Some(v) = manifest_json.get("icon") {
        identity.insert("icon".to_string(), v.clone());
    }
    if let Some(v) = manifest_json.get("icons") {
        identity.insert("icons".to_string(), v.clone());
    }

    // Static responses from _meta (full tool schemas)
    if let Some(meta) = manifest_json.get("_meta")
        && let Some(store_meta) = meta.get("store.tool.mcpb")
    {
        if let Some(v) = store_meta.get("static_responses") {
            identity.insert("static_responses".to_string(), v.clone());
        }
        if let Some(v) = store_meta.get("scripts") {
            identity.insert("scripts".to_string(), v.clone());
        }
    }

    // Serialize with sorted keys for consistent hashing
    let normalized = serde_json::to_string(&serde_json::Value::Object(identity))?;
    Ok(compute_sha256(normalized.as_bytes()))
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_builtin_ignored() {
        let dir = TempDir::new().unwrap();
        let base = dir.path();

        assert!(is_builtin_ignored(&base.join(".git"), base));
        assert!(is_builtin_ignored(&base.join(".git/config"), base));
        assert!(is_builtin_ignored(&base.join("foo.mcpb"), base));
        assert!(is_builtin_ignored(&base.join("dist/bar.mcpb"), base));
        assert!(is_builtin_ignored(&base.join("foo.mcpbx"), base));
        assert!(is_builtin_ignored(&base.join("dist/bar.mcpbx"), base));
        assert!(!is_builtin_ignored(&base.join("manifest.json"), base));
        assert!(!is_builtin_ignored(&base.join("server/index.js"), base));
    }

    #[test]
    fn test_build_ignore_matcher() {
        let dir = TempDir::new().unwrap();
        let matcher = build_ignore_matcher(dir.path()).unwrap();

        // Default patterns should be ignored
        assert!(
            matcher
                .matched_path_or_any_parents(Path::new(".DS_Store"), false)
                .is_ignore()
        );
        assert!(
            matcher
                .matched_path_or_any_parents(Path::new(".idea"), true)
                .is_ignore()
        );
        assert!(
            matcher
                .matched_path_or_any_parents(Path::new("test.swp"), false)
                .is_ignore()
        );

        // Regular files should not be ignored
        assert!(
            !matcher
                .matched_path_or_any_parents(Path::new("manifest.json"), false)
                .is_ignore()
        );
    }

    #[test]
    fn test_mcpbignore_patterns() {
        let dir = TempDir::new().unwrap();

        // Create .mcpbignore with custom patterns
        std::fs::write(
            dir.path().join(".mcpbignore"),
            "# Comment\n*.log\n!important.log\nbuild/\n",
        )
        .unwrap();

        let matcher = build_ignore_matcher(dir.path()).unwrap();

        // Custom patterns
        assert!(
            matcher
                .matched_path_or_any_parents(Path::new("debug.log"), false)
                .is_ignore()
        );
        assert!(
            !matcher
                .matched_path_or_any_parents(Path::new("important.log"), false)
                .is_ignore()
        );
        assert!(
            matcher
                .matched_path_or_any_parents(Path::new("build"), true)
                .is_ignore()
        );
    }

    #[test]
    fn test_pack_missing_manifest() {
        let dir = TempDir::new().unwrap();
        let result = pack_bundle(dir.path(), &PackOptions::default());
        assert!(matches!(result, Err(PackError::ManifestNotFound(_))));
    }

    #[test]
    fn test_pack_validation_failed() {
        let dir = TempDir::new().unwrap();

        // Create invalid manifest (missing required fields)
        let manifest = r#"{
            "manifest_version": "0.3",
            "server": { "type": "node" }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();

        let result = pack_bundle(dir.path(), &PackOptions::default());
        assert!(matches!(result, Err(PackError::ValidationFailed(_))));
    }

    #[test]
    fn test_pack_skip_validation() {
        let dir = TempDir::new().unwrap();

        // Create minimal manifest
        let manifest = r#"{
            "manifest_version": "0.3",
            "name": "test-pack-skip-validation",
            "version": "1.0.0",
            "server": { "type": "node" }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();

        let options = PackOptions {
            validate: false,
            ..Default::default()
        };

        let result = pack_bundle(dir.path(), &options).unwrap();
        assert_eq!(result.file_count, 1); // Just manifest.json
        assert!(result.output_path.exists());

        // Cleanup
        std::fs::remove_file(&result.output_path).ok();
    }

    #[test]
    fn test_pack_with_files() {
        let dir = TempDir::new().unwrap();

        // Create valid manifest
        std::fs::create_dir_all(dir.path().join("server")).unwrap();
        std::fs::write(dir.path().join("server/index.js"), "// entry").unwrap();

        let manifest = r#"{
            "manifest_version": "0.3",
            "name": "test-pack-with-files",
            "version": "1.0.0",
            "description": "Test tool",
            "author": { "name": "Test" },
            "server": {
                "type": "node",
                "entry_point": "server/index.js",
                "mcp_config": { "command": "node", "args": [] }
            }
        }"#;
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();

        let options = PackOptions {
            validate: false, // Skip validation to avoid warnings about node_modules
            ..Default::default()
        };

        let result = pack_bundle(dir.path(), &options).unwrap();
        assert_eq!(result.file_count, 2); // manifest.json + server/index.js
        assert!(result.output_path.exists());
        let path_str = result.output_path.to_string_lossy();
        assert!(
            path_str.ends_with(".mcpb") || path_str.ends_with(".mcpbx"),
            "expected .mcpb or .mcpbx extension, got: {}",
            path_str
        );

        // Cleanup
        std::fs::remove_file(&result.output_path).ok();
    }
}
