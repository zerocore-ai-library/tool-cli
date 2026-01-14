//! Platform detection and override resolution.

use super::TOOL_STORE_NAMESPACE;
use super::types::{McpbMcpConfig, McpbPlatform, McpbPlatformOverride};

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Detect the current platform.
pub fn detect_platform() -> McpbPlatform {
    if cfg!(target_os = "windows") {
        McpbPlatform::Win32
    } else if cfg!(target_os = "macos") {
        McpbPlatform::Darwin
    } else {
        McpbPlatform::Linux
    }
}

/// Get the current platform as "{os}-{arch}" (e.g., "darwin-arm64", "linux-x86_64").
pub fn get_current_platform() -> String {
    format!("{}-{}", get_current_os(), get_current_arch())
}

/// Get the current OS in MCPB format (darwin, linux, win32).
pub fn get_current_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        os => os, // linux, etc. pass through
    }
}

/// Get the current architecture in our format (arm64, x86_64).
pub fn get_current_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        arch => arch, // x86_64 passes through
    }
}

/// Resolve platform-specific overrides for mcp_config.
///
/// Resolution order:
/// 1. `_meta.store.tool.mcpb.mcp_config.platform_overrides["{os}-{arch}"]` (exact match)
/// 2. `server.mcp_config.platform_overrides["{os}"]` (os-only fallback)
/// 3. Base `mcp_config` (no override)
///
/// Returns a new McpbMcpConfig with overrides applied.
pub fn resolve_platform_overrides(
    base_config: &McpbMcpConfig,
    meta: Option<&serde_json::Value>,
) -> McpbMcpConfig {
    let os = get_current_os();
    let platform = get_current_platform();

    // Try tool.store namespace first (os-arch specific)
    if let Some(override_config) = meta
        .and_then(|m| m.get(TOOL_STORE_NAMESPACE))
        .and_then(|r| r.get("mcp_config"))
        .and_then(|c| c.get("platform_overrides"))
        .and_then(|p| p.get(&platform))
        && let Ok(platform_override) =
            serde_json::from_value::<McpbPlatformOverride>(override_config.clone())
    {
        return apply_platform_override(base_config, &platform_override);
    }

    // Fall back to spec-level os-only overrides
    if let Some(platform_override) = base_config.platform_overrides.get(os) {
        return apply_platform_override(base_config, platform_override);
    }

    // No override found, return base config
    base_config.clone()
}

/// Apply a platform override to a base mcp_config.
fn apply_platform_override(
    base: &McpbMcpConfig,
    override_config: &McpbPlatformOverride,
) -> McpbMcpConfig {
    let mut result = base.clone();

    // Override command if specified
    if let Some(ref command) = override_config.command {
        result.command = Some(command.clone());
    }

    // Override args if specified (replace, not merge)
    if let Some(ref args) = override_config.args {
        result.args = args.clone();
    }

    // Merge env (override values take precedence)
    if let Some(ref env) = override_config.env {
        for (k, v) in env {
            result.env.insert(k.clone(), v.clone());
        }
    }

    // Override url if specified
    if let Some(ref url) = override_config.url {
        result.url = Some(url.clone());
    }

    // Merge headers (override values take precedence)
    if let Some(ref headers) = override_config.headers {
        for (k, v) in headers {
            result.headers.insert(k.clone(), v.clone());
        }
    }

    result
}
