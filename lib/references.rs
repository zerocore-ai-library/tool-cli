//! Plugin reference system.
//!
//! Provides a unified way to reference tools using the format: `[<namespace>/]<name>[@<version>]`.

use crate::error::{ToolError, ToolResult};
use regex::Regex;
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Regex pattern for validating namespace segments.
const NAMESPACE_PATTERN: &str = r"^[a-z][a-z0-9_-]{1,49}$";

/// Regex pattern for validating name segments.
const NAME_PATTERN: &str = r"^[a-z][a-z0-9_-]{0,99}$";

/// Compiled namespace regex.
static NAMESPACE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(NAMESPACE_PATTERN).expect("Invalid regex"));

/// Compiled name regex.
static NAME_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(NAME_PATTERN).expect("Invalid regex"));

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A plugin reference that identifies tools.
///
/// References follow the format: `[<namespace>/]<name>[@<version>]`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginRef {
    /// Optional namespace (e.g., "my-org").
    namespace: Option<String>,

    /// Required name (e.g., "my-tool").
    name: String,

    /// Optional semantic version requirement.
    version: Option<VersionReq>,

    /// Raw version string as provided (without semver interpretation).
    version_str: Option<String>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl PluginRef {
    /// Parse a plugin reference from a string.
    pub fn parse(input: &str) -> ToolResult<Self> {
        if input.is_empty() {
            return Err(ToolError::InvalidReference("Empty reference".into()));
        }

        // Split by '@' to separate version
        let (base, version, version_str) = if let Some(at_pos) = input.rfind('@') {
            let ver_str = &input[at_pos + 1..];
            if ver_str.is_empty() {
                return Err(ToolError::InvalidReference(
                    "Empty version after '@'".into(),
                ));
            }
            let version = VersionReq::parse(ver_str).map_err(|e| {
                ToolError::InvalidReference(format!("Invalid version '{}': {}", ver_str, e))
            })?;
            (
                input[..at_pos].to_string(),
                Some(version),
                Some(ver_str.to_string()),
            )
        } else {
            (input.to_string(), None, None)
        };

        // Split by '/' to separate namespace
        let (namespace, name) = if let Some(slash_pos) = base.find('/') {
            let namespace_part = &base[..slash_pos];
            let name_part = &base[slash_pos + 1..];

            if namespace_part.is_empty() {
                return Err(ToolError::InvalidReference(
                    "Empty namespace before '/'".into(),
                ));
            }
            if name_part.is_empty() {
                return Err(ToolError::InvalidReference("Empty name after '/'".into()));
            }

            (Some(namespace_part.to_string()), name_part.to_string())
        } else {
            (None, base)
        };

        // Validate
        if input.contains("//") {
            return Err(ToolError::InvalidReference(
                "Double slash '//' not allowed".into(),
            ));
        }
        if input.contains("@@") {
            return Err(ToolError::InvalidReference("Double '@' not allowed".into()));
        }

        if let Some(ref ns) = namespace {
            Self::validate_namespace(ns)?;
        }
        Self::validate_name(&name)?;

        Ok(PluginRef {
            namespace,
            name,
            version,
            version_str,
        })
    }

    /// Create a new local-only PluginRef with just a name.
    pub fn new(name: impl Into<String>) -> ToolResult<Self> {
        let name = name.into();
        Self::validate_name(&name)?;
        Ok(PluginRef {
            namespace: None,
            name,
            version: None,
            version_str: None,
        })
    }

    /// Set the namespace for this reference.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> ToolResult<Self> {
        let namespace = namespace.into();
        Self::validate_namespace(&namespace)?;
        self.namespace = Some(namespace);
        Ok(self)
    }

    /// Set the version requirement for this reference.
    pub fn with_version(mut self, version: VersionReq) -> Self {
        self.version = Some(version);
        self
    }

    /// Get the namespace of this reference.
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Get the name of this reference.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the version requirement of this reference.
    pub fn version(&self) -> Option<&VersionReq> {
        self.version.as_ref()
    }

    /// Get the raw version string as provided.
    pub fn version_str(&self) -> Option<&str> {
        self.version_str.as_deref()
    }

    /// Check if this is a local reference (no namespace).
    pub fn is_local(&self) -> bool {
        self.namespace.is_none()
    }

    /// Check if this is a registry reference (has namespace).
    pub fn is_registry(&self) -> bool {
        self.namespace.is_some()
    }

    /// Validate a namespace segment.
    fn validate_namespace(namespace: &str) -> ToolResult<()> {
        if namespace.len() < 2 {
            return Err(ToolError::InvalidReference(format!(
                "Namespace '{}' must be at least 2 characters",
                namespace
            )));
        }
        if namespace.len() > 50 {
            return Err(ToolError::InvalidReference(format!(
                "Namespace '{}' exceeds 50 character limit",
                namespace
            )));
        }
        if !NAMESPACE_REGEX.is_match(namespace) {
            return Err(ToolError::InvalidReference(format!(
                "Namespace '{}' must start with lowercase letter and contain only lowercase letters, numbers, hyphens, and underscores",
                namespace
            )));
        }
        Ok(())
    }

    /// Validate a name segment.
    fn validate_name(name: &str) -> ToolResult<()> {
        if name.is_empty() {
            return Err(ToolError::InvalidReference("Name cannot be empty".into()));
        }
        if name.len() > 100 {
            return Err(ToolError::InvalidReference(format!(
                "Name '{}' exceeds 100 character limit",
                name
            )));
        }
        if !NAME_REGEX.is_match(name) {
            return Err(ToolError::InvalidReference(format!(
                "Name '{}' must start with lowercase letter and contain only lowercase letters, numbers, hyphens, and underscores",
                name
            )));
        }
        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl fmt::Display for PluginRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref ns) = self.namespace {
            write!(f, "{}/", ns)?;
        }
        write!(f, "{}", self.name)?;
        // Use the raw version string to avoid semver adding caret prefix
        if let Some(ref version_str) = self.version_str {
            write!(f, "@{}", version_str)?;
        }
        Ok(())
    }
}

impl FromStr for PluginRef {
    type Err = ToolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PluginRef::parse(s)
    }
}
