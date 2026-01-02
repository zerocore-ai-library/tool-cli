//! Rust project detector.

use super::utils::{has_any_pattern, read_toml};
use super::{
    DetectError, DetectOptions, DetectionDetails, DetectionResult, GeneratedScaffold,
    ProjectDetector,
};
use crate::mcpb::{
    McpbCompatibility, McpbManifest, McpbMcpConfig, McpbPlatform, McpbServer, McpbServerType,
    McpbTransport, detect_platform,
};
use crate::scaffold::rust_mcpbignore_template;
use std::collections::BTreeMap;
use std::path::Path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Detector for Rust MCP server projects.
pub struct RustDetector;

/// Parsed Cargo.toml structure.
#[derive(Debug, serde::Deserialize)]
struct CargoToml {
    package: Option<CargoPackage>,
    #[serde(default)]
    dependencies: toml::Table,
    bin: Option<Vec<CargoBin>>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoPackage {
    name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct CargoBin {
    name: Option<String>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl RustDetector {
    /// Create a new Rust detector.
    pub fn new() -> Self {
        Self
    }

    /// Get the binary name from Cargo.toml.
    fn get_binary_name(&self, cargo: &CargoToml) -> Option<String> {
        // Check [[bin]] section first
        if let Some(bins) = &cargo.bin
            && let Some(first) = bins.first()
            && let Some(name) = &first.name
        {
            return Some(name.clone());
        }

        // Fall back to package name
        cargo.package.as_ref().and_then(|p| p.name.clone())
    }

    /// Get the entry point path (binary location).
    fn get_entry_point(&self, name: &str, platform: &McpbPlatform) -> String {
        match platform {
            McpbPlatform::Win32 => format!("target/release/{}.exe", name),
            _ => format!("target/release/{}", name),
        }
    }

    /// Get the command path for mcp_config.
    fn get_command_path(&self, name: &str, platform: &McpbPlatform) -> String {
        match platform {
            McpbPlatform::Win32 => format!("${{__dirname}}\\target\\release\\{}.exe", name),
            _ => format!("${{__dirname}}/target/release/{}", name),
        }
    }

    /// Check if project has rmcp dependency.
    fn has_rmcp(&self, cargo: &CargoToml) -> bool {
        cargo.dependencies.contains_key("rmcp")
    }

    /// Detect transport by grepping source files.
    fn detect_transport(&self, dir: &Path) -> McpbTransport {
        let http_patterns = [
            r"transport::streamable_http_server",
            r"StreamableHttpService",
            r"axum::Router",
            r"TcpListener::bind",
        ];

        if has_any_pattern(dir, &http_patterns, &["rs"]).is_some() {
            McpbTransport::Http
        } else {
            McpbTransport::Stdio
        }
    }

    /// Check if the binary is already built.
    fn is_built(&self, dir: &Path, name: &str) -> bool {
        let release_path = dir.join(format!("target/release/{}", name));
        let debug_path = dir.join(format!("target/debug/{}", name));

        release_path.exists() || debug_path.exists()
    }
}

impl Default for RustDetector {
    fn default() -> Self {
        Self::new()
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl ProjectDetector for RustDetector {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn display_name(&self) -> &'static str {
        "Rust"
    }

    fn server_type(&self) -> McpbServerType {
        McpbServerType::Binary
    }

    fn detect(&self, dir: &Path) -> Option<DetectionResult> {
        let cargo_path = dir.join("Cargo.toml");
        if !cargo_path.exists() {
            return None;
        }

        let cargo: CargoToml = read_toml(&cargo_path)?;

        // Check for rmcp dependency
        if !self.has_rmcp(&cargo) {
            return None;
        }

        let binary_name = self.get_binary_name(&cargo)?;
        let platform = detect_platform();
        let transport = self.detect_transport(dir);
        let entry_point = self.get_entry_point(&binary_name, &platform);
        let is_built = self.is_built(dir, &binary_name);

        // Calculate confidence
        let mut confidence = 0.8; // High confidence for rmcp dependency
        if is_built {
            confidence += 0.15;
        }

        let mut notes = Vec::new();

        if !is_built {
            notes.push(format!(
                "Binary not found. Run `cargo build --release` to build '{}'.",
                binary_name
            ));
        }

        // Run args for mcp_config
        let command = self.get_command_path(&binary_name, &platform);

        Some(DetectionResult {
            confidence,
            server_type: McpbServerType::Binary,
            details: DetectionDetails {
                entry_point: Some(entry_point),
                script_name: None,
                package_manager: None, // Rust uses cargo, not a package manager in the MCPB sense
                transport: Some(transport),
                build_command: Some("cargo build --release".to_string()),
                run_command: Some(command),
                run_args: vec![],
                notes,
            },
        })
    }

    fn generate(
        &self,
        dir: &Path,
        detection: &DetectionResult,
        options: &DetectOptions,
    ) -> Result<GeneratedScaffold, DetectError> {
        let cargo: CargoToml = read_toml(&dir.join("Cargo.toml"))
            .ok_or_else(|| DetectError::IoError("Failed to read Cargo.toml".into()))?;

        let binary_name = self
            .get_binary_name(&cargo)
            .ok_or_else(|| DetectError::IoError("Could not determine binary name".into()))?;

        let platform = detect_platform();

        // Use options to override detected values
        let entry_point = options
            .entry_point
            .clone()
            .or(detection.details.entry_point.clone())
            .unwrap_or_else(|| self.get_entry_point(&binary_name, &platform));

        let transport = options
            .transport
            .or(detection.details.transport)
            .unwrap_or(McpbTransport::Stdio);

        // Get package name
        let name = options.name.clone().unwrap_or(binary_name.clone());

        // Build command path
        let command = self.get_command_path(&binary_name, &platform);

        // Build mcp_config
        let mcp_config = match transport {
            McpbTransport::Stdio => McpbMcpConfig {
                command: Some(command),
                args: vec![],
                env: BTreeMap::new(),
                url: None,
                headers: BTreeMap::new(),
                oauth_config: None,
            },
            McpbTransport::Http => McpbMcpConfig {
                command: Some(command),
                args: vec![
                    "--port=${system_config.port}".to_string(),
                    "--host=${system_config.hostname}".to_string(),
                ],
                env: BTreeMap::new(),
                url: Some("http://${system_config.hostname}:${system_config.port}/mcp".to_string()),
                headers: BTreeMap::new(),
                oauth_config: None,
            },
        };

        // Build manifest
        let manifest = McpbManifest {
            manifest_version: "0.3".to_string(),
            name: Some(name),
            version: Some("0.1.0".to_string()),
            description: Some("An MCP server".to_string()),
            author: None,
            server: McpbServer {
                server_type: Some(McpbServerType::Binary),
                transport,
                entry_point: Some(entry_point),
                mcp_config: Some(mcp_config),
            },
            display_name: None,
            long_description: None,
            license: None,
            icon: None,
            icons: None,
            homepage: None,
            documentation: None,
            support: None,
            repository: None,
            keywords: None,
            tools: None,
            prompts: None,
            tools_generated: None,
            prompts_generated: None,
            user_config: None,
            system_config: None,
            compatibility: Some(McpbCompatibility {
                claude_desktop: None,
                platforms: Some(vec![platform]),
                runtimes: None,
            }),
            privacy_policies: None,
            localization: None,
            meta: Some(serde_json::json!({
                "company.superrad.radical": {
                    "scripts": {
                        "build": "cargo build --release"
                    }
                }
            })),
            bundle_path: None,
        };

        // Determine files
        let manifest_path = dir.join("manifest.json");
        let mcpbignore_path = dir.join(".mcpbignore");

        let mut files_to_create = Vec::new();
        let mut files_to_overwrite = Vec::new();

        if manifest_path.exists() {
            files_to_overwrite.push(manifest_path);
        } else {
            files_to_create.push(dir.join("manifest.json"));
        }

        if mcpbignore_path.exists() {
            files_to_overwrite.push(mcpbignore_path);
        } else {
            files_to_create.push(dir.join(".mcpbignore"));
        }

        Ok(GeneratedScaffold {
            manifest,
            mcpbignore: rust_mcpbignore_template().to_string(),
            files_to_create,
            files_to_overwrite,
        })
    }
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_rust_project(tmp: &TempDir, has_rmcp: bool) {
        let deps = if has_rmcp {
            r#"rmcp = { version = "0.12", features = ["server"] }"#
        } else {
            r#"serde = "1.0""#
        };

        let cargo_toml = format!(
            r#"
[package]
name = "test-mcp-server"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
"#,
            deps
        );

        fs::write(tmp.path().join("Cargo.toml"), cargo_toml).unwrap();
    }

    #[test]
    fn test_detect_rust_project_with_rmcp() {
        let tmp = TempDir::new().unwrap();
        create_rust_project(&tmp, true);

        let detector = RustDetector::new();
        let result = detector.detect(tmp.path());

        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.confidence >= 0.8);
        assert_eq!(result.server_type, McpbServerType::Binary);
        assert!(
            result
                .details
                .entry_point
                .as_ref()
                .unwrap()
                .contains("test-mcp-server")
        );
    }

    #[test]
    fn test_detect_rust_project_without_rmcp() {
        let tmp = TempDir::new().unwrap();
        create_rust_project(&tmp, false);

        let detector = RustDetector::new();
        let result = detector.detect(tmp.path());

        assert!(result.is_none());
    }

    #[test]
    fn test_detect_binary_name_from_bin_section() {
        let tmp = TempDir::new().unwrap();

        let cargo_toml = r#"
[package]
name = "my-lib"
version = "0.1.0"

[[bin]]
name = "my-server"
path = "src/main.rs"

[dependencies]
rmcp = "0.12"
"#;
        fs::write(tmp.path().join("Cargo.toml"), cargo_toml).unwrap();

        let detector = RustDetector::new();
        let result = detector.detect(tmp.path());

        assert!(result.is_some());
        let result = result.unwrap();
        assert!(
            result
                .details
                .entry_point
                .as_ref()
                .unwrap()
                .contains("my-server")
        );
    }

    #[test]
    fn test_detect_transport_http() {
        let tmp = TempDir::new().unwrap();
        create_rust_project(&tmp, true);

        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("src/main.rs"),
            r#"
use rmcp::transport::streamable_http_server::StreamableHttpService;
use axum::Router;
"#,
        )
        .unwrap();

        let detector = RustDetector::new();
        assert_eq!(detector.detect_transport(tmp.path()), McpbTransport::Http);
    }

    #[test]
    fn test_detect_transport_stdio() {
        let tmp = TempDir::new().unwrap();
        create_rust_project(&tmp, true);

        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("src/main.rs"),
            r#"
use rmcp::transport::stdio;
let service = Server::new().serve(stdio()).await?;
"#,
        )
        .unwrap();

        let detector = RustDetector::new();
        assert_eq!(detector.detect_transport(tmp.path()), McpbTransport::Stdio);
    }

    #[test]
    fn test_is_built() {
        let tmp = TempDir::new().unwrap();
        create_rust_project(&tmp, true);

        let detector = RustDetector::new();

        // Not built yet
        assert!(!detector.is_built(tmp.path(), "test-mcp-server"));

        // Create fake binary
        fs::create_dir_all(tmp.path().join("target/release")).unwrap();
        fs::write(tmp.path().join("target/release/test-mcp-server"), "").unwrap();

        assert!(detector.is_built(tmp.path(), "test-mcp-server"));
    }
}
