//! Self-update and self-uninstall functionality for tool-cli.

use crate::error::{ToolError, ToolResult};
use crate::styles::Spinner;
use colored::Colorize;
use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use tar::Archive;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// GitHub repository for releases.
const GITHUB_REPO: &str = "zerocore-ai/tool-cli";

/// GitHub releases API URL.
const RELEASES_API_URL: &str = "https://api.github.com/repos/zerocore-ai/tool-cli/releases/latest";

/// Current version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// GitHub release information.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    /// Tag name (e.g., "v0.1.2").
    tag_name: String,
    /// Release assets.
    assets: Vec<GitHubAsset>,
}

/// GitHub release asset.
#[derive(Debug, Deserialize)]
struct GitHubAsset {
    /// Asset name.
    name: String,
    /// Download URL.
    browser_download_url: String,
    /// Size in bytes.
    size: u64,
}

/// Update check result.
#[derive(Debug)]
pub struct UpdateCheckResult {
    /// Current version.
    pub current: String,
    /// Latest version available.
    pub latest: String,
    /// Whether an update is available.
    pub update_available: bool,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Get the current executable path.
fn current_exe_path() -> ToolResult<PathBuf> {
    std::env::current_exe()
        .map_err(|e| ToolError::Generic(format!("Failed to get executable path: {}", e)))
}

/// Get the platform identifier for downloads.
fn get_platform() -> ToolResult<&'static str> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    match (os, arch) {
        ("macos", "aarch64") => Ok("darwin-aarch64"),
        ("macos", "x86_64") => Ok("darwin-x86_64"),
        ("linux", "aarch64") => Ok("linux-aarch64"),
        ("linux", "x86_64") => Ok("linux-x86_64"),
        _ => Err(ToolError::Generic(format!(
            "Unsupported platform: {}-{}",
            os, arch
        ))),
    }
}

/// Fetch the latest release information from GitHub.
async fn fetch_latest_release(client: &Client) -> ToolResult<GitHubRelease> {
    let response = client
        .get(RELEASES_API_URL)
        .header("User-Agent", format!("tool-cli/{}", VERSION))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to fetch release info: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        if status.as_u16() == 404 {
            return Err(ToolError::Generic(
                "No releases found. The repository may be private or has no published releases yet.".into()
            ));
        }
        return Err(ToolError::Generic(format!(
            "GitHub API error: HTTP {}",
            status
        )));
    }

    response
        .json::<GitHubRelease>()
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to parse release info: {}", e)))
}

/// Parse version from tag (removes 'v' prefix if present).
fn parse_version(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Compare two semantic versions. Returns true if `latest` is newer than `current`.
fn is_newer_version(current: &str, latest: &str) -> bool {
    let parse = |v: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() >= 3 {
            Some((
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].split('-').next()?.parse().ok()?,
            ))
        } else {
            None
        }
    };

    match (parse(current), parse(latest)) {
        (Some(curr), Some(lat)) => lat > curr,
        _ => false,
    }
}

/// Check for available updates.
pub async fn check_for_update() -> ToolResult<UpdateCheckResult> {
    let client = Client::new();
    let release = fetch_latest_release(&client).await?;
    let latest = parse_version(&release.tag_name).to_string();
    let current = VERSION.to_string();
    let update_available = is_newer_version(&current, &latest);

    Ok(UpdateCheckResult {
        current,
        latest,
        update_available,
    })
}

/// Download a file with progress bar.
async fn download_with_progress(client: &Client, url: &str, size: u64) -> ToolResult<Vec<u8>> {
    let response = client
        .get(url)
        .header("User-Agent", format!("tool-cli/{}", VERSION))
        .send()
        .await
        .map_err(|e| ToolError::Generic(format!("Download failed: {}", e)))?;

    if !response.status().is_success() {
        return Err(ToolError::Generic(format!(
            "Download failed: HTTP {}",
            response.status()
        )));
    }

    let pb = ProgressBar::new(size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  [{bar:40.cyan/dim}] {bytes}/{total_bytes} {bytes_per_sec}")
            .unwrap()
            .progress_chars("█░░"),
    );

    let bytes = response
        .bytes()
        .await
        .map_err(|e| ToolError::Generic(format!("Failed to read response: {}", e)))?;

    pb.set_position(bytes.len() as u64);
    pb.finish_and_clear();

    Ok(bytes.to_vec())
}

