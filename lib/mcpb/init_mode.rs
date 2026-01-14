//! Initialization mode for tool init command.

use super::types::{
    McpbServerType, McpbTransport, NodePackageManager, PackageManager, PythonPackageManager,
};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Initialization mode for `tool init`.
#[derive(Debug, Clone)]
pub enum InitMode {
    /// Bundled server with scaffolding.
    Bundle {
        server_type: McpbServerType,
        transport: McpbTransport,
        package_manager: Option<PackageManager>,
    },
    /// Reference to external server (no scaffolding).
    Reference { transport: McpbTransport },
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl InitMode {
    /// Create bundle mode with stdio transport.
    pub fn bundle_stdio(server_type: McpbServerType) -> Self {
        Self::Bundle {
            server_type,
            transport: McpbTransport::Stdio,
            package_manager: Self::default_package_manager(server_type),
        }
    }

    /// Create bundle mode with HTTP transport.
    pub fn bundle_http(server_type: McpbServerType) -> Self {
        Self::Bundle {
            server_type,
            transport: McpbTransport::Http,
            package_manager: Self::default_package_manager(server_type),
        }
    }

    /// Create bundle mode with explicit package manager.
    pub fn bundle(
        server_type: McpbServerType,
        transport: McpbTransport,
        package_manager: Option<PackageManager>,
    ) -> Self {
        Self::Bundle {
            server_type,
            transport,
            package_manager: package_manager.or_else(|| Self::default_package_manager(server_type)),
        }
    }

    /// Create reference mode with stdio transport.
    pub fn reference_stdio() -> Self {
        Self::Reference {
            transport: McpbTransport::Stdio,
        }
    }

    /// Create reference mode with HTTP transport.
    pub fn reference_http() -> Self {
        Self::Reference {
            transport: McpbTransport::Http,
        }
    }

    /// Get the default package manager for a server type.
    fn default_package_manager(server_type: McpbServerType) -> Option<PackageManager> {
        match server_type {
            McpbServerType::Node => Some(PackageManager::Node(NodePackageManager::default())),
            McpbServerType::Python => Some(PackageManager::Python(PythonPackageManager::default())),
            McpbServerType::Binary => None,
        }
    }

    /// Check if this is reference mode.
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::Reference { .. })
    }

    /// Check if this uses HTTP transport.
    pub fn is_http(&self) -> bool {
        match self {
            Self::Bundle { transport, .. } | Self::Reference { transport } => {
                *transport == McpbTransport::Http
            }
        }
    }

    /// Get the server type if in bundle mode.
    pub fn server_type(&self) -> Option<McpbServerType> {
        match self {
            Self::Bundle { server_type, .. } => Some(*server_type),
            Self::Reference { .. } => None,
        }
    }

    /// Get the transport type.
    pub fn transport(&self) -> McpbTransport {
        match self {
            Self::Bundle { transport, .. } | Self::Reference { transport } => *transport,
        }
    }

    /// Get the package manager if in bundle mode.
    pub fn package_manager(&self) -> Option<PackageManager> {
        match self {
            Self::Bundle {
                package_manager, ..
            } => *package_manager,
            Self::Reference { .. } => None,
        }
    }

    /// Get the Node.js package manager if applicable.
    pub fn node_package_manager(&self) -> Option<NodePackageManager> {
        match self.package_manager() {
            Some(PackageManager::Node(pm)) => Some(pm),
            _ => None,
        }
    }

    /// Get the Python package manager if applicable.
    pub fn python_package_manager(&self) -> Option<PythonPackageManager> {
        match self.package_manager() {
            Some(PackageManager::Python(pm)) => Some(pm),
            _ => None,
        }
    }
}
