//! Formatting utilities for human-readable output.

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
}
