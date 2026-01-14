//! Shared utilities for project detection.

use grep_regex::RegexMatcher;
use grep_searcher::Searcher;
use grep_searcher::sinks::UTF8;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// A match found by grep.
#[derive(Debug, Clone)]
pub struct GrepMatch {
    /// Path to the file containing the match.
    pub path: PathBuf,
    /// Line number (1-based).
    pub line_number: u64,
    /// The matching line content.
    pub line: String,
}

/// Options for grep operations.
#[derive(Debug, Clone)]
pub struct GrepOptions {
    /// File extensions to include (e.g., ["js", "ts"]).
    pub extensions: Vec<String>,
    /// Maximum depth to traverse.
    pub max_depth: Option<usize>,
    /// Respect .gitignore.
    pub respect_gitignore: bool,
    /// Stop after first match.
    pub first_match_only: bool,
}

impl Default for GrepOptions {
    fn default() -> Self {
        Self {
            extensions: vec![],
            max_depth: None,
            respect_gitignore: true,
            first_match_only: false,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Search for a regex pattern in files under a directory.
pub fn grep_dir(dir: &Path, pattern: &str, options: &GrepOptions) -> Vec<GrepMatch> {
    let matcher = match RegexMatcher::new(pattern) {
        Ok(m) => m,
        Err(_) => return vec![],
    };

    let mut matches = Vec::new();
    let mut searcher = Searcher::new();
    let walker = build_walker(dir, options);

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Skip directories
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }

        let path = entry.path();

        // Filter by extension if specified
        if !options.extensions.is_empty() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !options.extensions.iter().any(|e| e == ext) {
                continue;
            }
        }

        let path_buf = path.to_path_buf();
        let mut file_matches = Vec::new();

        let result = searcher.search_path(
            &matcher,
            path,
            UTF8(|line_number, line| {
                file_matches.push(GrepMatch {
                    path: path_buf.clone(),
                    line_number,
                    line: line.trim_end().to_string(),
                });

                // Continue searching unless first_match_only and we found one
                Ok(!options.first_match_only || file_matches.is_empty())
            }),
        );

        if result.is_ok() {
            matches.extend(file_matches);

            if options.first_match_only && !matches.is_empty() {
                break;
            }
        }
    }

    matches
}

/// Check if any file matches the pattern.
pub fn has_pattern(dir: &Path, pattern: &str, extensions: &[&str]) -> bool {
    let options = GrepOptions {
        extensions: extensions.iter().map(|s| s.to_string()).collect(),
        first_match_only: true,
        respect_gitignore: true,
        ..Default::default()
    };

    !grep_dir(dir, pattern, &options).is_empty()
}

/// Check if any of multiple patterns match, returning the first matching pattern.
pub fn has_any_pattern(dir: &Path, patterns: &[&str], extensions: &[&str]) -> Option<String> {
    for pattern in patterns {
        if has_pattern(dir, pattern, extensions) {
            return Some(pattern.to_string());
        }
    }
    None
}

/// Find first existing file and return its relative path.
pub fn find_first_relative(dir: &Path, paths: &[&str]) -> Option<String> {
    for path in paths {
        let full_path = dir.join(path);
        if full_path.exists() && full_path.is_file() {
            return Some(path.to_string());
        }
    }
    None
}

/// Read and parse JSON file.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read and parse TOML file.
pub fn read_toml<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

/// Parse .env.example or .env.template file and extract environment variables.
pub fn parse_env_example(dir: &Path) -> Vec<super::EnvVar> {
    use super::EnvVar;

    // Try .env.example first, then .env.template
    let env_file = dir.join(".env.example");
    let env_file = if env_file.exists() {
        env_file
    } else {
        let template = dir.join(".env.template");
        if template.exists() {
            template
        } else {
            return vec![];
        }
    };

    let content = match std::fs::read_to_string(&env_file) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut vars = Vec::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse KEY=VALUE or KEY=
        let (name, default) = if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let val = line[eq_pos + 1..].trim();
            let default = if val.is_empty() {
                None
            } else {
                Some(val.to_string())
            };
            (key, default)
        } else {
            // Line without '=' - just the key
            (line.to_string(), None)
        };

        // Skip invalid names
        if name.is_empty() || !name.chars().next().unwrap().is_ascii_alphabetic() {
            continue;
        }

        // Classify config type and value type
        let (config_type, value_type) = classify_env_var(&name, default.as_deref());

        // Detect sensitive values
        let sensitive = is_sensitive_env(&name);

        vars.push(EnvVar {
            name,
            default,
            sensitive,
            config_type,
            value_type,
        });
    }

    vars
}

