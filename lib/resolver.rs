//! Filesystem-based tool resolver.
//!
//! This module provides resolution of tools from the filesystem.
//!
//! By default, this resolver only searches the local filesystem. To enable
//! automatic fetching from remote registries when plugins aren't found locally,
//! use [`with_auto_install`](FilePluginResolver::with_auto_install).

use crate::constants::{DEFAULT_TOOLS_PATH, MCPB_MANIFEST_FILE};
use crate::error::{ToolError, ToolResult};
use crate::mcpb::McpbManifest;
use crate::references::PluginRef;
use crate::registry::RegistryClient;
use semver::{Version, VersionReq};
use std::path::{Path, PathBuf};
use std::sync::Arc;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A resolved plugin with its template, path, and reference.
#[derive(Debug, Clone)]
pub struct ResolvedPlugin<T> {
    /// The parsed template/manifest.
    pub template: T,

    /// The filesystem path where the plugin was found.
    pub path: PathBuf,

    /// The canonical plugin reference.
    pub plugin_ref: PluginRef,
}

/// Pure filesystem resolver for tools.
#[derive(Debug, Clone)]
pub struct FilePluginResolver {
    /// Search paths for plugin resolution, checked in order.
    search_paths: Vec<PathBuf>,

    /// Optional registry client for auto-installing plugins.
    /// When set, namespaced plugins not found locally will be fetched from the registry.
    auto_install: Option<Arc<RegistryClient>>,
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl Default for FilePluginResolver {
    fn default() -> Self {
        Self {
            search_paths: vec![DEFAULT_TOOLS_PATH.clone()],
            auto_install: None,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl FilePluginResolver {
    /// Create a new filesystem resolver with the given search paths.
    pub fn new(paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        Self {
            search_paths: paths.into_iter().map(|p| p.into()).collect(),
            auto_install: None,
        }
    }

    /// Add a search path to the end (lowest priority).
    pub fn with_search_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.search_paths.push(path.into());
        self
    }

    /// Enable auto-installation of tools from remote registries.
    ///
    /// When enabled, namespaced tools (e.g., `appcypher/filesystem`) that aren't found
    /// locally will be automatically fetched from the registry and installed.
    pub fn with_auto_install(mut self, client: RegistryClient) -> Self {
        self.auto_install = Some(Arc::new(client));
        self
    }

    /// Check if auto-install is enabled.
    pub fn has_auto_install(&self) -> bool {
        self.auto_install.is_some()
    }

    /// Get the search paths.
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Resolve a tool by reference.
    pub async fn resolve_tool(
        &self,
        reference: &str,
    ) -> ToolResult<Option<ResolvedPlugin<McpbManifest>>> {
        let plugin_ref = PluginRef::parse(reference)?;
        self.resolve_tool_internal(&plugin_ref).await
    }

    /// Internal tool resolution.
    async fn resolve_tool_internal(
        &self,
        plugin_ref: &PluginRef,
    ) -> ToolResult<Option<ResolvedPlugin<McpbManifest>>> {
        let name = plugin_ref.name();
        let namespace = plugin_ref.namespace();
        let version_req = plugin_ref.version();

        // Build search locations
        for search_path in &self.search_paths {
            // Check direct path: search_path/name/manifest.json
            let tool_dir = if let Some(ns) = namespace {
                search_path.join(ns).join(name)
            } else {
                search_path.join(name)
            };

            // Try unversioned first
            if version_req.is_none() {
                let manifest_path = tool_dir.join(MCPB_MANIFEST_FILE);
                if manifest_path.exists() {
                    let manifest = McpbManifest::load(&tool_dir)?;
                    return Ok(Some(ResolvedPlugin {
                        path: manifest_path,
                        template: manifest,
                        plugin_ref: plugin_ref.clone(),
                    }));
                }
            }

            // Try versioned: search_path/name@version/manifest.json
            if let Some(resolved) = self.find_versioned_tool(
                tool_dir.parent().unwrap_or(&tool_dir),
                name,
                namespace,
                version_req,
            )? {
                return Ok(Some(resolved));
            }
        }

        // Check for namespaced matches if unnamespaced query
        if namespace.is_none() {
            let matches = self.find_namespaced_tools(name, version_req).await?;
            if matches.len() > 1 {
                let candidates = matches
                    .iter()
                    .take(5)
                    .map(|r| format!("  - {}", r))
                    .collect::<Vec<_>>()
                    .join("\n");
                return Err(ToolError::AmbiguousReference {
                    requested: name.to_string(),
                    candidates,
                    suggestion: format!(
                        "Did you mean one of: {}?",
                        matches
                            .iter()
                            .take(3)
                            .map(|r| r.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
            }
            if let Some(ns_ref) = matches.into_iter().next() {
                return Box::pin(self.resolve_tool_internal(&ns_ref)).await;
            }
        }

        // Auto-install: fetch from registry if enabled and has namespace
        if let Some(ref client) = self.auto_install
            && let Some(ns) = namespace
            && let Some((bundle_content, version)) = client.fetch_tool(plugin_ref).await?
        {
            // Install the fetched tool
            self.install_fetched_tool(ns, name, &bundle_content, &version)
                .await?;

            // Retry local resolution (without auto-install to avoid infinite loop)
            let local_resolver = FilePluginResolver {
                search_paths: self.search_paths.clone(),
                auto_install: None,
            };
            return Box::pin(local_resolver.resolve_tool_internal(plugin_ref)).await;
        }

        Ok(None)
    }

    /// Find versioned tool matching a version requirement.
    fn find_versioned_tool(
        &self,
        base_dir: &Path,
        name: &str,
        namespace: Option<&str>,
        version_req: Option<&VersionReq>,
    ) -> ToolResult<Option<ResolvedPlugin<McpbManifest>>> {
        if !base_dir.exists() {
            return Ok(None);
        }

        let mut candidates: Vec<(PathBuf, Version)> = Vec::new();

        // Look for name@version directories
        if let Ok(entries) = std::fs::read_dir(base_dir) {
            for entry in entries.flatten() {
                let dir_name = entry.file_name().to_string_lossy().to_string();

                // Check if this is a versioned directory for our tool
                if let Some(at_pos) = dir_name.find('@') {
                    let entry_name = &dir_name[..at_pos];
                    let version_str = &dir_name[at_pos + 1..];

                    if entry_name == name
                        && let Ok(version) = Version::parse(version_str)
                    {
                        // Check version requirement
                        if let Some(req) = version_req {
                            if req.matches(&version) {
                                candidates.push((entry.path(), version));
                            }
                        } else {
                            candidates.push((entry.path(), version));
                        }
                    }
                }
            }
        }

        // Return the latest matching version
        if let Some((path, version)) = candidates.into_iter().max_by(|a, b| a.1.cmp(&b.1)) {
            let manifest_path = path.join(MCPB_MANIFEST_FILE);
            if manifest_path.exists() {
                let manifest = McpbManifest::load(&path)?;
                let mut plugin_ref = PluginRef::new(name)?;
                if let Some(ns) = namespace {
                    plugin_ref = plugin_ref.with_namespace(ns)?;
                }
                plugin_ref =
                    plugin_ref.with_version(VersionReq::parse(&version.to_string()).unwrap());
                return Ok(Some(ResolvedPlugin {
                    path: manifest_path,
                    template: manifest,
                    plugin_ref,
                }));
            }
        }

        Ok(None)
    }

    /// Find namespaced tools matching a name.
    async fn find_namespaced_tools(
        &self,
        name: &str,
        _version_req: Option<&VersionReq>,
    ) -> ToolResult<Vec<PluginRef>> {
        let mut matches = Vec::new();

        for search_path in &self.search_paths {
            if !search_path.exists() {
                continue;
            }

            // Look for namespace directories
            if let Ok(entries) = std::fs::read_dir(search_path) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let namespace = entry.file_name().to_string_lossy().to_string();

                        // Check if this namespace has our tool
                        let tool_dir = entry.path().join(name);
                        let manifest_path = tool_dir.join(MCPB_MANIFEST_FILE);

                        if manifest_path.exists()
                            && let Ok(plugin_ref) =
                                PluginRef::new(name).and_then(|r| r.with_namespace(&namespace))
                        {
                            matches.push(plugin_ref);
                        }
                    }
                }
            }
        }

        Ok(matches)
    }

    /// List all installed tools.
    pub async fn list_tools(&self) -> ToolResult<Vec<PluginRef>> {
        let mut tools = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for search_path in &self.search_paths {
            if !search_path.exists() {
                continue;
            }

            self.collect_tools_recursive(search_path, None, &mut tools, &mut seen)?;
        }

        Ok(tools)
    }

    /// List orphaned entries (broken symlinks, empty directories) in the tools directory.
    ///
    /// Returns paths to entries that exist in the filesystem but don't contain valid tools.
    pub fn list_orphaned_entries(&self) -> ToolResult<Vec<PathBuf>> {
        let mut orphans = Vec::new();

        for search_path in &self.search_paths {
            if !search_path.exists() {
                continue;
            }

            self.collect_orphans_recursive(search_path, &mut orphans)?;
        }

        Ok(orphans)
    }

    /// Recursively collect orphaned entries.
    fn collect_orphans_recursive(&self, dir: &Path, orphans: &mut Vec<PathBuf>) -> ToolResult<()> {
        if !dir.exists() {
            return Ok(());
        }

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                let entry_name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files
                if entry_name.starts_with('.') {
                    continue;
                }

                // Check for broken symlinks first
                if entry_path.is_symlink() && !entry_path.exists() {
                    // Broken symlink - target doesn't exist
                    orphans.push(entry_path);
                    continue;
                }

                if entry_path.is_dir() {
                    let manifest_path = entry_path.join(MCPB_MANIFEST_FILE);
                    if manifest_path.exists() {
                        // Valid tool directory, not an orphan
                        continue;
                    }

                    // Check if this is an empty directory or empty namespace
                    let is_empty = std::fs::read_dir(&entry_path)
                        .map(|mut entries| entries.next().is_none())
                        .unwrap_or(false);

                    if is_empty {
                        // Empty directory
                        orphans.push(entry_path);
                    } else {
                        // Non-empty directory without manifest - might be a namespace
                        // Recurse to find orphans inside, then check if it becomes empty
                        let orphans_before = orphans.len();
                        self.collect_orphans_recursive(&entry_path, orphans)?;

                        // Check if any valid tools exist in this namespace
                        let has_valid_tools = self.namespace_has_valid_tools(&entry_path);
                        if !has_valid_tools {
                            // No valid tools in this namespace, mark the whole dir as orphan
                            // Remove any child orphans we just added (we'll remove the parent instead)
                            orphans.truncate(orphans_before);
                            orphans.push(entry_path);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a namespace directory contains any valid tools.
    fn namespace_has_valid_tools(&self, namespace_dir: &Path) -> bool {
        if let Ok(entries) = std::fs::read_dir(namespace_dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();

                // Skip broken symlinks
                if entry_path.is_symlink() && !entry_path.exists() {
                    continue;
                }

                if entry_path.is_dir() {
                    let manifest_path = entry_path.join(MCPB_MANIFEST_FILE);
                    if manifest_path.exists() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Recursively collect tools from a directory.
    #[allow(clippy::only_used_in_recursion)]
    fn collect_tools_recursive(
        &self,
        dir: &Path,
        namespace: Option<&str>,
        tools: &mut Vec<PluginRef>,
        seen: &mut std::collections::HashSet<String>,
    ) -> ToolResult<()> {
        if !dir.exists() {
            return Ok(());
        }

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                let entry_name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden files
                if entry_name.starts_with('.') {
                    continue;
                }

                if entry_path.is_dir() {
                    // Check if this is a tool directory (has manifest.json)
                    let manifest_path = entry_path.join(MCPB_MANIFEST_FILE);
                    if manifest_path.exists() {
                        // Extract name (remove version suffix if present)
                        let name = if let Some(at_pos) = entry_name.find('@') {
                            &entry_name[..at_pos]
                        } else {
                            &entry_name
                        };

                        let key = if let Some(ns) = namespace {
                            format!("{}/{}", ns, name)
                        } else {
                            name.to_string()
                        };

                        if !seen.contains(&key) {
                            seen.insert(key);

                            let plugin_ref = if let Some(ns) = namespace {
                                PluginRef::new(name).and_then(|r| r.with_namespace(ns))
                            } else {
                                PluginRef::new(name)
                            };

                            if let Ok(pr) = plugin_ref {
                                tools.push(pr);
                            }
                        }
                    } else if namespace.is_none() {
                        // This might be a namespace directory - recurse
                        self.collect_tools_recursive(&entry_path, Some(&entry_name), tools, seen)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Install a fetched tool bundle to the filesystem.
    async fn install_fetched_tool(
        &self,
        namespace: &str,
        name: &str,
        bundle_content: &[u8],
        version: &str,
    ) -> ToolResult<()> {
        // Use the first search path as the install location (typically ~/.tool/tools)
        let install_base = self.search_paths.first().ok_or_else(|| {
            ToolError::Generic("No search paths configured for installation".into())
        })?;

        // Build the target directory: ~/.tool/tools/<namespace>/<name>@<version>/
        let target_dir = install_base
            .join(namespace)
            .join(format!("{}@{}", name, version));

        // Create the target directory
        tokio::fs::create_dir_all(&target_dir).await.map_err(|e| {
            ToolError::Generic(format!(
                "Failed to create tool directory {:?}: {}",
                target_dir, e
            ))
        })?;

        // Extract the ZIP bundle
        self.extract_bundle(bundle_content, &target_dir)?;

        Ok(())
    }

    /// Extract a ZIP bundle to a directory.
    fn extract_bundle(&self, content: &[u8], target_dir: &Path) -> ToolResult<()> {
        use std::io::Read;
        use zip::ZipArchive;

        let cursor = std::io::Cursor::new(content);
        let mut archive = ZipArchive::new(cursor)
            .map_err(|e| ToolError::Generic(format!("Failed to read ZIP archive: {}", e)))?;

        // Extract entries
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| ToolError::Generic(format!("Failed to read archive entry: {}", e)))?;

            let entry_path = entry
                .enclosed_name()
                .ok_or_else(|| ToolError::Generic("Invalid entry path in archive".into()))?;

            let dest_path = target_dir.join(entry_path);

            // Create parent directories if needed
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolError::Generic(format!("Failed to create directory {:?}: {}", parent, e))
                })?;
            }

            // Get Unix permissions from ZIP entry (if available)
            #[cfg(unix)]
            let unix_mode = entry.unix_mode();

            // Extract file or directory
            if entry.is_dir() {
                std::fs::create_dir_all(&dest_path).map_err(|e| {
                    ToolError::Generic(format!("Failed to create directory {:?}: {}", dest_path, e))
                })?;
            } else {
                let mut file_content = Vec::new();
                entry.read_to_end(&mut file_content).map_err(|e| {
                    ToolError::Generic(format!("Failed to read entry content: {}", e))
                })?;

                std::fs::write(&dest_path, &file_content).map_err(|e| {
                    ToolError::Generic(format!("Failed to write file {:?}: {}", dest_path, e))
                })?;

                // Restore Unix permissions if available
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
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Load a tool manifest from a directory path.
pub fn load_tool_from_path(path: &Path) -> ToolResult<ResolvedPlugin<McpbManifest>> {
    let abs_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| ToolError::Generic(format!("Failed to get current directory: {}", e)))?
            .join(path)
    };

    // Create a synthetic plugin ref from the directory name
    let dir_name = abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Sanitize the name
    let sanitized_name: String = dir_name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let plugin_ref = PluginRef::parse(&sanitized_name)
        .unwrap_or_else(|_| PluginRef::parse("local-tool").expect("static ref should be valid"));

    // Look for manifest.json
    let manifest_path = abs_path.join(MCPB_MANIFEST_FILE);
    if manifest_path.exists() {
        let manifest = McpbManifest::load(&abs_path)?;
        return Ok(ResolvedPlugin {
            path: manifest_path,
            template: manifest,
            plugin_ref,
        });
    }

    Err(ToolError::Generic(format!(
        "No {} found in {}",
        MCPB_MANIFEST_FILE,
        abs_path.display()
    )))
}
