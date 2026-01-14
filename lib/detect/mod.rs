//! Project detection for auto-generating MCPB scaffolding.
//!
//! This module provides a modular system for detecting existing MCP server projects
//! and generating appropriate MCPB manifests and configuration files.

mod node;
mod python;
mod rust;
mod utils;

use crate::error::{ToolError, ToolResult};
use crate::mcpb::{McpbManifest, McpbServerType, McpbTransport, PackageManager};
use std::path::{Path, PathBuf};

pub use node::NodeDetector;
pub use python::PythonDetector;
pub use rust::RustDetector;
pub use utils::{
    GrepMatch, GrepOptions, grep_dir, has_any_pattern, has_pattern, parse_env_example,
};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Result of project detection.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Confidence score (0.0 - 1.0), calculated from signals.
    pub confidence: f32,
    /// Detected project type.
    pub server_type: McpbServerType,
    /// Detection details.
    pub details: DetectionDetails,
    /// Detection signals used to calculate confidence.
    pub signals: DetectionSignals,
}

/// Signals used for deduction-based confidence calculation.
/// Starts at 1.0 and deducts for each missing/uncertain piece.
#[derive(Debug, Clone, Default)]
pub struct DetectionSignals {
    /// Entry point found in config (package.json bin/main, pyproject scripts, Cargo [[bin]]).
    pub entry_point_from_config: bool,
    /// Entry point file exists on disk.
    pub entry_point_exists: bool,
    /// MCP SDK detected in dependencies.
    pub has_mcp_sdk: bool,
    /// Package manager is certain (lock file exists).
    pub package_manager_certain: bool,
    /// Name found in config (vs inferred from directory).
    pub name_from_config: bool,
}

impl DetectionSignals {
    /// Calculate confidence score using deduction method.
    /// Starts at 1.0 and deducts for each missing signal.
    pub fn confidence(&self) -> f32 {
        let mut score: f32 = 1.0;

        // Critical: Entry point detection
        if !self.entry_point_from_config {
            score -= 0.30;
        }
        if !self.entry_point_exists {
            score -= 0.20;
        }

        // Important: SDK and package manager
        if !self.has_mcp_sdk {
            score -= 0.10;
        }
        if !self.package_manager_certain {
            score -= 0.10;
        }

        // Minor: Metadata quality
        if !self.name_from_config {
            score -= 0.05;
        }

        score.max(0.0)
    }

    /// Get warnings based on signals.
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if !self.has_mcp_sdk {
            warnings.push("No MCP SDK detected. This may be a custom implementation.".into());
        }
        if !self.entry_point_exists {
            warnings.push("Entry point file not found. Project may need to be built first.".into());
        }

        warnings
    }
}

/// Environment variable configuration type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvConfigType {
    /// User-provided config (API keys, credentials, etc.)
    User,
    /// System-managed config (PORT, HOST)
    System,
}

/// Environment variable parsed from .env.example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    /// Variable name (e.g., "API_KEY").
    pub name: String,
    /// Default value from .env.example (if any).
    pub default: Option<String>,
    /// Whether this is a sensitive value (KEY, SECRET, TOKEN, PASSWORD, etc.)
    pub sensitive: bool,
    /// Config type (user_config or system_config).
    pub config_type: EnvConfigType,
    /// Inferred MCPB config type (string, number, boolean, port, hostname).
    pub value_type: EnvValueType,
}

/// Inferred value type for env var.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvValueType {
    String,
    Number,
    Boolean,
    Port,
    Hostname,
}

impl EnvVar {
    /// Convert env var name to manifest config key (lowercase, underscores).
    pub fn config_key(&self) -> String {
        self.name.to_lowercase()
    }
}

/// Detailed detection information.
#[derive(Debug, Clone, Default)]
pub struct DetectionDetails {
    /// Detected entry point path (relative to project root).
    pub entry_point: Option<String>,
    /// CLI script name (from [project.scripts] or similar).
    /// When present, use this instead of entry_point for running.
    pub script_name: Option<String>,
    /// Detected package manager.
    pub package_manager: Option<PackageManager>,
    /// Detected transport type.
    pub transport: Option<McpbTransport>,
    /// Suggested build command.
    pub build_command: Option<String>,
    /// Suggested run command.
    pub run_command: Option<String>,
    /// Suggested run args.
    pub run_args: Vec<String>,
    /// Additional notes/warnings for the user.
    pub notes: Vec<String>,
}

