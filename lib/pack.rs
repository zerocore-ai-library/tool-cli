//! MCPB bundle packing.

use crate::constants::MCPB_MANIFEST_FILE;
use crate::mcpb::McpbManifest;
use crate::validate::{ValidationResult, validate_manifest};
use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tar::Builder;
use thiserror::Error;
use walkdir::WalkDir;
use zip::DateTime as ZipDateTime;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

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
#[derive(Debug, Clone)]
pub struct PackOptions {
    /// Output file path.
    pub output: Option<PathBuf>,

    /// Whether to validate before packing.
    pub validate: bool,

    /// Include dotfiles (except .git/).
    pub include_dotfiles: bool,

    /// Show files being added.
    pub verbose: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            output: None,
            validate: true,
            include_dotfiles: false,
            verbose: false,
        }
    }
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
}

/// Options for collecting bundle files.
#[derive(Debug, Clone, Default)]
pub struct CollectOptions {
    /// Include dotfiles (except .git/).
    pub include_dotfiles: bool,

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

    // 6. Create zip archive
    let file = File::create(&output_path)?;
    let mut zip = ZipWriter::new(file);

    let zip_options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut file_count = 0;
    let mut total_size = 0u64;
    let mut ignored_files = Vec::new();

    // 7. Walk directory and add files
    // follow_links(true) ensures symlinks to directories are correctly identified as directories
    for entry in WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            // Always skip .git directory
            !is_builtin_ignored(e.path(), dir)
        })
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
            if options.verbose {
                ignored_files.push(path_str);
            }
            continue;
        }

        // Skip dotfiles unless explicitly included
        if !options.include_dotfiles && is_dotfile(&path_str) {
            if options.verbose {
                ignored_files.push(path_str);
            }
            continue;
        }

        // Get file options with modification time and permissions preserved
        let file_options = if let Ok(metadata) = std::fs::metadata(path) {
            let mut opts = zip_options;

            // Preserve modification time
            if let Ok(modified) = metadata.modified()
                && let Some(dt) = system_time_to_zip_datetime(modified)
            {
                opts = opts.last_modified_time(dt);
            }

            // Preserve Unix permissions
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
            // Add directory entry
            let dir_path = format!("{}/", path_str);
            zip.add_directory(&dir_path, file_options)?;
        } else {
            // Add file
            let mut file = File::open(path)?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;

            total_size += contents.len() as u64;
            file_count += 1;

            zip.start_file(&path_str, file_options)?;
            zip.write_all(&contents)?;
        }
    }

    zip.finish()?;

    let compressed_size = std::fs::metadata(&output_path)?.len();

    Ok(PackResult {
        output_path,
        file_count,
        total_size,
        compressed_size,
        ignored_files,
        extension: ext.to_string(),
    })
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

        // Skip dotfiles unless explicitly included
        if !options.include_dotfiles && is_dotfile(&path_str) {
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
/// This applies the same filtering as `pack_bundle` (.mcpbignore, dotfiles, etc.)
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

/// Check if a path is a dotfile (starts with .).
fn is_dotfile(path: &str) -> bool {
    path.starts_with('.') || path.split('/').any(|component| component.starts_with('.'))
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

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_dotfile() {
        assert!(is_dotfile(".git"));
        assert!(is_dotfile(".DS_Store"));
        assert!(is_dotfile("foo/.hidden"));
        assert!(!is_dotfile("manifest.json"));
        assert!(!is_dotfile("server/index.js"));
    }

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
