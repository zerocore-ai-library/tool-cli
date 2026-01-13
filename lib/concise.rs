//! Concise output formatting for AI agents.
//!
//! This module provides utilities for formatting output in a machine-parseable,
//! minimal format suitable for AI agent consumption. Design principles:
//!
//! 1. **No decorations** - No emojis, colors, or tree-drawing characters
//! 2. **Minimal whitespace** - No extra blank lines, minimal indentation
//! 3. **Header + TSV format** - Column names on first line (prefixed with `#`), then tab-separated values
//! 4. **Quoted strings** - Fields that may contain spaces are double-quoted
//! 5. **Machine-parseable** - Easy to parse with `cut`, `awk`, or skip header line programmatically
//! 6. **Errors on stderr** - Success data on stdout, errors on stderr

use serde::Serialize;
use std::path::Path;

//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Output formatter that handles both normal and concise modes.
#[derive(Debug, Clone, Copy)]
pub struct Output {
    pub concise: bool,
    pub no_header: bool,
}

/// A tool entry for concise list output.
pub struct ConciseToolEntry<'a> {
    pub name: &'a str,
    pub tool_type: &'a str,
    pub description: Option<&'a str>,
    pub path: &'a Path,
}

/// A search result for concise output.
pub struct ConciseSearchResult<'a> {
    pub namespace: &'a str,
    pub name: &'a str,
    pub version: Option<&'a str>,
    pub description: Option<&'a str>,
    pub downloads: u64,
}

/// A tool schema entry for grep output.
pub struct ConciseGrepMatch<'a> {
    pub toolset: &'a str,
    pub tool_name: &'a str,
    pub field_type: &'a str, // "name", "description", "param", "output"
    pub field_name: &'a str,
    pub matched_text: &'a str,
}

/// A detection result for concise output.
pub struct ConciseDetectionResult<'a> {
    pub server_type: &'a str,
    pub transport: &'a str,
    pub entry_point: Option<&'a str>,
    pub confidence: f64,
    pub build_command: Option<&'a str>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl Output {
    /// Create a new Output formatter.
    pub fn new(concise: bool, no_header: bool) -> Self {
        Self { concise, no_header }
    }

    /// Print JSON in either pretty or minified format.
    pub fn json<T: Serialize>(&self, value: &T) {
        if self.concise {
            println!(
                "{}",
                serde_json::to_string(value).expect("Failed to serialize JSON")
            );
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(value).expect("Failed to serialize JSON")
            );
        }
    }

    /// Print a success message (only in normal mode).
    pub fn success(&self, message: &str) {
        if !self.concise {
            use colored::Colorize;
            println!("  {} {}", "✓".bright_green(), message);
        }
    }

    /// Print a progress message (only in normal mode).
    pub fn progress(&self, message: &str) {
        if !self.concise {
            use colored::Colorize;
            println!("  {} {}", "→".bright_blue(), message);
        }
    }

    /// Print an error message (only in normal mode).
    pub fn error(&self, message: &str) {
        if !self.concise {
            use colored::Colorize;
            println!("  {} {}", "✗".bright_red(), message);
        }
    }

    /// Print a blank line (only in normal mode).
    pub fn blank(&self) {
        if !self.concise {
            println!();
        }
    }

    /// Print a TSV header line (if headers are enabled).
    pub fn header(&self, columns: &[&str]) {
        if self.concise && !self.no_header {
            println!("#{}", columns.join("\t"));
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Quote a string if it contains spaces, tabs, quotes, or newlines.
/// Uses double quotes and escapes internal quotes/backslashes with backslash.
pub fn quote(s: &str) -> String {
    if s.contains(' ')
        || s.contains('\t')
        || s.contains('"')
        || s.contains('\n')
        || s.contains('\\')
    {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

/// Quote a string for TSV output (always quote for consistency in certain fields).
pub fn quote_always(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{}\"", escaped)
}

/// Format a list of tools for concise output (Header + TSV).
/// Columns: name, type, path
pub fn format_tool_list(entries: &[ConciseToolEntry], no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#name\ttype\tpath".to_string());
    }
    for e in entries {
        lines.push(format!(
            "{}\t{}\t{}",
            e.name,
            e.tool_type,
            quote(&e.path.display().to_string())
        ));
    }
    lines.join("\n")
}

/// Format search results for concise output (Header + TSV).
/// Columns: ref, description, downloads
pub fn format_search_results(results: &[ConciseSearchResult], no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#ref\tdescription\tdownloads".to_string());
    }
    for r in results {
        let ref_str = match r.version {
            Some(v) => format!("{}/{}@{}", r.namespace, r.name, v),
            None => format!("{}/{}", r.namespace, r.name),
        };
        let desc = r.description.unwrap_or("");
        lines.push(format!("{}\t{}\t{}", ref_str, quote(desc), r.downloads));
    }
    lines.join("\n")
}

/// Format tool info in concise function signature format (Header + TSV).
/// Columns: tool, params, outputs
pub fn format_tool_signature(
    toolset: &str,
    tool_name: &str,
    input_schema: &serde_json::Value,
    output_schema: Option<&serde_json::Value>,
) -> String {
    let params = format_schema_params(input_schema, true);
    let outputs = output_schema
        .map(|s| format_schema_params(s, false))
        .unwrap_or_default();

    format!(
        "{}:{}\t{}\t{}",
        toolset,
        tool_name,
        quote(&params),
        quote(&outputs)
    )
}

/// Format schema properties as param list.
fn format_schema_params(schema: &serde_json::Value, is_input: bool) -> String {
    let props = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return String::new(),
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let params: Vec<String> = props
        .iter()
        .map(|(name, prop)| {
            let type_str = prop.get("type").and_then(|t| t.as_str()).unwrap_or("any");
            let marker = if required.contains(&name.as_str()) {
                "*"
            } else {
                "?"
            };
            if is_input {
                format!("{}{}: {}", name, marker, type_str)
            } else {
                format!("{}: {}", name, type_str)
            }
        })
        .collect();

    params.join(", ")
}

/// Format grep matches for concise output (Header + TSV).
/// Columns: tool, match_type, text
pub fn format_grep_matches(matches: &[ConciseGrepMatch], no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#tool\tmatch_type\ttext".to_string());
    }
    for m in matches {
        lines.push(format!(
            "{}:{}\t{}:{}\t{}",
            m.toolset,
            m.tool_name,
            m.field_type,
            m.field_name,
            quote(m.matched_text)
        ));
    }
    lines.join("\n")
}

