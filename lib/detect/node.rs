//! Node.js project detector.

use super::utils::{find_first_relative, has_any_pattern, read_json};
use super::{
    DetectError, DetectOptions, DetectionDetails, DetectionResult, GeneratedScaffold,
    ProjectDetector,
};
use crate::mcpb::{
    McpbManifest, McpbMcpConfig, McpbServer, McpbServerType, McpbTransport, NodePackageManager,
    PackageManager,
};
use crate::scaffold::mcpbignore_template;
use std::collections::BTreeMap;
use std::path::Path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Detector for Node.js MCP server projects.
pub struct NodeDetector;

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl NodeDetector {
    /// Create a new Node.js detector.
    pub fn new() -> Self {
        Self
    }

    /// Detect package manager from lock files.
    fn detect_package_manager(&self, dir: &Path) -> NodePackageManager {
        if dir.join("bun.lockb").exists() {
            NodePackageManager::Bun
        } else if dir.join("pnpm-lock.yaml").exists() {
            NodePackageManager::Pnpm
        } else if dir.join("yarn.lock").exists() {
            NodePackageManager::Yarn
        } else {
            NodePackageManager::Npm
        }
    }

    /// Detect entry point from package.json and file structure.
    /// Returns (entry_point, exists) - exists indicates if the file is present on disk.
    fn detect_entry_point(&self, dir: &Path, pkg: &serde_json::Value) -> (Option<String>, bool) {
        // 1. Check package.json.main
        if let Some(main) = pkg.get("main").and_then(|v| v.as_str())
            && dir.join(main).exists()
        {
            return (Some(main.to_string()), true);
        }

        // 2. Check package.json.bin (first, then check existence)
        let bin_entry = if let Some(bin) = pkg.get("bin") {
            match bin {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(obj) => obj
                    .values()
                    .next()
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            }
        } else {
            None
        };

        if let Some(ref entry) = bin_entry
            && dir.join(entry).exists()
        {
            return (Some(entry.clone()), true);
        }

        // 3. Check package.json.exports["."]
        if let Some(exports) = pkg.get("exports")
            && let Some(root_export) = exports.get(".")
        {
            let entry = match root_export {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Object(obj) => obj
                    .get("default")
                    .or_else(|| obj.get("import"))
                    .or_else(|| obj.get("require"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                _ => None,
            };
            if let Some(e) = entry
                && dir.join(&e).exists()
            {
                return (Some(e), true);
            }
        }

        // 4. Check TypeScript config for outDir
        if let Some(tsconfig) = read_json::<serde_json::Value>(&dir.join("tsconfig.json"))
            && let Some(out_dir) = tsconfig
                .get("compilerOptions")
                .and_then(|c| c.get("outDir"))
                .and_then(|v| v.as_str())
        {
            let out_dir = out_dir.trim_start_matches("./").trim_matches('/');
            let candidates = [
                format!("{}/index.js", out_dir),
                format!("{}/main.js", out_dir),
                format!("{}/server.js", out_dir),
            ];
            for candidate in candidates {
                if dir.join(&candidate).exists() {
                    return (Some(candidate), true);
                }
            }
        }

        // 5. Common patterns (existing files)
        let patterns = [
            "dist/index.js",
            "dist/main.js",
            "dist/server.js",
            "build/index.js",
            "build/main.js",
            "server/index.js",
            "src/index.js",
            "src/main.js",
            "index.js",
            "main.js",
            "server.js",
        ];

        if let Some(found) = find_first_relative(dir, &patterns) {
            return (Some(found), true);
        }

        // 6. Fallback: use bin entry even if file doesn't exist (for unbuilt TypeScript)
        if let Some(entry) = bin_entry {
            return (Some(entry), false);
        }

        // 7. Fallback: use main entry even if file doesn't exist
        if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
            return (Some(main.to_string()), false);
        }

        (None, false)
    }

    /// Detect transport by grepping source files.
    fn detect_transport(&self, dir: &Path) -> McpbTransport {
        let http_patterns = [
            r"StreamableHTTPServerTransport",
            r"streamableHttp",
            r"createServer\s*\(",
            r"\.listen\s*\(",
        ];

        if has_any_pattern(dir, &http_patterns, &["js", "ts", "mjs", "mts"]).is_some() {
            // Double-check it's not just importing but actually using HTTP
            let stdio_patterns = [r"StdioServerTransport"];
            if has_any_pattern(dir, &stdio_patterns, &["js", "ts", "mjs", "mts"]).is_some() {
                // Has both - check which is actually used for connection
                // Default to stdio if both present (safer assumption)
                return McpbTransport::Stdio;
            }
            McpbTransport::Http
        } else {
            McpbTransport::Stdio
        }
    }

    /// Check if project has MCP SDK dependency.
    fn has_mcp_sdk(&self, pkg: &serde_json::Value) -> bool {
        let has_dep = pkg
            .get("dependencies")
            .and_then(|d| d.get("@modelcontextprotocol/sdk"))
            .is_some();

        let has_dev_dep = pkg
            .get("devDependencies")
            .and_then(|d| d.get("@modelcontextprotocol/sdk"))
            .is_some();

        has_dep || has_dev_dep
    }

    /// Check if this is a TypeScript project.
    fn is_typescript(&self, dir: &Path) -> bool {
        dir.join("tsconfig.json").exists()
    }

    /// Detect build command from package.json scripts.
    /// Returns the full build command including install + custom build script if present.
    fn detect_build_command(&self, dir: &Path, pm: NodePackageManager) -> String {
        let install_cmd = pm.build_command();

        // Check for build script in package.json
        if let Some(pkg) = read_json::<serde_json::Value>(&dir.join("package.json"))
            && let Some(scripts) = pkg.get("scripts").and_then(|s| s.as_object())
        {
            // Check for common build script names
            let has_build_script = scripts
                .get("build")
                .or_else(|| scripts.get("compile"))
                .or_else(|| scripts.get("tsc"))
                .and_then(|v| v.as_str())
                .is_some();

            if has_build_script {
                let run_cmd = match pm {
                    NodePackageManager::Npm => "npm run build",
                    NodePackageManager::Pnpm => "pnpm run build",
                    NodePackageManager::Yarn => "yarn build",
                    NodePackageManager::Bun => "bun run build",
                };
                return format!("{} && {}", install_cmd, run_cmd);
            }
        }

        install_cmd.to_string()
    }
}

impl Default for NodeDetector {
    fn default() -> Self {
        Self::new()
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl ProjectDetector for NodeDetector {
    fn name(&self) -> &'static str {
        "node"
    }

    fn display_name(&self) -> &'static str {
        "Node.js"
    }

    fn server_type(&self) -> McpbServerType {
        McpbServerType::Node
    }

    fn detect(&self, dir: &Path) -> Option<DetectionResult> {
        let pkg_path = dir.join("package.json");
        if !pkg_path.exists() {
            return None;
        }

        let pkg: serde_json::Value = read_json(&pkg_path)?;

        // Check for MCP SDK dependency
        if !self.has_mcp_sdk(&pkg) {
            return None;
        }

        let package_manager = self.detect_package_manager(dir);
        let (entry_point, entry_exists) = self.detect_entry_point(dir, &pkg);
        let transport = self.detect_transport(dir);
        let is_typescript = self.is_typescript(dir);
        let build_command = self.detect_build_command(dir, package_manager);

        // Calculate confidence
        let mut confidence = 0.7; // Base confidence for having MCP SDK
        if entry_point.is_some() && entry_exists {
            confidence += 0.2;
        } else if entry_point.is_some() {
            confidence += 0.1; // Partial boost for inferred entry point
        }
        if is_typescript {
            confidence += 0.05; // Slight boost for TypeScript (more likely to be intentional)
        }

        let mut notes = Vec::new();

        if entry_point.is_none() {
            notes.push(
                "Could not auto-detect entry point. Specify --entry to set it manually.".into(),
            );
        } else if !entry_exists && is_typescript {
            notes.push(
                "Entry point inferred from package.json but not yet built. Run `tool build` first."
                    .into(),
            );
        }

        // Determine run args
        let run_args = if let Some(ref ep) = entry_point {
            vec![format!("${{__dirname}}/{}", ep)]
        } else {
            vec!["${__dirname}/dist/index.js".to_string()]
        };

        Some(DetectionResult {
            confidence,
            server_type: McpbServerType::Node,
            details: DetectionDetails {
                entry_point,
                script_name: None,
                package_manager: Some(PackageManager::Node(package_manager)),
                transport: Some(transport),
                build_command: Some(build_command),
                run_command: Some("node".to_string()),
                run_args,
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
        // Use options to override detected values
        let entry_point = options
            .entry_point
            .clone()
            .or(detection.details.entry_point.clone())
            .ok_or(DetectError::NoEntryPoint)?;

        let transport = options
            .transport
            .or(detection.details.transport)
            .unwrap_or(McpbTransport::Stdio);

        let package_manager = options
            .package_manager
            .or(detection.details.package_manager);

        // Get package name from options or package.json
        let name = if let Some(n) = &options.name {
            n.clone()
        } else if let Some(pkg) = read_json::<serde_json::Value>(&dir.join("package.json")) {
            pkg.get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "my-mcp-server".to_string())
        } else {
            "my-mcp-server".to_string()
        };

        // Build mcp_config
        let mcp_config = match transport {
            McpbTransport::Stdio => McpbMcpConfig {
                command: Some("node".to_string()),
                args: vec![format!("${{__dirname}}/{}", entry_point)],
                env: BTreeMap::new(),
                url: None,
                headers: BTreeMap::new(),
                oauth_config: None,
            },
            McpbTransport::Http => McpbMcpConfig {
                command: Some("node".to_string()),
                args: vec![
                    format!("${{__dirname}}/{}", entry_point),
                    "--port=${system_config.port}".to_string(),
                    "--host=${system_config.hostname}".to_string(),
                ],
                env: BTreeMap::new(),
                url: Some("http://${system_config.hostname}:${system_config.port}/mcp".to_string()),
                headers: BTreeMap::new(),
                oauth_config: None,
            },
        };

        // Get build command - use detected command which includes custom build scripts
        let build_cmd =
            detection
                .details
                .build_command
                .clone()
                .unwrap_or_else(|| match package_manager {
                    Some(PackageManager::Node(pm)) => pm.build_command().to_string(),
                    _ => "npm install".to_string(),
                });

        // Build manifest
        let manifest = McpbManifest {
            manifest_version: "0.3".to_string(),
            name: Some(name),
            version: Some("0.1.0".to_string()),
            description: Some("An MCP server".to_string()),
            author: None,
            server: McpbServer {
                server_type: Some(McpbServerType::Node),
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
            compatibility: None,
            privacy_policies: None,
            localization: None,
            meta: Some(serde_json::json!({
                "company.superrad.radical": {
                    "scripts": {
                        "build": build_cmd
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
            mcpbignore: mcpbignore_template().to_string(),
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

    fn create_node_project(tmp: &TempDir, has_sdk: bool) -> serde_json::Value {
        let deps = if has_sdk {
            serde_json::json!({
                "@modelcontextprotocol/sdk": "^1.0.0"
            })
        } else {
            serde_json::json!({})
        };

        let pkg = serde_json::json!({
            "name": "test-mcp-server",
            "version": "1.0.0",
            "main": "dist/index.js",
            "dependencies": deps
        });

        fs::write(
            tmp.path().join("package.json"),
            serde_json::to_string_pretty(&pkg).unwrap(),
        )
        .unwrap();

        pkg
    }

    #[test]
    fn test_detect_node_project_with_sdk() {
        let tmp = TempDir::new().unwrap();
        create_node_project(&tmp, true);
        fs::create_dir_all(tmp.path().join("dist")).unwrap();
        fs::write(tmp.path().join("dist/index.js"), "// server code").unwrap();

        let detector = NodeDetector::new();
        let result = detector.detect(tmp.path());

        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.confidence >= 0.9); // Should be high confidence with existing entry point
        assert_eq!(result.server_type, McpbServerType::Node);
        assert_eq!(
            result.details.entry_point,
            Some("dist/index.js".to_string())
        );
    }

    #[test]
    fn test_detect_node_project_unbuilt_typescript() {
        let tmp = TempDir::new().unwrap();

        // Create package.json with bin entry
        let pkg = serde_json::json!({
            "name": "test-mcp-server",
            "version": "1.0.0",
            "bin": {
                "mcp-server": "dist/index.js"
            },
            "dependencies": {
                "@modelcontextprotocol/sdk": "^1.0.0"
            }
        });
        fs::write(
            tmp.path().join("package.json"),
            serde_json::to_string_pretty(&pkg).unwrap(),
        )
        .unwrap();

        // Create tsconfig but don't build
        fs::write(tmp.path().join("tsconfig.json"), "{}").unwrap();

        let detector = NodeDetector::new();
        let result = detector.detect(tmp.path());

        assert!(result.is_some());
        let result = result.unwrap();
        // Should infer entry point from bin even without dist/
        assert_eq!(
            result.details.entry_point,
            Some("dist/index.js".to_string())
        );
        // Confidence should be lower since file doesn't exist
        assert!(result.confidence < 0.9);
    }

    #[test]
    fn test_detect_node_project_without_sdk() {
        let tmp = TempDir::new().unwrap();
        create_node_project(&tmp, false);

        let detector = NodeDetector::new();
        let result = detector.detect(tmp.path());

        assert!(result.is_none());
    }

    #[test]
    fn test_detect_package_manager_npm() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("package-lock.json"), "{}").unwrap();

        let detector = NodeDetector::new();
        assert_eq!(
            detector.detect_package_manager(tmp.path()),
            NodePackageManager::Npm
        );
    }

    #[test]
    fn test_detect_package_manager_pnpm() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("pnpm-lock.yaml"), "").unwrap();

        let detector = NodeDetector::new();
        assert_eq!(
            detector.detect_package_manager(tmp.path()),
            NodePackageManager::Pnpm
        );
    }

    #[test]
    fn test_detect_package_manager_bun() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("bun.lockb"), "").unwrap();

        let detector = NodeDetector::new();
        assert_eq!(
            detector.detect_package_manager(tmp.path()),
            NodePackageManager::Bun
        );
    }

    #[test]
    fn test_detect_package_manager_yarn() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("yarn.lock"), "").unwrap();

        let detector = NodeDetector::new();
        assert_eq!(
            detector.detect_package_manager(tmp.path()),
            NodePackageManager::Yarn
        );
    }
}
