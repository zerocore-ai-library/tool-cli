//! Python project detector.

use super::utils::{GrepOptions, find_first_relative, grep_dir, has_any_pattern, read_toml};
use super::{
    DetectError, DetectOptions, DetectionDetails, DetectionResult, DetectionSignals,
    GeneratedScaffold, ProjectDetector, SignalCallback,
};
use crate::mcpb::{
    McpbManifest, McpbMcpConfig, McpbServer, McpbServerType, McpbTransport, McpbUserConfigField,
    McpbUserConfigType, PackageManager, PythonPackageManager,
};
use crate::scaffold::mcpbignore_template;
use std::collections::BTreeMap;
use std::path::Path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Detector for Python MCP server projects.
pub struct PythonDetector;

/// Parsed pyproject.toml structure.
#[derive(Debug, serde::Deserialize)]
struct PyProject {
    project: Option<PyProjectMeta>,
    tool: Option<PyProjectTool>,
}

#[derive(Debug, serde::Deserialize)]
struct PyProjectMeta {
    name: Option<String>,
    dependencies: Option<Vec<String>>,
    scripts: Option<toml::Table>,
}

#[derive(Debug, serde::Deserialize)]
struct PyProjectTool {
    poetry: Option<PoetryConfig>,
    uv: Option<toml::Value>,
}

