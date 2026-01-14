//! Validation tests.

use super::codes::{ErrorCode, ValidationCode};
use super::validators::validate_manifest;
use tempfile::TempDir;

fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[test]
fn test_valid_package_name() {
    assert!(is_valid_package_name("my-tool"));
    assert!(is_valid_package_name("tool123"));
    assert!(is_valid_package_name("a"));
    assert!(!is_valid_package_name(""));
    assert!(!is_valid_package_name("My-Tool"));
    assert!(!is_valid_package_name("123tool"));
    assert!(!is_valid_package_name("-tool"));
    assert!(!is_valid_package_name("tool_name"));
}

#[test]
fn test_missing_manifest() {
    let dir = TempDir::new().unwrap();
    let result = validate_manifest(dir.path());
    assert!(!result.is_valid());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(
        result.errors[0].code,
        ValidationCode::Error(ErrorCode::ManifestNotFound)
    );
}

#[test]
fn test_invalid_json() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("manifest.json"), "{ invalid json }").unwrap();
    let result = validate_manifest(dir.path());
    assert!(!result.is_valid());
    assert_eq!(
        result.errors[0].code,
        ValidationCode::Error(ErrorCode::InvalidJson)
    );
}

#[test]
fn test_missing_required_fields() {
    let dir = TempDir::new().unwrap();
    let manifest = r#"{
        "manifest_version": "0.3",
        "server": { "type": "node" }
    }"#;
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let result = validate_manifest(dir.path());
    assert!(!result.is_valid());
    // Should have errors for: name, version, description, author, entry_point, mcp_config
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationCode::Error(ErrorCode::MissingRequiredField))
    );
}

#[test]
fn test_invalid_version() {
    let dir = TempDir::new().unwrap();
    let manifest = r#"{
        "manifest_version": "0.3",
        "name": "my-tool",
        "version": "not-semver",
        "description": "A tool",
        "author": { "name": "Test" },
        "server": {
            "type": "node",
            "entry_point": "server/index.js",
            "mcp_config": { "command": "node", "args": [] }
        }
    }"#;
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let result = validate_manifest(dir.path());
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationCode::Error(ErrorCode::InvalidVersion))
    );
}

#[test]
fn test_invalid_name() {
    let dir = TempDir::new().unwrap();
    let manifest = r#"{
        "manifest_version": "0.3",
        "name": "MyTool",
        "version": "1.0.0",
        "description": "A tool",
        "author": { "name": "Test" },
        "server": {
            "type": "node",
            "entry_point": "server/index.js",
            "mcp_config": { "command": "node", "args": [] }
        }
    }"#;
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let result = validate_manifest(dir.path());
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationCode::Error(ErrorCode::InvalidPackageName))
    );
}

#[test]
fn test_missing_entry_point_file() {
    let dir = TempDir::new().unwrap();
    let manifest = r#"{
        "manifest_version": "0.3",
        "name": "my-tool",
        "version": "1.0.0",
        "description": "A tool",
        "author": { "name": "Test" },
        "server": {
            "type": "node",
            "entry_point": "server/index.js",
            "mcp_config": { "command": "node", "args": [] }
        }
    }"#;
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let result = validate_manifest(dir.path());
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == ValidationCode::Error(ErrorCode::FileNotFound))
    );
}

#[test]
fn test_valid_manifest_with_warnings() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join("server")).unwrap();
    std::fs::write(dir.path().join("server/index.js"), "// entry").unwrap();

    let manifest = r#"{
        "manifest_version": "0.3",
        "name": "my-tool",
        "version": "1.0.0",
        "description": "A tool",
        "author": { "name": "Test" },
        "server": {
            "type": "node",
            "entry_point": "server/index.js",
            "mcp_config": { "command": "node", "args": [] }
        }
    }"#;
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let result = validate_manifest(dir.path());

    // Should be valid (no errors)
    assert!(result.is_valid());
    // But should have warnings for missing: author.email, license, icon, node_modules
    assert!(!result.warnings.is_empty());
}