/// Format grep matches as list (tool names only, Header + TSV).
/// Columns: tool
pub fn format_grep_list(matches: &[ConciseGrepMatch], no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#tool".to_string());
    }
    let mut seen = std::collections::HashSet::new();
    for m in matches {
        let key = format!("{}:{}", m.toolset, m.tool_name);
        if seen.insert(key.clone()) {
            lines.push(key);
        }
    }
    lines.join("\n")
}

/// Format detection result for concise output (Header + TSV).
/// Columns: type, transport, entry, confidence, build
pub fn format_detection(result: &ConciseDetectionResult, no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#type\ttransport\tentry\tconfidence\tbuild".to_string());
    }

    let entry = result.entry_point.unwrap_or("-");
    let build = result
        .build_command
        .map(quote)
        .unwrap_or_else(|| "-".to_string());

    lines.push(format!(
        "{}\t{}\t{}\t{:.0}%\t{}",
        result.server_type,
        result.transport,
        entry,
        result.confidence * 100.0,
        build
    ));

    lines.join("\n")
}

/// Format pack result for concise output (Header + TSV).
/// Columns: file, size
pub fn format_pack_result(filename: &str, bytes: u64, no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#file\tsize".to_string());
    }
    lines.push(format!("{}\t{}", filename, bytes));
    lines.join("\n")
}

/// Format init result for concise output (Header + TSV).
/// Columns: file
pub fn format_init_result(files: &[&str], no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#file".to_string());
    }
    for file in files {
        lines.push((*file).to_string());
    }
    lines.join("\n")
}

/// Format add result for concise output (Header + TSV).
/// Columns: path
pub fn format_add_result(path: &Path, no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#path".to_string());
    }
    lines.push(quote(&path.display().to_string()));
    lines.join("\n")
}

/// Format download result for concise output (Header + TSV).
/// Columns: path
pub fn format_download_result(path: &Path, no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#path".to_string());
    }
    lines.push(path.display().to_string());
    lines.join("\n")
}

/// Format publish result for concise output (Header + TSV).
/// Columns: ref, url
pub fn format_publish_result(
    namespace: &str,
    name: &str,
    version: &str,
    url: &str,
    no_header: bool,
) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#ref\turl".to_string());
    }
    lines.push(format!("{}/{}@{}\t{}", namespace, name, version, url));
    lines.join("\n")
}