#[derive(Debug, serde::Deserialize)]
struct PoetryConfig {
    name: Option<String>,
    dependencies: Option<toml::Table>,
    scripts: Option<toml::Table>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl PythonDetector {
    /// Create a new Python detector.
    pub fn new() -> Self {
        Self
    }

    /// Detect package manager from project files.
    /// Returns (package_manager, has_lock_file).
    fn detect_package_manager(&self, dir: &Path) -> (PythonPackageManager, bool) {
        // Check for uv.lock
        if dir.join("uv.lock").exists() {
            return (PythonPackageManager::Uv, true);
        }

        // Check for poetry.lock
        if dir.join("poetry.lock").exists() {
            return (PythonPackageManager::Poetry, true);
        }

        // Check pyproject.toml for tool sections (no lock file = not certain)
        if let Some(pyproject) = read_toml::<PyProject>(&dir.join("pyproject.toml")) {
            if pyproject
                .tool
                .as_ref()
                .and_then(|t| t.uv.as_ref())
                .is_some()
            {
                return (PythonPackageManager::Uv, false);
            }
            if pyproject
                .tool
                .as_ref()
                .and_then(|t| t.poetry.as_ref())
                .is_some()
            {
                return (PythonPackageManager::Poetry, false);
            }
        }

        // Check if only requirements.txt exists
        if dir.join("requirements.txt").exists() && !dir.join("pyproject.toml").exists() {
            return (PythonPackageManager::Pip, false);
        }

        // Default to uv for modern projects with pyproject.toml
        if dir.join("pyproject.toml").exists() {
            return (PythonPackageManager::Uv, false);
        }

        (PythonPackageManager::Pip, false)
    }

    /// Find the file path for a Python module.
    /// Handles both single-file modules (foo.py) and packages (foo/__init__.py).
    fn find_module_path(&self, dir: &Path, module: &str) -> Option<String> {
        // Replace dots with path separators
        let module_path = module.replace('.', "/");

        // Candidates to check (in order of preference)
        let candidates = [
            // Package with __init__.py (src layout)
            format!("src/{}/__init__.py", module_path),
            // Package with __init__.py (flat layout)
            format!("{}/__init__.py", module_path),
            // Single file module (src layout)
            format!("src/{}.py", module_path),
            // Single file module (flat layout)
            format!("{}.py", module_path),
            // Package with server.py (common pattern, src layout)
            format!("src/{}/server.py", module_path),
            // Package with server.py (flat layout)
            format!("{}/server.py", module_path),
        ];

        candidates
            .into_iter()
            .find(|candidate| dir.join(candidate).exists())
    }

    /// Detect entry point from project configuration and file structure.
    /// Returns (entry_point_file, script_name, exists, from_config):
    /// - entry_point_file: path to entry point
    /// - script_name: CLI command if available
    /// - exists: file exists on disk
    /// - from_config: found in pyproject.toml scripts vs inferred
    fn detect_entry_point(&self, dir: &Path) -> (Option<String>, Option<String>, bool, bool) {
        // 1. Check pyproject.toml for scripts (from config)
        if let Some(pyproject) = read_toml::<PyProject>(&dir.join("pyproject.toml")) {
            // Check [project.scripts]
            if let Some(scripts) = pyproject.project.as_ref().and_then(|p| p.scripts.as_ref())
                && let Some((script_name, first)) = scripts.iter().next()
                && let Some(entry) = first.as_str()
                && let Some(module) = entry.split(':').next()
            {
                // Try to find the source file
                if let Some(path) = self.find_module_path(dir, module) {
                    return (Some(path), Some(script_name.clone()), true, true);
                }
                // Even if file not found, return script name for running
                return (None, Some(script_name.clone()), false, true);
            }

            // Check [tool.poetry.scripts]
            if let Some(scripts) = pyproject
                .tool
                .as_ref()
                .and_then(|t| t.poetry.as_ref())
                .and_then(|p| p.scripts.as_ref())
                && let Some((script_name, first)) = scripts.iter().next()
                && let Some(entry) = first.as_str()
                && let Some(module) = entry.split(':').next()
            {
                if let Some(path) = self.find_module_path(dir, module) {
                    return (Some(path), Some(script_name.clone()), true, true);
                }
                return (None, Some(script_name.clone()), false, true);
            }
        }

        // 2. Look for files with FastMCP or mcp.server imports (not from config)
        let options = GrepOptions {
            extensions: vec!["py".into()],
            respect_gitignore: true,
            first_match_only: true,
            ..Default::default()
        };

        let patterns = [r"FastMCP\s*\(", r"from mcp\.server", r"import mcp\.server"];

        for pattern in patterns {
            let matches = grep_dir(dir, pattern, &options);
            if let Some(m) = matches.first()
                && let Ok(rel) = m.path.strip_prefix(dir)
            {
                return (Some(rel.to_string_lossy().to_string()), None, true, false);
            }
        }

        // 3. Common patterns (not from config)
        let patterns = [
            "main.py",
            "server.py",
            "app.py",
            "src/main.py",
            "src/server.py",
            "server/main.py",
            "server/__main__.py",
        ];

        if let Some(found) = find_first_relative(dir, &patterns) {
            return (Some(found), None, true, false);
        }

        (None, None, false, false)
    }

    /// Detect transport by grepping source files.
    fn detect_transport(&self, dir: &Path) -> McpbTransport {
        let http_patterns = [
            r"streamable_http_app",
            r"stateless_http\s*=\s*True",
            r"FastAPI\s*\(",
            r"uvicorn\.run",
            r"from fastapi",
        ];

        if has_any_pattern(dir, &http_patterns, &["py"]).is_some() {
            McpbTransport::Http
        } else {
            McpbTransport::Stdio
        }
    }

    /// Check if project has MCP dependency.
    fn has_mcp_dependency(&self, dir: &Path) -> bool {
        // Check pyproject.toml
        if let Some(pyproject) = read_toml::<PyProject>(&dir.join("pyproject.toml")) {
            // Check [project.dependencies]
            if let Some(deps) = pyproject
                .project
                .as_ref()
                .and_then(|p| p.dependencies.as_ref())
                && deps.iter().any(|d| d.starts_with("mcp"))
            {
                return true;
            }

            // Check [tool.poetry.dependencies]
            if let Some(deps) = pyproject
                .tool
                .as_ref()
                .and_then(|t| t.poetry.as_ref())
                .and_then(|p| p.dependencies.as_ref())
                && deps.contains_key("mcp")
            {
                return true;
            }
        }

        // Check requirements.txt
        if let Ok(content) = std::fs::read_to_string(dir.join("requirements.txt"))
            && content.lines().any(|line| {
                let line = line.trim();
                line.starts_with("mcp") || line.contains("mcp>=") || line.contains("mcp==")
            })
        {
            return true;
        }

        // Fallback: grep for imports
        has_any_pattern(
            dir,
            &[r"from mcp\.", r"import mcp", r"from mcp import"],
            &["py"],
        )
        .is_some()
    }

    /// Core detection logic with optional signal callback.
    fn detect_impl(
        &self,
        dir: &Path,
        on_signal: Option<SignalCallback<'_>>,
    ) -> Option<DetectionResult> {
        let has_pyproject = dir.join("pyproject.toml").exists();
        let has_requirements = dir.join("requirements.txt").exists();

        if !has_pyproject && !has_requirements {
            return None;
        }

        // Gather detection signals, reporting each as it's evaluated
        let (entry_point, script_name, entry_exists, entry_from_config) =
            self.detect_entry_point(dir);
        if let Some(cb) = &on_signal {
            cb("Entry point in config", entry_from_config, "30%");
        }
        if let Some(cb) = &on_signal {
            cb("Entry point exists", entry_exists, "20%");
        }

        let has_mcp_sdk = self.has_mcp_dependency(dir);
        if let Some(cb) = &on_signal {
            cb("MCP SDK detected (mcp)", has_mcp_sdk, "10%");
        }

        let (package_manager, has_lock_file) = self.detect_package_manager(dir);
        if let Some(cb) = &on_signal {
            cb("Lock file found", has_lock_file, "10%");
        }

        let name_from_config =
            if let Some(pyproject) = read_toml::<PyProject>(&dir.join("pyproject.toml")) {
                pyproject
                    .project
                    .as_ref()
                    .and_then(|p| p.name.as_ref())
                    .is_some()
                    || pyproject
                        .tool
                        .as_ref()
                        .and_then(|t| t.poetry.as_ref())
                        .and_then(|p| p.name.as_ref())
                        .is_some()
            } else {
                false
            };
        if let Some(cb) = &on_signal {
            cb("Name in config", name_from_config, "5%");
        }

        let transport = self.detect_transport(dir);

        // Build detection signals
        let signals = DetectionSignals {
            entry_point_from_config: entry_from_config,
            entry_point_exists: entry_exists,
            has_mcp_sdk,
            package_manager_certain: has_lock_file,
            name_from_config,
        };

        let confidence = signals.confidence();
        let mut notes = signals.warnings();

        if entry_point.is_none() && script_name.is_none() {
            notes.push(
                "Could not auto-detect entry point. Specify --entry to set it manually.".into(),
            );
        }

        let (run_command, run_args) = match package_manager {
            PythonPackageManager::Uv => {
                let args = if let Some(ref sn) = script_name {
                    vec!["run".to_string(), sn.clone()]
                } else if let Some(ref ep) = entry_point {
                    vec!["run".to_string(), ep.clone()]
                } else {
                    vec!["run".to_string(), "main.py".to_string()]
                };
                ("uv".to_string(), args)
            }
            PythonPackageManager::Poetry => {
                let args = if let Some(ref sn) = script_name {
                    vec!["run".to_string(), sn.clone()]
                } else if let Some(ref ep) = entry_point {
                    vec!["run".to_string(), "python".to_string(), ep.clone()]
                } else {
                    vec![
                        "run".to_string(),
                        "python".to_string(),
                        "main.py".to_string(),
                    ]
                };
                ("poetry".to_string(), args)
            }
            PythonPackageManager::Pip => {
                if let Some(ref sn) = script_name {
                    (format!(".venv/bin/{}", sn), vec![])
                } else {
                    let args = if let Some(ref ep) = entry_point {
                        vec![ep.clone()]
                    } else {
                        vec!["main.py".to_string()]
                    };
                    (".venv/bin/python".to_string(), args)
                }
            }
        };

        Some(DetectionResult {
            confidence,
            server_type: McpbServerType::Python,
            details: DetectionDetails {
                entry_point,
                script_name,
                package_manager: Some(PackageManager::Python(package_manager)),
                transport: Some(transport),
                build_command: Some(package_manager.build_command().to_string()),
                run_command: Some(run_command),
                run_args,
                notes,
            },
            signals,
        })
    }

    /// Get project name from pyproject.toml or directory name.
    fn get_project_name(&self, dir: &Path) -> String {
        if let Some(pyproject) = read_toml::<PyProject>(&dir.join("pyproject.toml")) {
            if let Some(name) = pyproject.project.and_then(|p| p.name) {
                return name;
            }
            if let Some(name) = pyproject.tool.and_then(|t| t.poetry).and_then(|p| p.name) {
                return name;
            }
        }

        dir.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "my-mcp-server".to_string())
    }
}