/// Generated scaffolding files.
#[derive(Debug, Clone)]
pub struct GeneratedScaffold {
    /// manifest.json content.
    pub manifest: McpbManifest,
    /// .mcpbignore content.
    pub mcpbignore: String,
    /// Files that would be created (for display).
    pub files_to_create: Vec<PathBuf>,
    /// Files that already exist and would be overwritten.
    pub files_to_overwrite: Vec<PathBuf>,
}

/// Options for detection and generation.
#[derive(Debug, Clone, Default)]
pub struct DetectOptions {
    /// Override detected entry point.
    pub entry_point: Option<String>,
    /// Override detected transport.
    pub transport: Option<McpbTransport>,
    /// Override detected package manager.
    pub package_manager: Option<PackageManager>,
    /// Package name override.
    pub name: Option<String>,
}

/// Error type for detection operations.
#[derive(Debug, Clone)]
pub enum DetectError {
    /// No entry point could be detected.
    NoEntryPoint,
    /// Project type could not be determined.
    UnknownProjectType,
    /// Multiple project types detected with similar confidence.
    AmbiguousProject(Vec<String>),
    /// IO error during detection.
    IoError(String),
}

impl std::fmt::Display for DetectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEntryPoint => write!(f, "Could not detect entry point"),
            Self::UnknownProjectType => write!(f, "Could not determine project type"),
            Self::AmbiguousProject(types) => {
                write!(f, "Multiple project types detected: {}", types.join(", "))
            }
            Self::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for DetectError {}

impl From<DetectError> for ToolError {
    fn from(e: DetectError) -> Self {
        ToolError::Generic(e.to_string())
    }
}

//--------------------------------------------------------------------------------------------------
// Traits
//--------------------------------------------------------------------------------------------------

/// Trait for project type detectors.
pub trait ProjectDetector: Send + Sync {
    /// Unique identifier for this detector.
    fn name(&self) -> &'static str;

    /// Human-readable display name.
    fn display_name(&self) -> &'static str;

    /// The server type this detector handles.
    fn server_type(&self) -> McpbServerType;

    /// Check if this detector can handle the project.
    /// Returns None if not applicable, Some(result) with confidence if applicable.
    fn detect(&self, dir: &Path) -> Option<DetectionResult>;

    /// Generate MCPB scaffolding for the detected project.
    fn generate(
        &self,
        dir: &Path,
        detection: &DetectionResult,
        options: &DetectOptions,
    ) -> Result<GeneratedScaffold, DetectError>;
}

//--------------------------------------------------------------------------------------------------
// Types: Registry
//--------------------------------------------------------------------------------------------------

