//! Validation tests.

use super::codes::{ErrorCode, ValidationCode};
use super::validators::fields::is_valid_package_name;
use super::validators::validate_manifest;
use tempfile::TempDir;

#[test]
fn test_valid_package_name() {
    // Valid
    assert!(is_valid_package_name("my-tool"));
    assert!(is_valid_package_name("tool123"));
    assert!(is_valid_package_name("abc"));
    assert!(is_valid_package_name(&"a".repeat(64)));

    // Invalid - too short
    assert!(!is_valid_package_name("ab"));
    assert!(!is_valid_package_name("a"));

    // Invalid - too long
    assert!(!is_valid_package_name(&"a".repeat(65)));

    // Invalid - empty
    assert!(!is_valid_package_name(""));

    // Invalid - uppercase
    assert!(!is_valid_package_name("My-Tool"));
    assert!(!is_valid_package_name("TOOL"));

    // Invalid - starts with digit
    assert!(!is_valid_package_name("123tool"));

    // Invalid - starts with hyphen
    assert!(!is_valid_package_name("-tool"));

    // Invalid - underscore
    assert!(!is_valid_package_name("tool_name"));

    // Invalid - special chars
    assert!(!is_valid_package_name("tool@name"));
    assert!(!is_valid_package_name("tool.name"));
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
