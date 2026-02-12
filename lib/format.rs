//! Formatting utilities for human-readable output.

use colored::Colorize;
use serde_json::Value;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Maximum length for single-line descriptions before truncation.
const MAX_DESC_LEN: usize = 60;

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Truncate a parameter description for non-verbose display.
///
/// Returns the description truncated to `MAX_DESC_LEN` with "..." suffix if needed.
/// In verbose mode, returns the full description.
pub fn truncate_param_desc(desc: &str, verbose: bool) -> String {
    if verbose || desc.len() <= MAX_DESC_LEN {
        desc.to_string()
    } else {
        format!("{}...", &desc[..MAX_DESC_LEN - 3])
    }
}

/// Format a description for display.
///
/// - Default: returns first non-empty line only (truncated if needed)
/// - Verbose: returns all lines with relative indentation preserved
pub fn format_description(desc: &str, verbose: bool, indent: &str) -> Option<String> {
    let lines: Vec<&str> = desc.lines().collect();

    // Find first and last non-empty lines (using trimmed check)
    let first_non_empty = lines.iter().position(|l| !l.trim().is_empty())?;

    if !verbose {
        // Default: first non-empty line only, trimmed and truncated
        let line = lines[first_non_empty].trim();
        if line.len() > MAX_DESC_LEN {
            return Some(format!("{}...", &line[..MAX_DESC_LEN - 3]));
        }
        return Some(line.to_string());
    }

    // Verbose: preserve relative indentation
    let last_non_empty = lines.iter().rposition(|l| !l.trim().is_empty())?;
    let relevant_lines = &lines[first_non_empty..=last_non_empty];

    // Find minimum indentation (ignoring empty lines)
    let min_indent = relevant_lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    // Format lines: strip common indent, add new indent prefix
    let formatted: Vec<String> = relevant_lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                // Strip the common minimum indent, then add our prefix
                let stripped = if l.len() > min_indent {
                    &l[min_indent..]
                } else {
                    l.trim_start()
                };
                format!("{}{}", indent, stripped)
            }
        })
        .collect();

    Some(formatted.join("\n"))
}

/// Syntax highlight a JSON value for terminal output.
///
/// Applies colors to different JSON token types:
/// - Keys: cyan
/// - Strings: green
/// - Numbers: yellow
/// - Booleans: magenta
/// - Null: dimmed
/// - Punctuation: default
pub fn highlight_json(value: &Value) -> String {
    highlight_json_inner(value, 0)
}

fn highlight_json_inner(value: &Value, indent: usize) -> String {
    let indent_str = "  ".repeat(indent);
    let next_indent = "  ".repeat(indent + 1);

    match value {
        Value::Null => "null".dimmed().to_string(),
        Value::Bool(b) => b.to_string().magenta().to_string(),
        Value::Number(n) => n.to_string().yellow().to_string(),
        Value::String(s) => format!("\"{}\"", escape_json_string(s)).green().to_string(),
        Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else {
                let items: Vec<String> = arr
                    .iter()
                    .map(|v| format!("{}{}", next_indent, highlight_json_inner(v, indent + 1)))
                    .collect();
                format!("[\n{}\n{}]", items.join(",\n"), indent_str)
            }
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else {
                let items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| {
                        let key = format!("\"{}\"", k).cyan().to_string();
                        let val = highlight_json_inner(v, indent + 1);
                        format!("{}{}: {}", next_indent, key, val)
                    })
                    .collect();
                format!("{{\n{}\n{}}}", items.join(",\n"), indent_str)
            }
        }
    }
}

/// Escape special characters in a JSON string.
fn escape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_line() {
        let desc = "Get current weather for a location.";
        assert_eq!(
            format_description(desc, false, ""),
            Some("Get current weather for a location.".to_string())
        );
    }

    #[test]
    fn test_multiline_default() {
        let desc = "Get current weather for a location.\n\nArgs:\n    latitude: Latitude";
        assert_eq!(
            format_description(desc, false, ""),
            Some("Get current weather for a location.".to_string())
        );
    }

    #[test]
    fn test_multiline_verbose() {
        let desc = "Get current weather.\n\nArgs:\n    latitude: Lat";
        let result = format_description(desc, true, "  ");
        // Relative indentation is preserved: "Args" at base, "latitude" indented 4 more
        assert_eq!(
            result,
            Some("  Get current weather.\n\n  Args:\n      latitude: Lat".to_string())
        );
    }

    #[test]
    fn test_truncation() {
        let desc = "This is a very long description that should be truncated because it exceeds the maximum length";
        let result = format_description(desc, false, "").unwrap();
        assert!(result.len() <= MAX_DESC_LEN);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_leading_whitespace() {
        let desc = "\n\n  Get weather.  \n\n";
        assert_eq!(
            format_description(desc, false, ""),
            Some("Get weather.".to_string())
        );
    }

    #[test]
    fn test_empty() {
        let desc = "\n\n  \n";
        assert_eq!(format_description(desc, false, ""), None);
    }

    #[test]
    fn test_highlight_json_primitives() {
        use serde_json::json;

        // Null
        let result = highlight_json(&json!(null));
        assert!(result.contains("null"));

        // Boolean
        let result = highlight_json(&json!(true));
        assert!(result.contains("true"));

        // Number
        let result = highlight_json(&json!(42));
        assert!(result.contains("42"));

        // String
        let result = highlight_json(&json!("hello"));
        assert!(result.contains("hello"));
    }

    #[test]
    fn test_highlight_json_object() {
        use serde_json::json;

        let value = json!({"key": "value", "num": 123});
        let result = highlight_json(&value);

        // Should contain the key and value
        assert!(result.contains("key"));
        assert!(result.contains("value"));
        assert!(result.contains("123"));
    }

    #[test]
    fn test_highlight_json_array() {
        use serde_json::json;

        let value = json!([1, 2, 3]);
        let result = highlight_json(&value);

        assert!(result.contains('1'));
        assert!(result.contains('2'));
        assert!(result.contains('3'));
    }

    #[test]
    fn test_escape_json_string() {
        assert_eq!(escape_json_string("hello"), "hello");
        assert_eq!(escape_json_string("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_json_string("tab\there"), "tab\\there");
        assert_eq!(escape_json_string("quote\"here"), "quote\\\"here");
    }
}
