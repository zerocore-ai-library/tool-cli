//! Security utilities for credential encryption and storage.

use crate::constants::DEFAULT_CREDENTIALS_PATH;
use aes_gcm::Aes256Gcm;
use aes_gcm::aead::{AeadInPlace, KeyInit, OsRng};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rand::RngCore;
use rmcp::transport::auth::{AuthError, CredentialStore, StoredCredentials};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;
use zeroize::Zeroize;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Environment variable for the credential encryption key.
pub const CREDENTIALS_SECRET_KEY_ENV: &str = "CREDENTIALS_SECRET_KEY";

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Error type for credential crypto operations.
#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("Invalid nonce length: expected 12, got {0}")]
    InvalidNonceLength(usize),

    #[error("Invalid auth tag length: expected 16, got {0}")]
    InvalidAuthTagLength(usize),
}

/// Error type for secret provider operations.
#[derive(Debug, Error)]
pub enum SecretProviderError {
    #[error("Secret not found: {0}")]
    NotFound(String),

    #[error("Invalid key format: {0}")]
    InvalidFormat(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Secret key with ID and material.
#[derive(Clone, Debug)]
pub struct SecretKey {
    pub key_id: String,
    pub key_material: [u8; 32],
}

/// Environment-based secret provider.
pub struct EnvSecretProvider {
    key_id: String,
    key_material: [u8; 32],
}

/// Handles AES-256-GCM encryption/decryption for credential storage.
#[derive(Clone)]
pub struct CredentialCrypto {
    cipher: Aes256Gcm,
    key_id: String,
}

/// Result of encrypting a secret.
pub struct EncryptedSecret {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub auth_tag: Vec<u8>,
    pub key_id: String,
}

/// File-based credential store that encrypts OAuth tokens at rest.
pub struct FileCredentialStore {
    tool_ref: String,
    crypto: CredentialCrypto,
}

/// Encrypted credential envelope stored on disk.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialEnvelope {
    /// Tool reference (e.g., "asana" or "namespace/tool")
    tool: String,
    /// Provider type (always "oauth" for now)
    provider: String,
    /// Key ID used for encryption
    key_id: String,
    /// AES-GCM nonce (12 bytes)
    #[serde(with = "base64_bytes")]
    nonce: Vec<u8>,
    /// AES-GCM authentication tag (16 bytes)
    #[serde(with = "base64_bytes")]
    auth_tag: Vec<u8>,
    /// Encrypted StoredCredentials JSON
    #[serde(with = "base64_bytes")]
    ciphertext: Vec<u8>,
    /// When the credential was first created
    created_at: chrono::DateTime<chrono::Utc>,
    /// When the credential was last updated
    updated_at: chrono::DateTime<chrono::Utc>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl EnvSecretProvider {
    /// Create a new environment-based secret provider.
    pub fn new() -> Result<Self, SecretProviderError> {
        let key_material_str = std::env::var(CREDENTIALS_SECRET_KEY_ENV)
            .map_err(|_| SecretProviderError::NotFound(CREDENTIALS_SECRET_KEY_ENV.to_string()))?;

        let key_id = std::env::var("CREDENTIALS_KEY_ID").unwrap_or_else(|_| "default".to_string());

        let key_material = decode_key_material(&key_material_str)?;
        let provider = Self {
            key_id,
            key_material,
        };

        Ok(provider)
    }

    /// Get the encryption key.
    pub async fn get_encryption_key(
        &self,
        _key_id: &str,
    ) -> Result<SecretKey, SecretProviderError> {
        Ok(SecretKey {
            key_id: self.key_id.clone(),
            key_material: self.key_material,
        })
    }
}

impl Drop for EnvSecretProvider {
    fn drop(&mut self) {
        self.key_material.zeroize();
    }
}

impl CredentialCrypto {
    /// Create a new CredentialCrypto instance from a secret key.
    pub fn new(key: &SecretKey) -> Self {
        let cipher = Aes256Gcm::new((&key.key_material).into());

        Self {
            cipher,
            key_id: key.key_id.clone(),
        }
    }

    /// Encrypt a JSON payload using AES-256-GCM.
    pub fn encrypt(&self, payload: &Value) -> Result<EncryptedSecret, CryptoError> {
        let mut buffer = serde_json::to_vec(payload)?;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);

        let tag = self
            .cipher
            .encrypt_in_place_detached((&nonce_bytes).into(), &[], &mut buffer)
            .map_err(|err| CryptoError::Encryption(err.to_string()))?;

        Ok(EncryptedSecret {
            ciphertext: buffer,
            nonce: nonce_bytes.to_vec(),
            auth_tag: tag.to_vec(),
            key_id: self.key_id.clone(),
        })
    }

    /// Decrypt ciphertext using AES-256-GCM.
    pub fn decrypt(
        &self,
        ciphertext: &[u8],
        nonce: &[u8],
        auth_tag: &[u8],
    ) -> Result<Value, CryptoError> {
        if nonce.len() != 12 {
            return Err(CryptoError::InvalidNonceLength(nonce.len()));
        }
        if auth_tag.len() != 16 {
            return Err(CryptoError::InvalidAuthTagLength(auth_tag.len()));
        }

        let mut buffer = ciphertext.to_vec();

        self.cipher
            .decrypt_in_place_detached(nonce.into(), &[], &mut buffer, auth_tag.into())
            .map_err(|err| CryptoError::Decryption(err.to_string()))?;

        let value = serde_json::from_slice(&buffer)?;

        buffer.zeroize();

        Ok(value)
    }
}

impl FileCredentialStore {
    /// Create a new file-based credential store.
    pub fn new(tool_ref: &str, crypto: CredentialCrypto) -> Self {
        Self {
            tool_ref: tool_ref.to_string(),
            crypto,
        }
    }

