//! Registry client for tool.store.

use crate::constants::{get_registry_url, REGISTRY_TOKEN_ENV};
use crate::error::{ToolError, ToolResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// API version prefix.
const API_PREFIX: &str = "/api/v1";

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Client for the tool registry.
#[derive(Debug, Clone)]
pub struct RegistryClient {
    /// Registry URL.
    url: String,

    /// Optional authentication token.
    auth_token: Option<String>,

    /// HTTP client.
    http: Client,
}

/// User info returned from auth validation.
#[derive(Debug, Clone, Deserialize)]
pub struct UserInfoResponse {
    /// Username.
    pub username: String,
    /// Email address.
    pub email: Option<String>,
    /// Display name.
    pub display_name: Option<String>,
}

/// Search result from the registry.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    /// Artifact namespace.
    pub namespace: String,
    /// Artifact name.
    pub name: String,
    /// Short description.
    pub description: Option<String>,
    /// Latest version.
    pub latest_version: Option<String>,
    /// Total download count.
    pub total_downloads: i64,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<SearchResultItem>,
}

#[derive(Debug, Deserialize)]
struct SearchResultItem {
    artifact: ArtifactSummary,
}

#[derive(Debug, Deserialize)]
struct ArtifactSummary {
    namespace: String,
    name: String,
    description: Option<String>,
    #[serde(default)]
    total_downloads: i64,
    latest_version: Option<String>,
}

/// Artifact details from the registry.
#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactDetails {
    /// Artifact namespace.
    pub namespace: String,
    /// Artifact name.
    pub name: String,
    /// Short description.
    pub description: Option<String>,
    /// Latest version info.
    pub latest_version: Option<VersionInfo>,
    /// Total download count.
    #[serde(default)]
    pub total_downloads: i64,
}

/// Version info from the registry.
#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    /// Version string.
    pub version: String,
    /// Bundle size in bytes.
    pub bundle_size: Option<u64>,
    /// Bundle checksum.
    pub bundle_checksum: Option<String>,
}

/// Upload initiation response.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadInitResponse {
    /// Upload ID to reference when publishing.
    pub upload_id: String,
    /// Presigned URL for uploading the bundle.
    pub upload_url: String,
}

/// Publish result.
#[derive(Debug, Clone, Deserialize)]
pub struct PublishResult {
    /// The published version.
    pub version: String,
    /// CDN URL for the bundle.
    pub bundle_url: String,
}

