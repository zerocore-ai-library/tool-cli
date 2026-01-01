//! Registry authentication command handlers.

use crate::constants::{REGISTRY_AUTH_DIR, REGISTRY_TOKEN_ENV, get_registry_url};
use crate::error::ToolResult;
use crate::registry::RegistryClient;
use colored::Colorize;
use console::Term;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::fs;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Stored registry credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryCredentials {
    /// The API token.
    pub token: String,

    /// The username associated with the token.
    pub username: String,

    /// The registry URL this token is for.
    pub registry_url: String,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Get the path to the registry credentials file.
fn get_credentials_path() -> PathBuf {
    REGISTRY_AUTH_DIR.join("credentials.json")
}

/// Load stored registry credentials.
pub async fn load_credentials() -> ToolResult<Option<RegistryCredentials>> {
    let path = get_credentials_path();

    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path).await?;
    let creds: RegistryCredentials = serde_json::from_str(&content)?;
    Ok(Some(creds))
}

/// Save registry credentials.
pub async fn save_credentials(creds: &RegistryCredentials) -> ToolResult<()> {
    let path = get_credentials_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let content = serde_json::to_string_pretty(creds)?;
    fs::write(&path, content).await?;

    Ok(())
}

/// Delete stored registry credentials.
pub async fn delete_credentials() -> ToolResult<()> {
    let path = get_credentials_path();

    if path.exists() {
        fs::remove_file(&path).await?;
    }

    // Try to remove the directory if empty
    if let Some(dir) = path.parent()
        && dir.exists()
    {
        let _ = fs::remove_dir(dir).await; // Ignore error if not empty
    }

    Ok(())
}

/// Get the current registry token (from env or stored credentials).
pub async fn get_registry_token() -> ToolResult<Option<String>> {
    // Priority: env var > stored credential
    if let Ok(token) = std::env::var(REGISTRY_TOKEN_ENV) {
        return Ok(Some(token));
    }

    if let Some(creds) = load_credentials().await? {
        return Ok(Some(creds.token));
    }

    Ok(None)
}

/// Login to the registry.
///
/// If `token` is provided, uses it directly. Otherwise prompts for interactive input.
pub async fn auth_login(token: Option<&str>) -> ToolResult<()> {
    let registry_url = get_registry_url();

    // Get token
    let token = if let Some(t) = token {
        t.to_string()
    } else {
        // Interactive mode: show instructions and prompt
        println!();
        println!(
            "  To authenticate with {}, create an API token:",
            registry_url.bright_blue()
        );
        println!();
        println!(
            "  1. Open: {}",
            format!("{}/dashboard?tab=tokens", registry_url)
                .bright_cyan()
                .underline()
        );
        println!(
            "  2. Click {} with read + write permissions",
            "\"Create Token\"".bright_white()
        );
        println!("  3. Copy the token");
        println!();
        print!("  Paste your API token: ");
        io::stdout().flush()?;

        // Read token with hidden input
        let term = Term::stderr();
        let token = term.read_secure_line().map_err(|e| {
            crate::error::ToolError::Generic(format!("Failed to read token: {}", e))
        })?;
        token.trim().to_string()
    };

    if token.is_empty() {
        println!("  {} No token provided", "✗".bright_red());
        return Ok(());
    }

    // Validate the token
    println!("\n  {} Validating token...", "→".bright_blue());

    let client = RegistryClient::new()
        .with_url(&registry_url)
        .with_auth_token(&token);

    match client.validate_token().await {
        Ok(user_info) => {
            // Save credentials
            let creds = RegistryCredentials {
                token,
                username: user_info.username.clone(),
                registry_url: registry_url.clone(),
            };

            save_credentials(&creds).await?;

            println!(
                "  {} Authenticated as {}",
                "✓".bright_green(),
                format!("@{}", user_info.username).bright_cyan()
            );
            println!(
                "    Token stored in {}",
                get_credentials_path().display().to_string().dimmed()
            );
        }
        Err(e) => {
            println!(
                "  {} Authentication failed: {}",
                "✗".bright_red(),
                e.to_string().dimmed()
            );
        }
    }

    Ok(())
}

/// Logout from the registry.
pub async fn auth_logout() -> ToolResult<()> {
    if let Some(creds) = load_credentials().await? {
        delete_credentials().await?;
        println!(
            "  {} Logged out from {} (was @{})",
            "✓".bright_green(),
            creds.registry_url.bright_blue(),
            creds.username.bright_cyan()
        );
    } else {
        println!("  {} Not logged in", "✗".bright_yellow());
    }

    Ok(())
}

/// Show current authentication status.
pub async fn auth_status() -> ToolResult<()> {
    let registry_url = get_registry_url();

    // Check environment variable first
    if let Ok(token) = std::env::var(REGISTRY_TOKEN_ENV) {
        // Try to validate the token
        let client = RegistryClient::new()
            .with_url(&registry_url)
            .with_auth_token(&token);

        match client.validate_token().await {
            Ok(user_info) => {
                println!(
                    "  {} Authenticated via environment variable",
                    "✓".bright_green()
                );
                println!(
                    "    {}: @{}",
                    "User".dimmed(),
                    user_info.username.bright_cyan()
                );
                println!(
                    "    {}: {}",
                    "Registry".dimmed(),
                    registry_url.bright_blue()
                );
                println!(
                    "    {}: {}...{}",
                    "Token".dimmed(),
                    &token[..15.min(token.len())],
                    &token[token.len().saturating_sub(4)..]
                );
            }
            Err(_) => {
                println!("  {} Environment token is invalid", "✗".bright_red());
                println!("    {}: {}", "Variable".dimmed(), REGISTRY_TOKEN_ENV);
            }
        }
        return Ok(());
    }

    // Check stored credentials
    if let Some(creds) = load_credentials().await? {
        // Validate stored token
        let client = RegistryClient::new()
            .with_url(&creds.registry_url)
            .with_auth_token(&creds.token);

        match client.validate_token().await {
            Ok(_) => {
                println!("  {} Authenticated", "✓".bright_green());
                println!("    {}: @{}", "User".dimmed(), creds.username.bright_cyan());
                println!(
                    "    {}: {}",
                    "Registry".dimmed(),
                    creds.registry_url.bright_blue()
                );
                let token = &creds.token;
                println!(
                    "    {}: {}...{}",
                    "Token".dimmed(),
                    &token[..15.min(token.len())],
                    &token[token.len().saturating_sub(4)..]
                );
            }
            Err(_) => {
                println!(
                    "  {} Stored token is invalid or expired",
                    "✗".bright_yellow()
                );
                println!("    Run {} to re-authenticate", "tool login".bright_cyan());
            }
        }
    } else {
        println!("  {} Not authenticated", "✗".bright_yellow());
        println!();
        println!(
            "    Run {} to authenticate with {}",
            "tool login".bright_cyan(),
            registry_url.bright_blue()
        );
        println!(
            "    Or set {} environment variable",
            REGISTRY_TOKEN_ENV.bright_white()
        );
    }

    Ok(())
}
