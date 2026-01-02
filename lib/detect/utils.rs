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
}