/// Classify an env var as system_config or user_config and infer value type.
fn classify_env_var(
    name: &str,
    default: Option<&str>,
) -> (super::EnvConfigType, super::EnvValueType) {
    use super::{EnvConfigType, EnvValueType};

    let name_upper = name.to_uppercase();

    // System config: PORT and HOST patterns only
    if name_upper == "PORT" || name_upper.ends_with("_PORT") {
        return (EnvConfigType::System, EnvValueType::Port);
    }

    if name_upper == "HOST"
        || name_upper == "HOSTNAME"
        || name_upper == "BIND_ADDRESS"
        || name_upper.ends_with("_HOST")
    {
        return (EnvConfigType::System, EnvValueType::Hostname);
    }

    // User config: infer type from default value
    let value_type = if let Some(val) = default {
        if val == "true" || val == "false" {
            EnvValueType::Boolean
        } else if val.parse::<i64>().is_ok() || val.parse::<f64>().is_ok() {
            EnvValueType::Number
        } else {
            EnvValueType::String
        }
    } else {
        EnvValueType::String
    };

    (EnvConfigType::User, value_type)
}

/// Check if env var name suggests a sensitive value.
fn is_sensitive_env(name: &str) -> bool {
    let name_upper = name.to_uppercase();
    name_upper.contains("KEY")
        || name_upper.contains("SECRET")
        || name_upper.contains("TOKEN")
        || name_upper.contains("PASSWORD")
        || name_upper.contains("CREDENTIAL")
        || name_upper.contains("URL")
}