/// Format whoami result for concise output (Header + TSV).
/// Columns: user, registry, status
pub fn format_whoami(
    username: Option<&str>,
    registry: &str,
    status: &str,
    no_header: bool,
) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#user\tregistry\tstatus".to_string());
    }
    let user = username.unwrap_or("-");
    lines.push(format!("{}\t{}\t{}", user, registry, status));
    lines.join("\n")
}

/// Format scripts list for concise output (Header + TSV).
/// Columns: script, command
pub fn format_scripts(scripts: &[(String, String)], no_header: bool) -> String {
    let mut lines = Vec::new();
    if !no_header {
        lines.push("#script\tcommand".to_string());
    }
    for (name, cmd) in scripts {
        lines.push(format!("{}\t{}", name, quote(cmd)));
    }
    lines.join("\n")
}

/// Format validation error for concise output (to stderr, Header + TSV).
/// Columns: code, message, location, help
pub fn format_validation_error(
    code: &str,
    message: &str,
    location: &str,
    help: Option<&str>,
) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        code,
        quote(message),
        quote(location),
        help.map(quote).unwrap_or_else(|| "-".to_string())
    )
}

/// Print validation error header (to stderr).
pub fn print_validation_error_header() {
    eprintln!("#code\tmessage\tlocation\thelp");
}

/// Format tool call result for concise output.
/// Just the raw JSON result.
pub fn format_call_result(result: &serde_json::Value) -> String {
    serde_json::to_string(result).expect("Failed to serialize JSON")
}

/// Format tool info header for concise output.
/// Columns: type, location
pub fn format_tool_info_header() -> &'static str {
    "#type\tlocation"
}

/// Format tool info metadata for concise output.
pub fn format_tool_info_meta(tool_type: &str, location: &str) -> String {
    format!("{}\t{}", tool_type, quote(location))
}

/// Format tool list header for info command.
/// Columns: tool, params, outputs
pub fn format_tool_info_tools_header() -> &'static str {
    "#tool\tparams\toutputs"
}

/// Format prompts header.
/// Columns: prompt, args
pub fn format_prompts_header() -> &'static str {
    "#prompt\targs"
}

/// Format resources header.
/// Columns: uri, name, mime
pub fn format_resources_header() -> &'static str {
    "#uri\tname\tmime"
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_quote() {
        assert_eq!(quote("simple"), "simple");
        assert_eq!(quote("has spaces"), "\"has spaces\"");
        assert_eq!(quote("has\ttab"), "\"has\ttab\"");
        assert_eq!(quote("has\"quote"), "\"has\\\"quote\"");
        assert_eq!(quote("has\\backslash"), "\"has\\\\backslash\"");
        assert_eq!(quote("has\nnewline"), "\"has\\nnewline\"");
    }

    #[test]
    fn test_format_tool_list_with_header() {
        let path = PathBuf::from("/home/user/.tool/tools/appcypher/filesystem@0.1.2");
        let entries = vec![ConciseToolEntry {
            name: "appcypher/filesystem",
            tool_type: "stdio",
            description: Some("File operations"),
            path: &path,
        }];

        let output = format_tool_list(&entries, false);
        assert!(output.starts_with("#name\ttype\tpath"));
        assert!(output.contains("appcypher/filesystem"));
    }

    #[test]
    fn test_format_tool_list_no_header() {
        let path = PathBuf::from("/path");
        let entries = vec![ConciseToolEntry {
            name: "appcypher/filesystem",
            tool_type: "stdio",
            description: Some("File operations"),
            path: &path,
        }];

        let output = format_tool_list(&entries, true);
        assert!(!output.starts_with("#"));
        assert!(output.contains("appcypher/filesystem"));
    }

    #[test]
    fn test_format_search_results() {
        let results = vec![ConciseSearchResult {
            namespace: "appcypher",
            name: "filesystem",
            version: Some("0.1.2"),
            description: Some("File operations"),
            downloads: 1000,
        }];

        let output = format_search_results(&results, false);
        assert!(output.starts_with("#ref\tdescription\tdownloads"));
        assert!(output.contains("appcypher/filesystem@0.1.2"));
        assert!(output.contains("1000"));
    }

    #[test]
    fn test_format_whoami() {
        let output = format_whoami(Some("@user"), "https://tool.store", "authenticated", false);
        assert!(output.starts_with("#user\tregistry\tstatus"));
        assert!(output.contains("@user\thttps://tool.store\tauthenticated"));
    }
}