/// Download and verify checksum.
async fn download_checksum(
    client: &Client,
    version: &str,
    archive_name: &str,
) -> ToolResult<Option<String>> {
    let checksum_url = format!(
        "https://github.com/{}/releases/download/v{}/{}.sha256",
        GITHUB_REPO, version, archive_name
    );

    match client
        .get(&checksum_url)
        .header("User-Agent", format!("tool-cli/{}", VERSION))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            let text = response.text().await.unwrap_or_default();
            // Extract just the hash (first field, space-separated)
            Ok(Some(
                text.split_whitespace().next().unwrap_or("").to_string(),
            ))
        }
        _ => Ok(None),
    }
}

/// Verify SHA256 checksum.
fn verify_checksum(data: &[u8], expected: &str) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = format!("{:x}", hasher.finalize());
    actual == expected
}

/// Extract binary from tarball.
fn extract_binary(tarball: &[u8]) -> ToolResult<Vec<u8>> {
    let decoder = GzDecoder::new(tarball);
    let mut archive = Archive::new(decoder);

    for entry in archive
        .entries()
        .map_err(|e| ToolError::Generic(format!("Failed to read archive: {}", e)))?
    {
        let mut entry =
            entry.map_err(|e| ToolError::Generic(format!("Failed to read entry: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| ToolError::Generic(format!("Invalid path: {}", e)))?;

        // Look for the 'tool' binary
        if path.file_name().map(|n| n == "tool").unwrap_or(false) {
            let mut binary = Vec::new();
            entry
                .read_to_end(&mut binary)
                .map_err(|e| ToolError::Generic(format!("Failed to read binary: {}", e)))?;
            return Ok(binary);
        }
    }

    Err(ToolError::Generic(
        "Binary 'tool' not found in archive".into(),
    ))
}

/// Perform the self-update.
pub async fn self_update(target_version: Option<&str>) -> ToolResult<()> {
    if cfg!(windows) {
        return Err(ToolError::Generic(
            "Self-update is not supported on Windows yet. Reinstall with: cargo install --git https://github.com/zerocore-ai/tool-cli --locked".into(),
        ));
    }

    println!();
    let spinner = Spinner::with_indent("Checking for updates", 2);

    let client = Client::new();
    let release = match fetch_latest_release(&client).await {
        Ok(release) => {
            spinner.succeed(Some("Checked for updates"));
            release
        }
        Err(e) => {
            spinner.fail(None);
            return Err(e);
        }
    };
    let latest_version = parse_version(&release.tag_name);

    // Determine target version
    let version = target_version.unwrap_or(latest_version);

    // Check if update is needed
    if version == VERSION && target_version.is_none() {
        println!(
            "  {} Already up to date ({})",
            "✓".bright_green(),
            VERSION.bright_cyan()
        );
        println!();
        return Ok(());
    }

    if !is_newer_version(VERSION, version) && target_version.is_none() {
        println!(
            "  {} Already up to date ({})",
            "✓".bright_green(),
            VERSION.bright_cyan()
        );
        println!();
        return Ok(());
    }

    println!(
        "  {} Update available: {} → {}",
        "✓".bright_green(),
        VERSION.dimmed(),
        version.bright_cyan()
    );
    println!();

    // Get platform and find matching asset
    let platform = get_platform()?;
    let archive_name = format!("tool-{}-{}.tar.gz", version, platform);

    let asset = release
        .assets
        .iter()
        .find(|a| a.name == archive_name)
        .ok_or_else(|| {
            ToolError::Generic(format!(
                "No release found for platform '{}'. Available: {}",
                platform,
                release
                    .assets
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;

    // Download
    println!(
        "  {} Downloading {}",
        "→".bright_blue(),
        archive_name.bright_cyan()
    );
    let tarball = download_with_progress(&client, &asset.browser_download_url, asset.size).await?;
    println!("  {} Downloaded", "✓".bright_green());

    // Verify checksum if available
    if let Some(expected_hash) = download_checksum(&client, version, &archive_name).await? {
        let spinner = Spinner::with_indent("Verifying checksum", 2);
        if verify_checksum(&tarball, &expected_hash) {
            spinner.succeed(Some("Checksum verified"));
        } else {
            spinner.fail(Some("Checksum mismatch"));
            return Err(ToolError::Generic("Checksum verification failed".into()));
        }
    }

    // Extract binary
    let spinner = Spinner::with_indent("Extracting", 2);
    let binary = match extract_binary(&tarball) {
        Ok(binary) => {
            spinner.succeed(Some("Extracted"));
            binary
        }
        Err(e) => {
            spinner.fail(None);
            return Err(e);
        }
    };

    // Replace current executable
    let exe_path = current_exe_path()?;
    let temp_path = exe_path.with_extension("new");
    let backup_path = exe_path.with_extension("backup");

    let spinner = Spinner::with_indent("Installing", 2);

    // Installation closure to handle all steps
    let install_result: ToolResult<()> = (|| {
        // Write new binary to temp file
        {
            let mut file = File::create(&temp_path)
                .map_err(|e| ToolError::Generic(format!("Failed to create temp file: {}", e)))?;
            file.write_all(&binary)
                .map_err(|e| ToolError::Generic(format!("Failed to write binary: {}", e)))?;
        }

        // Set executable permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o755))
                .map_err(|e| ToolError::Generic(format!("Failed to set permissions: {}", e)))?;
        }

        // Backup current, move new into place
        if exe_path.exists() {
            fs::rename(&exe_path, &backup_path).map_err(|e| {
                ToolError::Generic(format!("Failed to backup current binary: {}", e))
            })?;
        }

        if let Err(e) = fs::rename(&temp_path, &exe_path) {
            // Restore backup on failure
            if backup_path.exists() {
                let _ = fs::rename(&backup_path, &exe_path);
            }
            return Err(ToolError::Generic(format!(
                "Failed to install new binary: {}",
                e
            )));
        }

        // Remove backup
        let _ = fs::remove_file(&backup_path);

        Ok(())
    })();

    match install_result {
        Ok(()) => {
            spinner.succeed(Some("Installed"));
        }
        Err(e) => {
            spinner.fail(None);
            return Err(e);
        }
    }
    println!();
    println!(
        "  {} Updated to version {}",
        "✓".bright_green().bold(),
        version.bright_cyan().bold()
    );
    println!();

    Ok(())
}

/// Uninstall tool-cli.
pub async fn self_uninstall(skip_confirm: bool) -> ToolResult<()> {
    println!();

    let exe_path = current_exe_path()?;

    println!(
        "  {} This will remove: {}",
        "✗".bright_yellow(),
        exe_path.display().to_string().bright_cyan()
    );
    println!();

    if !skip_confirm {
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
    }

    println!();

    // Remove the binary
    // On Unix, we can remove a running executable - it stays in memory until process exits
    fs::remove_file(&exe_path)
        .map_err(|e| ToolError::Generic(format!("Failed to remove binary: {}", e)))?;

    println!(
        "  {} Removed {}",
        "✓".bright_green(),
        exe_path.display().to_string().dimmed()
    );
    println!();
    println!(
        "  {} tool-cli has been uninstalled",
        "✓".bright_green().bold()
    );
    println!();

    Ok(())
}