/// Build a directory walker with options.
fn build_walker(dir: &Path, options: &GrepOptions) -> ignore::Walk {
    let mut builder = WalkBuilder::new(dir);

    builder
        .hidden(false) // Don't skip hidden files by default
        .git_ignore(options.respect_gitignore)
        .git_global(false)
        .git_exclude(false);

    if let Some(depth) = options.max_depth {
        builder.max_depth(Some(depth));
    }

    builder.build()
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
    fn test_grep_simple_pattern() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.js");
        fs::write(&file, "import { StdioServerTransport } from '@mcp/sdk';").unwrap();

        let matches = grep_dir(
            tmp.path(),
            "StdioServerTransport",
            &GrepOptions {
                extensions: vec!["js".into()],
                ..Default::default()
            },
        );

        assert_eq!(matches.len(), 1);
        assert!(matches[0].line.contains("StdioServerTransport"));
    }

    #[test]
    fn test_grep_no_match() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("test.js");
        fs::write(&file, "console.log('hello');").unwrap();

        let matches = grep_dir(
            tmp.path(),
            "StdioServerTransport",
            &GrepOptions {
                extensions: vec!["js".into()],
                ..Default::default()
            },
        );

        assert!(matches.is_empty());
    }

    #[test]
    fn test_has_pattern() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("server.py");
        fs::write(
            &file,
            "from mcp.server.fastmcp import FastMCP\nmcp = FastMCP()",
        )
        .unwrap();

        assert!(has_pattern(tmp.path(), "FastMCP", &["py"]));
        assert!(!has_pattern(tmp.path(), "NonExistent", &["py"]));
    }

    #[test]
    fn test_has_any_pattern() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("index.js");
        fs::write(&file, "const transport = new StdioServerTransport();").unwrap();

        let result = has_any_pattern(
            tmp.path(),
            &["StreamableHTTPServerTransport", "StdioServerTransport"],
            &["js"],
        );

        assert_eq!(result, Some("StdioServerTransport".to_string()));
    }

    #[test]
    fn test_extension_filter() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("test.js"), "FastMCP").unwrap();
        fs::write(tmp.path().join("test.py"), "FastMCP").unwrap();

        // Should only find in .js files
        let matches = grep_dir(
            tmp.path(),
            "FastMCP",
            &GrepOptions {
                extensions: vec!["js".into()],
                ..Default::default()
            },
        );

        assert_eq!(matches.len(), 1);
        assert!(matches[0].path.to_string_lossy().ends_with(".js"));
    }

    #[test]
    fn test_first_match_only() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("a.js"), "pattern\npattern").unwrap();
        fs::write(tmp.path().join("b.js"), "pattern").unwrap();

        let matches = grep_dir(
            tmp.path(),
            "pattern",
            &GrepOptions {
                extensions: vec!["js".into()],
                first_match_only: true,
                ..Default::default()
            },
        );

        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_parse_env_example_basic() {
        use super::super::{EnvConfigType, EnvValueType};

        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(".env.example"),
            "API_KEY=\nDEBUG=false\nPORT=3000\nHOST=127.0.0.1\n",
        )
        .unwrap();

        let vars = parse_env_example(tmp.path());
        assert_eq!(vars.len(), 4);

        // API_KEY - user config, sensitive
        let api_key = vars.iter().find(|v| v.name == "API_KEY").unwrap();
        assert!(api_key.sensitive);
        assert_eq!(api_key.config_type, EnvConfigType::User);
        assert!(api_key.default.is_none());

        // DEBUG - user config, boolean
        let debug = vars.iter().find(|v| v.name == "DEBUG").unwrap();
        assert!(!debug.sensitive);
        assert_eq!(debug.config_type, EnvConfigType::User);
        assert_eq!(debug.value_type, EnvValueType::Boolean);
        assert_eq!(debug.default, Some("false".to_string()));

        // PORT - system config
        let port = vars.iter().find(|v| v.name == "PORT").unwrap();
        assert_eq!(port.config_type, EnvConfigType::System);
        assert_eq!(port.value_type, EnvValueType::Port);
        assert_eq!(port.default, Some("3000".to_string()));

        // HOST - system config
        let host = vars.iter().find(|v| v.name == "HOST").unwrap();
        assert_eq!(host.config_type, EnvConfigType::System);
        assert_eq!(host.value_type, EnvValueType::Hostname);
    }

    #[test]
    fn test_parse_env_example_skips_comments() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(".env.example"),
            "# This is a comment\nAPI_KEY=secret\n# Another comment\n",
        )
        .unwrap();

        let vars = parse_env_example(tmp.path());
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "API_KEY");
    }

    #[test]
    fn test_parse_env_example_sensitive_detection() {
        use super::super::EnvConfigType;

        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(".env.example"),
            "DATABASE_URL=\nAWS_SECRET_KEY=\nAUTH_TOKEN=\nUSER_PASSWORD=\nAPP_NAME=\n",
        )
        .unwrap();

        let vars = parse_env_example(tmp.path());

        // These should be sensitive
        assert!(
            vars.iter()
                .find(|v| v.name == "DATABASE_URL")
                .unwrap()
                .sensitive
        );
        assert!(
            vars.iter()
                .find(|v| v.name == "AWS_SECRET_KEY")
                .unwrap()
                .sensitive
        );
        assert!(
            vars.iter()
                .find(|v| v.name == "AUTH_TOKEN")
                .unwrap()
                .sensitive
        );
        assert!(
            vars.iter()
                .find(|v| v.name == "USER_PASSWORD")
                .unwrap()
                .sensitive
        );

        // APP_NAME should NOT be sensitive
        assert!(
            !vars
                .iter()
                .find(|v| v.name == "APP_NAME")
                .unwrap()
                .sensitive
        );

        // All should be user config (not PORT/HOST)
        for var in &vars {
            assert_eq!(var.config_type, EnvConfigType::User);
        }
    }

    #[test]
    fn test_parse_env_example_falls_back_to_template() {
        let tmp = TempDir::new().unwrap();
        // No .env.example, but .env.template exists
        fs::write(tmp.path().join(".env.template"), "MY_VAR=value\n").unwrap();

        let vars = parse_env_example(tmp.path());
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "MY_VAR");
    }

    #[test]
    fn test_parse_env_example_empty_if_no_file() {
        let tmp = TempDir::new().unwrap();
        let vars = parse_env_example(tmp.path());
        assert!(vars.is_empty());
    }

    #[test]
    fn test_parse_env_example_system_config_patterns() {
        use super::super::{EnvConfigType, EnvValueType};

        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(".env.example"),
            "PORT=8080\nSERVER_PORT=3000\nHOST=localhost\nHOSTNAME=example.com\nBIND_ADDRESS=0.0.0.0\nDB_HOST=db.local\n",
        )
        .unwrap();

        let vars = parse_env_example(tmp.path());

        // All PORT patterns -> system config, port type
        let port = vars.iter().find(|v| v.name == "PORT").unwrap();
        assert_eq!(port.config_type, EnvConfigType::System);
        assert_eq!(port.value_type, EnvValueType::Port);

        let server_port = vars.iter().find(|v| v.name == "SERVER_PORT").unwrap();
        assert_eq!(server_port.config_type, EnvConfigType::System);
        assert_eq!(server_port.value_type, EnvValueType::Port);

        // All HOST patterns -> system config, hostname type
        let host = vars.iter().find(|v| v.name == "HOST").unwrap();
        assert_eq!(host.config_type, EnvConfigType::System);
        assert_eq!(host.value_type, EnvValueType::Hostname);

        let hostname = vars.iter().find(|v| v.name == "HOSTNAME").unwrap();
        assert_eq!(hostname.config_type, EnvConfigType::System);

        let bind_addr = vars.iter().find(|v| v.name == "BIND_ADDRESS").unwrap();
        assert_eq!(bind_addr.config_type, EnvConfigType::System);

        let db_host = vars.iter().find(|v| v.name == "DB_HOST").unwrap();
        assert_eq!(db_host.config_type, EnvConfigType::System);
    }
}
