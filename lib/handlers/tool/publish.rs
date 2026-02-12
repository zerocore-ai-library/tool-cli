//! Registry publish command handlers.

use super::pack_cmd::format_size;
use crate::constants::MCPB_MANIFEST_FILE;
use crate::error::{ToolError, ToolResult};
use crate::mcpb::McpbManifest;
use crate::pack::{PackError, PackOptions, compute_sha256, pack_bundle};
use crate::registry::RegistryClient;
use crate::styles::Spinner;
use colored::Colorize;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

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

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Publish a tool to the registry.
///
/// If `token` is provided, uses it directly instead of stored credentials.
pub async fn publish_mcpb(
    path: &str,
    dry_run: bool,
    strict: bool,
    multi_platform: bool,
    prebuilt_artifacts: HashMap<String, PathBuf>,
    token: Option<&str>,
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
    // Priority: explicit token > env var > stored credentials
    let (namespace, resolved_token) = if dry_run {
        let creds = load_credentials().await?.map(|c| (c.username, c.token));
        match creds {
            Some((username, token)) => (username, Some(token)),
            None => ("<your-username>".to_string(), None),
        }
    } else {
        let resolved_token = if let Some(t) = token {
            t.to_string()
        } else {
            get_registry_token().await?.ok_or_else(|| {
                ToolError::Generic("Authentication required. Run `tool login` first.".into())
            })?
        };
        let client = RegistryClient::new().with_auth_token(&resolved_token);
        let user = client.validate_token().await?;
        (user.username, Some(resolved_token))
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
                resolved_token,
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
    let resolved_token = resolved_token.unwrap();
    let client = RegistryClient::new().with_auth_token(&resolved_token);

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