impl Default for PythonDetector {
    fn default() -> Self {
        Self::new()
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl ProjectDetector for PythonDetector {
    fn name(&self) -> &'static str {
        "python"
    }

    fn display_name(&self) -> &'static str {
        "Python"
    }

    fn server_type(&self) -> McpbServerType {
        McpbServerType::Python
    }

    fn detect(&self, dir: &Path) -> Option<DetectionResult> {
        self.detect_impl(dir, None)
    }

    fn detect_verbose(&self, dir: &Path, on_signal: SignalCallback<'_>) -> Option<DetectionResult> {
        self.detect_impl(dir, Some(on_signal))
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
            .or(detection.details.entry_point.clone());

        let script_name = detection.details.script_name.clone();

        // Need either entry_point or script_name
        if entry_point.is_none() && script_name.is_none() {
            return Err(DetectError::NoEntryPoint);
        }

        let transport = options
            .transport
            .or(detection.details.transport)
            .unwrap_or(McpbTransport::Stdio);

        let package_manager = options
            .package_manager
            .or(detection.details.package_manager);

        let python_pm = match package_manager {
            Some(PackageManager::Python(pm)) => pm,
            _ => PythonPackageManager::Uv,
        };

        // Get package name
        let name = options
            .name
            .clone()
            .unwrap_or_else(|| self.get_project_name(dir));

        // Build mcp_config based on package manager and transport
        let mcp_config = build_mcp_config(
            entry_point.as_deref(),
            script_name.as_deref(),
            transport,
            python_pm,
        );

        // Get build command
        let build_cmd = python_pm.build_command().to_string();

        // For manifest entry_point, prefer actual file path over script name
        // If we only have script_name, search for a likely source file
        let manifest_entry_point = if let Some(ref ep) = entry_point {
            Some(ep.clone())
        } else if script_name.is_some() {
            // Try to find the source file that the script points to
            self.detect_entry_point(dir).0.or_else(|| {
                // Fallback: just note the script name
                script_name.clone()
            })
        } else {
            None
        };

        // Create user_config with host for HTTP transport
        let user_config = if transport == McpbTransport::Http {
            let mut cfg = BTreeMap::new();
            cfg.insert(
                "host".to_string(),
                McpbUserConfigField {
                    field_type: McpbUserConfigType::String,
                    title: "Bind Address".to_string(),
                    description: Some("Network interface to bind to".to_string()),
                    required: None,
                    default: Some(serde_json::json!("127.0.0.1")),
                    multiple: None,
                    sensitive: None,
                    enum_values: None,
                    min: None,
                    max: None,
                },
            );
            Some(cfg)
        } else {
            None
        };

        // Build manifest
        let manifest = McpbManifest {
            manifest_version: "0.3".to_string(),
            name: Some(name),
            version: Some("0.1.0".to_string()),
            description: Some("An MCP server".to_string()),
            author: None,
            server: McpbServer {
                server_type: Some(McpbServerType::Python),
                transport,
                entry_point: manifest_entry_point,
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
            user_config,
            system_config: None,
            compatibility: None,
            privacy_policies: None,
            localization: None,
            meta: Some(serde_json::json!({
                "store.tool.mcpb": {
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
// Functions
//--------------------------------------------------------------------------------------------------

/// Build MCP config for Python based on package manager and transport.
/// Prefers script_name over entry_point when available.
fn build_mcp_config(
    entry_point: Option<&str>,
    script_name: Option<&str>,
    transport: McpbTransport,
    pm: PythonPackageManager,
) -> McpbMcpConfig {
    // Prefer script_name for running (e.g., `uv run mcp-server-git`)
    // Fall back to entry_point file (e.g., `uv run src/server.py`)
    let run_target = script_name.or(entry_point).unwrap_or("main.py");

    let (command, mut args) = match pm {
        PythonPackageManager::Uv => ("uv".to_string(), vec!["run".to_string()]),
        PythonPackageManager::Poetry => {
            // For poetry with script_name, just use `poetry run <script>`
            // For poetry with file, use `poetry run python <file>`
            if script_name.is_some() {
                ("poetry".to_string(), vec!["run".to_string()])
            } else {
                (
                    "poetry".to_string(),
                    vec!["run".to_string(), "python".to_string()],
                )
            }
        }
        PythonPackageManager::Pip => {
            // For pip with script_name, the script is in .venv/bin/
            // For pip with file, use .venv/bin/python <file>
            if script_name.is_some() {
                (format!(".venv/bin/{}", run_target), vec![])
            } else {
                (".venv/bin/python".to_string(), vec![])
            }
        }
    };

    // Only add run_target to args if we didn't embed it in command (pip with script_name case)
    if !(pm == PythonPackageManager::Pip && script_name.is_some()) {
        args.push(run_target.to_string());
    }

    match transport {
        McpbTransport::Stdio => McpbMcpConfig {
            command: Some(command),
            args,
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
            oauth_config: None,
            platform_overrides: BTreeMap::new(),
        },
        McpbTransport::Http => {
            args.push("--port".to_string());
            args.push("${system_config.port}".to_string());
            args.push("--host".to_string());
            args.push("${user_config.host}".to_string());

            McpbMcpConfig {
                command: Some(command),
                args,
                env: BTreeMap::new(),
                url: Some("http://${user_config.host}:${system_config.port}/mcp".to_string()),
                headers: BTreeMap::new(),
                oauth_config: None,
                platform_overrides: BTreeMap::new(),
            }
        }
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

    #[test]
    fn test_detect_python_project_with_mcp() {
        let tmp = TempDir::new().unwrap();

        // Create pyproject.toml with mcp dependency
        let pyproject = r#"
[project]
name = "test-mcp-server"
dependencies = ["mcp>=1.0.0"]
"#;
        fs::write(tmp.path().join("pyproject.toml"), pyproject).unwrap();
        fs::write(tmp.path().join("uv.lock"), "").unwrap(); // Add lock file
        fs::write(
            tmp.path().join("main.py"),
            "from mcp.server.fastmcp import FastMCP",
        )
        .unwrap();

        let detector = PythonDetector::new();
        let result = detector.detect(tmp.path());

        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.signals.has_mcp_sdk);
        assert!(result.signals.entry_point_exists);
        assert!(result.signals.name_from_config);
        assert_eq!(result.server_type, McpbServerType::Python);
    }

    #[test]
    fn test_detect_python_project_without_mcp() {
        let tmp = TempDir::new().unwrap();

        let pyproject = r#"
[project]
name = "test-project"
dependencies = ["requests"]
"#;
        fs::write(tmp.path().join("pyproject.toml"), pyproject).unwrap();
        fs::write(tmp.path().join("main.py"), "print('hello')").unwrap();

        let detector = PythonDetector::new();
        let result = detector.detect(tmp.path());

        // Now detected but with lower confidence (no SDK = -0.10)
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(!result.signals.has_mcp_sdk);
        assert!(result.confidence < 1.0);
    }

    #[test]
    fn test_detect_package_manager_uv() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("uv.lock"), "").unwrap();

        let detector = PythonDetector::new();
        let (pm, certain) = detector.detect_package_manager(tmp.path());
        assert_eq!(pm, PythonPackageManager::Uv);
        assert!(certain);
    }

    #[test]
    fn test_detect_package_manager_poetry() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("poetry.lock"), "").unwrap();

        let detector = PythonDetector::new();
        let (pm, certain) = detector.detect_package_manager(tmp.path());
        assert_eq!(pm, PythonPackageManager::Poetry);
        assert!(certain);
    }

    #[test]
    fn test_detect_package_manager_pip() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("requirements.txt"), "mcp>=1.0.0").unwrap();

        let detector = PythonDetector::new();
        let (pm, certain) = detector.detect_package_manager(tmp.path());
        assert_eq!(pm, PythonPackageManager::Pip);
        assert!(!certain); // No lock file for pip
    }

    #[test]
    fn test_detect_transport_http() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("main.py"),
            r#"
from mcp.server.fastmcp import FastMCP
mcp = FastMCP("test", stateless_http=True)
app = mcp.streamable_http_app()
"#,
        )
        .unwrap();

        let detector = PythonDetector::new();
        assert_eq!(detector.detect_transport(tmp.path()), McpbTransport::Http);
    }

    #[test]
    fn test_detect_transport_stdio() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("main.py"),
            r#"
from mcp.server.fastmcp import FastMCP
mcp = FastMCP("test")
mcp.run()
"#,
        )
        .unwrap();

        let detector = PythonDetector::new();
        assert_eq!(detector.detect_transport(tmp.path()), McpbTransport::Stdio);
    }
}