    /// Get the path where credentials are stored.
    pub fn credential_path(&self) -> PathBuf {
        // Handle namespaced references: "namespace/name" -> "namespace/name/enc.json"
        if self.tool_ref.contains('/') {
            let parts: Vec<&str> = self.tool_ref.splitn(2, '/').collect();
            DEFAULT_CREDENTIALS_PATH
                .join(parts[0])
                .join(parts[1])
                .join("enc.json")
        } else {
            DEFAULT_CREDENTIALS_PATH
                .join(&self.tool_ref)
                .join("enc.json")
        }
    }
}

#[async_trait]
impl CredentialStore for FileCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let path = self.credential_path();

        if !path.exists() {
            return Ok(None);
        }

        let contents = tokio::fs::read_to_string(&path).await.map_err(|e| {
            AuthError::InternalError(format!("Failed to read credential file: {}", e))
        })?;

        let envelope: CredentialEnvelope = serde_json::from_str(&contents).map_err(|e| {
            AuthError::InternalError(format!("Failed to parse credential envelope: {}", e))
        })?;

        let decrypted = self
            .crypto
            .decrypt(&envelope.ciphertext, &envelope.nonce, &envelope.auth_tag)
            .map_err(|e| AuthError::InternalError(e.to_string()))?;

        let creds: StoredCredentials = serde_json::from_value(decrypted)
            .map_err(|e| AuthError::InternalError(format!("Failed to parse credentials: {}", e)))?;

        Ok(Some(creds))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let path = self.credential_path();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AuthError::InternalError(format!("Failed to create credential directory: {}", e))
            })?;
        }

        // Check if we're updating an existing credential
        let created_at = if path.exists() {
            match tokio::fs::read_to_string(&path).await {
                Ok(contents) => serde_json::from_str::<CredentialEnvelope>(&contents)
                    .map(|e| e.created_at)
                    .unwrap_or_else(|_| chrono::Utc::now()),
                Err(_) => chrono::Utc::now(),
            }
        } else {
            chrono::Utc::now()
        };

        let payload = serde_json::to_value(&credentials).map_err(|e| {
            AuthError::InternalError(format!("Failed to serialize credentials: {}", e))
        })?;

        let encrypted = self
            .crypto
            .encrypt(&payload)
            .map_err(|e| AuthError::InternalError(e.to_string()))?;

        let envelope = CredentialEnvelope {
            tool: self.tool_ref.clone(),
            provider: "oauth".to_string(),
            key_id: encrypted.key_id,
            nonce: encrypted.nonce,
            auth_tag: encrypted.auth_tag,
            ciphertext: encrypted.ciphertext,
            created_at,
            updated_at: chrono::Utc::now(),
        };

        let contents = serde_json::to_string_pretty(&envelope).map_err(|e| {
            AuthError::InternalError(format!("Failed to serialize envelope: {}", e))
        })?;

        tokio::fs::write(&path, contents).await.map_err(|e| {
            AuthError::InternalError(format!("Failed to write credential file: {}", e))
        })?;

        Ok(())
    }

    async fn clear(&self) -> Result<(), AuthError> {
        let path = self.credential_path();

        if path.exists() {
            tokio::fs::remove_file(&path).await.map_err(|e| {
                AuthError::InternalError(format!("Failed to remove credential file: {}", e))
            })?;
        }

        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Check if we're running in an interactive terminal.
pub fn is_interactive() -> bool {
    atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout)
}

/// Get credential crypto from environment.
///
/// Returns None if CREDENTIALS_SECRET_KEY is not set.
pub fn get_credential_crypto() -> Option<CredentialCrypto> {
    EnvSecretProvider::new().ok().and_then(|provider| {
        // EnvSecretProvider stores the key directly
        let key = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(provider.get_encryption_key("default"))
        });
        key.ok().map(|k| CredentialCrypto::new(&k))
    })
}

fn decode_key_material(key_material: &str) -> Result<[u8; 32], SecretProviderError> {
    let cleaned = key_material.trim();
    let decoded = BASE64
        .decode(cleaned)
        .map_err(|e| SecretProviderError::InvalidFormat(format!("Base64 decode failed: {}", e)))?;

    if decoded.len() != 32 {
        return Err(SecretProviderError::InvalidFormat(format!(
            "Key must be exactly 32 bytes, got {}",
            decoded.len()
        )));
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

//--------------------------------------------------------------------------------------------------
// Modules
//--------------------------------------------------------------------------------------------------

/// Serde helper for base64 encoding/decoding of byte vectors.
mod base64_bytes {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}
