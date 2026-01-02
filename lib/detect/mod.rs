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
pub use utils::{GrepMatch, GrepOptions, grep_dir, has_any_pattern, has_pattern};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Result of project detection.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
    /// Detected project type.
    pub server_type: McpbServerType,
    /// Detection details.
    pub details: DetectionDetails,
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