#[derive(Debug, Serialize)]
struct UploadInitRequest {
    version: String,
    bundle_size: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct PublishVersionRequest {
    upload_id: String,
    version: String,
    manifest: serde_json::Value,
    description: Option<String>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl RegistryClient {
    /// Create a new registry client with default configuration.
    pub fn new() -> Self {
        let url = get_registry_url();
        let auth_token = std::env::var(REGISTRY_TOKEN_ENV).ok();

        Self {
            url,
            auth_token,
            http: Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Set the registry URL.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    /// Set the authentication token.
    pub fn with_auth_token(mut self, token: impl Into<String>) -> Self {
        self.auth_token = Some(token.into());
        self
    }

    /// Get the registry URL.
    pub fn registry_url(&self) -> &str {
        &self.url
    }

    /// Check if authentication is configured.
    pub fn has_auth(&self) -> bool {
        self.auth_token.is_some()
    }

    /// Validate the auth token and return user info.
    pub async fn validate_token(&self) -> ToolResult<UserInfoResponse> {
        let token = self
            .auth_token
            .as_ref()
            .ok_or_else(|| ToolError::Generic("No auth token configured".into()))?;

        let url = format!("{}{}/auth/me", self.url, API_PREFIX);

        let response = self
            .http
            .get(&url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to validate token: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Token validation failed ({}): {}",
                status, body
            )));
        }

        response
            .json::<UserInfoResponse>()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to parse user info: {}", e)))
    }

    /// Get artifact details from the registry.
    pub async fn get_artifact(
        &self,
        namespace: &str,
        name: &str,
    ) -> ToolResult<ArtifactDetails> {
        let url = format!(
            "{}{}/artifacts/{}/{}",
            self.url, API_PREFIX, namespace, name
        );

        let mut request = self.http.get(&url);
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to fetch artifact: {}", e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ToolError::Generic(format!(
                "Tool {}/{} not found in registry",
                namespace, name
            )));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Failed to fetch artifact ({}): {}",
                status, body
            )));
        }

        response
            .json::<ArtifactDetails>()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to parse artifact details: {}", e)))
    }

    /// List all versions of an artifact.
    pub async fn list_versions(
        &self,
        namespace: &str,
        name: &str,
    ) -> ToolResult<Vec<VersionInfo>> {
        let url = format!(
            "{}{}/artifacts/{}/{}/versions",
            self.url, API_PREFIX, namespace, name
        );

        let mut request = self.http.get(&url);
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to list versions: {}", e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Vec::new());
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Failed to list versions ({}): {}",
                status, body
            )));
        }

        // The API returns { data: [...] }
        #[derive(serde::Deserialize)]
        struct VersionsResponse {
            data: Vec<VersionInfo>,
        }

        let versions_response: VersionsResponse = response
            .json()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to parse versions: {}", e)))?;

        Ok(versions_response.data)
    }

    /// Get the download URL for an artifact version.
    pub fn get_download_url(&self, namespace: &str, name: &str, version: &str) -> String {
        format!(
            "{}{}/artifacts/{}/{}/versions/{}/download",
            self.url, API_PREFIX, namespace, name, version
        )
    }

    /// Download an artifact bundle to a file.
    pub async fn download_artifact(
        &self,
        namespace: &str,
        name: &str,
        version: &str,
        output_path: &Path,
    ) -> ToolResult<u64> {
        let url = self.get_download_url(namespace, name, version);

        let mut request = self.http.get(&url);
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Download failed: {}", e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ToolError::Generic(format!(
                "Version {} not found for {}/{}",
                version, namespace, name
            )));
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Download failed ({}): {}",
                status, body
            )));
        }

        // Stream the response to a file
        let mut file = tokio::fs::File::create(output_path)
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to create file: {}", e)))?;

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to read response: {}", e)))?;

        let size = bytes.len() as u64;

        file.write_all(&bytes)
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to write file: {}", e)))?;

        file.flush()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to flush file: {}", e)))?;

        Ok(size)
    }

    /// Check if an artifact exists in the registry.
    pub async fn artifact_exists(&self, namespace: &str, name: &str) -> ToolResult<bool> {
        let url = format!(
            "{}{}/artifacts/{}/{}",
            self.url, API_PREFIX, namespace, name
        );

        let mut request = self.http.get(&url);
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to check artifact: {}", e)))?;

        Ok(response.status().is_success())
    }

    /// Create a new artifact in the registry.
    pub async fn create_artifact(
        &self,
        namespace: &str,
        name: &str,
        description: Option<&str>,
    ) -> ToolResult<()> {
        let token = self
            .auth_token
            .as_ref()
            .ok_or_else(|| ToolError::Generic("Authentication required for publishing".into()))?;

        let url = format!("{}{}/artifacts", self.url, API_PREFIX);

        let body = serde_json::json!({
            "namespace": namespace,
            "name": name,
            "slug": name,
            "plugin_type": "tool",
            "description": description,
        });

        let response = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to create artifact: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Failed to create artifact ({}): {}",
                status, body
            )));
        }

        Ok(())
    }

    /// Initiate an upload for a new version.
    pub async fn init_upload(
        &self,
        namespace: &str,
        name: &str,
        version: &str,
        bundle_size: u64,
        sha256: &str,
    ) -> ToolResult<UploadInitResponse> {
        let token = self
            .auth_token
            .as_ref()
            .ok_or_else(|| ToolError::Generic("Authentication required for publishing".into()))?;

        let url = format!(
            "{}{}/artifacts/{}/{}/versions/upload",
            self.url, API_PREFIX, namespace, name
        );

        let body = UploadInitRequest {
            version: version.to_string(),
            bundle_size,
            sha256: sha256.to_string(),
        };

        let response = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to init upload: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Failed to init upload ({}): {}",
                status, body
            )));
        }

        response
            .json::<UploadInitResponse>()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to parse upload response: {}", e)))
    }

    /// Upload a bundle to the presigned URL.
    pub async fn upload_bundle(&self, upload_url: &str, content: &[u8]) -> ToolResult<()> {
        let response = self
            .http
            .put(upload_url)
            .header("Content-Type", "application/gzip")
            .body(content.to_vec())
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to upload bundle: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Failed to upload bundle ({}): {}",
                status, body
            )));
        }

        Ok(())
    }

    /// Publish a version after upload.
    pub async fn publish_version(
        &self,
        namespace: &str,
        name: &str,
        upload_id: &str,
        version: &str,
        manifest: serde_json::Value,
        description: Option<&str>,
    ) -> ToolResult<PublishResult> {
        let token = self
            .auth_token
            .as_ref()
            .ok_or_else(|| ToolError::Generic("Authentication required for publishing".into()))?;

        let url = format!(
            "{}{}/artifacts/{}/{}/versions",
            self.url, API_PREFIX, namespace, name
        );

        let body = PublishVersionRequest {
            upload_id: upload_id.to_string(),
            version: version.to_string(),
            manifest,
            description: description.map(String::from),
        };

        let response = self
            .http
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to publish version: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Failed to publish version ({}): {}",
                status, body
            )));
        }

        response
            .json::<PublishResult>()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to parse publish response: {}", e)))
    }

    /// Fetch a tool from the registry.
    ///
    /// Returns the bundle content and version if found.
    pub async fn fetch_tool(
        &self,
        plugin_ref: &crate::references::PluginRef,
    ) -> ToolResult<Option<(Vec<u8>, String)>> {
        // Must have namespace for remote fetch
        let namespace = match plugin_ref.namespace() {
            Some(ns) => ns,
            None => return Ok(None),
        };

        let name = plugin_ref.name();

        // Resolve version - find matching version or get latest
        let resolved_version = match plugin_ref.version() {
            Some(req) => {
                // Find latest version matching the requirement
                match self.get_matching_version(namespace, name, req).await? {
                    Some(v) => v,
                    None => return Ok(None),
                }
            }
            None => {
                // Get latest version
                match self.get_latest_version(namespace, name).await? {
                    Some(v) => v,
                    None => return Ok(None),
                }
            }
        };

        // Get download URL
        let download_url = format!(
            "{}{}/artifacts/{}/{}/versions/{}/download",
            self.url, API_PREFIX, namespace, name, resolved_version
        );

        let mut request = self.http.get(&download_url);
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to fetch tool: {}", e)))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Failed to download tool ({}): {}",
                status, body
            )));
        }

        let content = response
            .bytes()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to read tool content: {}", e)))?;

        Ok(Some((content.to_vec(), resolved_version)))
    }

    /// Get the latest version matching a requirement.
    async fn get_matching_version(
        &self,
        namespace: &str,
        name: &str,
        req: &semver::VersionReq,
    ) -> ToolResult<Option<String>> {
        let versions = self.list_versions(namespace, name).await?;
        for v in versions {
            if let Ok(version) = semver::Version::parse(&v.version) {
                if req.matches(&version) {
                    return Ok(Some(v.version));
                }
            }
        }
        Ok(None)
    }

    /// Get the latest version of a tool.
    async fn get_latest_version(&self, namespace: &str, name: &str) -> ToolResult<Option<String>> {
        let artifact = self.get_artifact(namespace, name).await?;
        Ok(artifact.latest_version.map(|v| v.version))
    }

    /// Search for tools in the registry.
    pub async fn search(&self, query: &str, limit: Option<usize>) -> ToolResult<Vec<SearchResult>> {
        let per_page = limit.unwrap_or(20);
        let url = format!(
            "{}{}/search?q={}&plugin_type=tool&page=1&per_page={}",
            self.url,
            API_PREFIX,
            urlencoding::encode(query),
            per_page
        );

        let mut request = self.http.get(&url);
        if let Some(token) = &self.auth_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ToolError::Generic(format!("Search failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ToolError::Generic(format!(
                "Search failed ({}): {}",
                status, body
            )));
        }

        let search_response: SearchResponse = response
            .json()
            .await
            .map_err(|e| ToolError::Generic(format!("Failed to parse search results: {}", e)))?;

        Ok(search_response
            .data
            .into_iter()
            .map(|item| SearchResult {
                namespace: item.artifact.namespace,
                name: item.artifact.name,
                description: item.artifact.description,
                latest_version: item.artifact.latest_version,
                total_downloads: item.artifact.total_downloads,
            })
            .collect())
    }
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations
//--------------------------------------------------------------------------------------------------

impl Default for RegistryClient {
    fn default() -> Self {
        Self::new()
    }
}