/// Registry of project detectors.
pub struct DetectorRegistry {
    detectors: Vec<Box<dyn ProjectDetector>>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl DetectorRegistry {
    /// Create a new registry with default detectors.
    pub fn new() -> Self {
        Self {
            detectors: vec![
                Box::new(NodeDetector::new()),
                Box::new(PythonDetector::new()),
                Box::new(RustDetector::new()),
            ],
        }
    }

    /// Register a custom detector.
    pub fn register(&mut self, detector: Box<dyn ProjectDetector>) {
        self.detectors.push(detector);
    }

    /// Detect project type, returning best match.
    pub fn detect(&self, dir: &Path) -> Option<DetectionMatch> {
        let mut best: Option<DetectionMatch> = None;

        for detector in &self.detectors {
            if let Some(result) = detector.detect(dir) {
                let current_match = DetectionMatch {
                    detector_name: detector.name(),
                    display_name: detector.display_name(),
                    server_type: detector.server_type(),
                    result,
                };

                match &best {
                    None => best = Some(current_match),
                    Some(prev) if current_match.result.confidence > prev.result.confidence => {
                        best = Some(current_match);
                    }
                    _ => {}
                }
            }
        }

        best
    }

    /// Detect all applicable project types (for monorepos or multi-runtime projects).
    pub fn detect_all(&self, dir: &Path) -> Vec<DetectionMatch> {
        self.detectors
            .iter()
            .filter_map(|d| {
                d.detect(dir).map(|result| DetectionMatch {
                    detector_name: d.name(),
                    display_name: d.display_name(),
                    server_type: d.server_type(),
                    result,
                })
            })
            .collect()
    }

    /// Get a detector by name.
    pub fn get(&self, name: &str) -> Option<&dyn ProjectDetector> {
        self.detectors
            .iter()
            .find(|d| d.name() == name)
            .map(|d| d.as_ref())
    }

    /// Generate scaffolding using a specific detector.
    pub fn generate(
        &self,
        detector_name: &str,
        dir: &Path,
        detection: &DetectionResult,
        options: &DetectOptions,
    ) -> Result<GeneratedScaffold, DetectError> {
        let detector = self
            .get(detector_name)
            .ok_or(DetectError::UnknownProjectType)?;

        detector.generate(dir, detection, options)
    }
}

impl Default for DetectorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A detection match with detector metadata.
#[derive(Debug, Clone)]
pub struct DetectionMatch {
    /// Detector identifier.
    pub detector_name: &'static str,
    /// Human-readable detector name.
    pub display_name: &'static str,
    /// Server type.
    pub server_type: McpbServerType,
    /// Detection result.
    pub result: DetectionResult,
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Convenience function to detect and generate scaffolding.
pub fn detect_and_generate(
    dir: &Path,
    options: &DetectOptions,
) -> ToolResult<(DetectionMatch, GeneratedScaffold)> {
    let registry = DetectorRegistry::new();

    let detection = registry
        .detect(dir)
        .ok_or_else(|| ToolError::Generic("No MCP server project detected".into()))?;

    let scaffold = registry.generate(detection.detector_name, dir, &detection.result, options)?;

    Ok((detection, scaffold))
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detection_signals_perfect_confidence() {
        let signals = DetectionSignals {
            entry_point_from_config: true,
            entry_point_exists: true,
            has_mcp_sdk: true,
            package_manager_certain: true,
            name_from_config: true,
        };
        assert_eq!(signals.confidence(), 1.0);
    }

    #[test]
    fn test_detection_signals_no_entry_point_config() {
        let signals = DetectionSignals {
            entry_point_from_config: false,
            entry_point_exists: true,
            has_mcp_sdk: true,
            package_manager_certain: true,
            name_from_config: true,
        };
        // 1.0 - 0.30 = 0.70
        assert!((signals.confidence() - 0.70).abs() < 0.001);
    }

    #[test]
    fn test_detection_signals_entry_point_not_exists() {
        let signals = DetectionSignals {
            entry_point_from_config: true,
            entry_point_exists: false,
            has_mcp_sdk: true,
            package_manager_certain: true,
            name_from_config: true,
        };
        // 1.0 - 0.20 = 0.80
        assert!((signals.confidence() - 0.80).abs() < 0.001);
    }

    #[test]
    fn test_detection_signals_no_mcp_sdk() {
        let signals = DetectionSignals {
            entry_point_from_config: true,
            entry_point_exists: true,
            has_mcp_sdk: false,
            package_manager_certain: true,
            name_from_config: true,
        };
        // 1.0 - 0.10 = 0.90
        assert!((signals.confidence() - 0.90).abs() < 0.001);
    }

    #[test]
    fn test_detection_signals_multiple_deductions() {
        let signals = DetectionSignals {
            entry_point_from_config: false, // -0.30
            entry_point_exists: false,      // -0.20
            has_mcp_sdk: false,             // -0.10
            package_manager_certain: false, // -0.10
            name_from_config: false,        // -0.05
        };
        // 1.0 - 0.30 - 0.20 - 0.10 - 0.10 - 0.05 = 0.25
        assert!((signals.confidence() - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_detection_signals_warnings() {
        let signals = DetectionSignals {
            entry_point_from_config: true,
            entry_point_exists: false,
            has_mcp_sdk: false,
            package_manager_certain: true,
            name_from_config: true,
        };

        let warnings = signals.warnings();
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|w| w.contains("MCP SDK")));
        assert!(warnings.iter().any(|w| w.contains("Entry point")));
    }

    #[test]
    fn test_detection_signals_no_warnings_when_all_good() {
        let signals = DetectionSignals {
            entry_point_from_config: true,
            entry_point_exists: true,
            has_mcp_sdk: true,
            package_manager_certain: true,
            name_from_config: true,
        };

        assert!(signals.warnings().is_empty());
    }

    #[test]
    fn test_env_var_config_key() {
        let var = EnvVar {
            name: "API_KEY".to_string(),
            default: None,
            sensitive: true,
            config_type: EnvConfigType::User,
            value_type: EnvValueType::String,
        };
        assert_eq!(var.config_key(), "api_key");

        let var2 = EnvVar {
            name: "DATABASE_URL".to_string(),
            default: Some("postgres://localhost/db".to_string()),
            sensitive: true,
            config_type: EnvConfigType::User,
            value_type: EnvValueType::String,
        };
        assert_eq!(var2.config_key(), "database_url");
    }
}
