//! Variable substitution utilities for MCP manifests.
//!
//! Handles `${__dirname}`, `${HOME}`, `${user_config.X}`, `${system_config.X}` and
//! template functions like `${base64(value)}`, `${default(value, fallback)}` in
//! mcp_config args, env, and header values.

use crate::error::{ToolError, ToolResult};
use crate::mcpb::{
    McpbSystemConfigField, McpbSystemConfigType, McpbUserConfigField, McpbUserConfigType,
};
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::SystemTime;

//--------------------------------------------------------------------------------------------------
// Constants
//--------------------------------------------------------------------------------------------------

/// Regex pattern for user_config variable references.
pub const USER_CONFIG_VAR_PATTERN: &str = r"\$\{user_config\.(\w+)\}";

/// Regex pattern for system_config variable references.
pub const SYSTEM_CONFIG_VAR_PATTERN: &str = r"\$\{system_config\.(\w+)\}";

/// Built-in variables that don't require config definition.
pub const BUILTIN_VARS: &[&str] = &["__dirname", "HOME", "DESKTOP", "DOCUMENTS", "DOWNLOADS"];

/// Compiled regex for user_config variable extraction.
static USER_CONFIG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(USER_CONFIG_VAR_PATTERN).expect("Invalid regex pattern"));

/// Compiled regex for system_config variable extraction.
static SYSTEM_CONFIG_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(SYSTEM_CONFIG_VAR_PATTERN).expect("Invalid regex pattern"));

/// Regex for all variable patterns.
static VAR_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{([^}]+)\}").expect("Invalid regex pattern"));

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Extract all user_config variable names from a string.
pub fn extract_user_config_vars(s: &str) -> Vec<String> {
    USER_CONFIG_REGEX
        .captures_iter(s)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Extract all system_config variable names from a string.
pub fn extract_system_config_vars(s: &str) -> Vec<String> {
    SYSTEM_CONFIG_REGEX
        .captures_iter(s)
        .map(|cap| cap[1].to_string())
        .collect()
}

/// Substitute variables in a string.
///
/// Finds all `${...}` patterns and evaluates the inner expression using
/// `eval_expr`, which supports variable references, string literals,
/// function calls, and nested function calls.
pub fn substitute_vars(
    s: &str,
    dirname: &str,
    user_config: &BTreeMap<String, String>,
    system_config: &BTreeMap<String, String>,
) -> ToolResult<String> {
    let mut result = s.to_string();
    let mut errors = Vec::new();

    for cap in VAR_REGEX.captures_iter(s) {
        let full_match = &cap[0];
        let inner = &cap[1];

        match eval_expr(inner, dirname, user_config, system_config) {
            Ok(Some(value)) => {
                result = result.replace(full_match, &value);
            }
            Ok(None) => {
                errors.push(format!("Undefined variable: {}", inner));
            }
            Err(e) => {
                errors.push(e.to_string());
            }
        }
    }

    if !errors.is_empty() {
        return Err(ToolError::Generic(errors.join(", ")));
    }

    Ok(result)
}

/// Evaluate a template expression.
///
/// Handles:
/// - String literals: `'hello'`
/// - Variable references: `user_config.X`, `system_config.X`, `__dirname`, `HOME`, etc.
/// - Function calls: `base64(value)`, `concat(a, b)`, `default(value, fallback)`, etc.
/// - Nested calls: `base64(concat(user_config.user, ':', user_config.pass))`
///
/// Returns `Ok(Some(value))` for resolved values, `Ok(None)` for undefined variables.
fn eval_expr(
    expr: &str,
    dirname: &str,
    user_config: &BTreeMap<String, String>,
    system_config: &BTreeMap<String, String>,
) -> Result<Option<String>, ToolError> {
    let expr = expr.trim();

    if expr.is_empty() {
        return Ok(Some(String::new()));
    }

    // String literal: 'value'
    if expr.starts_with('\'') && expr.ends_with('\'') && expr.len() >= 2 {
        return Ok(Some(expr[1..expr.len() - 1].to_string()));
    }

    // Function call: name(args...)
    // Find the first '(' and check that the prefix is a valid identifier
    if let Some(paren_start) = expr.find('(') {
        let name = &expr[..paren_start];
        if expr.ends_with(')')
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            && !name.contains('.')
        {
            let args_str = &expr[paren_start + 1..expr.len() - 1];
            return eval_func(name, args_str, dirname, user_config, system_config);
        }
    }

    // Variable reference
    resolve_var(expr, dirname, user_config, system_config)
}

/// Resolve a variable reference to its value.
fn resolve_var(
    name: &str,
    dirname: &str,
    user_config: &BTreeMap<String, String>,
    system_config: &BTreeMap<String, String>,
) -> Result<Option<String>, ToolError> {
    if name == "__dirname" {
        Ok(Some(dirname.to_string()))
    } else if name == "HOME" {
        Ok(dirs::home_dir().map(|p| p.to_string_lossy().to_string()))
    } else if name == "DESKTOP" {
        Ok(dirs::desktop_dir().map(|p| p.to_string_lossy().to_string()))
    } else if name == "DOCUMENTS" {
        Ok(dirs::document_dir().map(|p| p.to_string_lossy().to_string()))
    } else if name == "DOWNLOADS" {
        Ok(dirs::download_dir().map(|p| p.to_string_lossy().to_string()))
    } else if let Some(key) = name.strip_prefix("user_config.") {
        Ok(user_config.get(key).cloned())
    } else if let Some(key) = name.strip_prefix("system_config.") {
        Ok(system_config.get(key).cloned())
    } else {
        Ok(std::env::var(name).ok())
    }
}

/// Split function arguments at depth-0 commas, respecting nested parens and quoted strings.
fn split_args(args_str: &str) -> Vec<&str> {
    if args_str.trim().is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut depth = 0;
    let mut in_quote = false;
    let mut start = 0;

    for (i, c) in args_str.char_indices() {
        match c {
            '\'' if !in_quote => in_quote = true,
            '\'' if in_quote => in_quote = false,
            '(' if !in_quote => depth += 1,
            ')' if !in_quote && depth > 0 => depth -= 1,
            ',' if !in_quote && depth == 0 => {
                result.push(&args_str[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    result.push(&args_str[start..]);
    result
}

/// Evaluate a template function call.
fn eval_func(
    name: &str,
    args_str: &str,
    dirname: &str,
    user_config: &BTreeMap<String, String>,
    system_config: &BTreeMap<String, String>,
) -> Result<Option<String>, ToolError> {
    let args = split_args(args_str);

    match name {
        // -- Encoding functions --
        "base64" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(base64_encode(&val)))
        }
        "base64url" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(base64url_encode(&val)))
        }
        "urlEncode" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(urlencoding::encode(&val).into_owned()))
        }
        "hex" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(hex_encode(&val)))
        }

        // -- String functions --
        "concat" => {
            let mut result = String::new();
            for arg in &args {
                let val = eval_expr_required(arg, dirname, user_config, system_config)?;
                result.push_str(&val);
            }
            Ok(Some(result))
        }
        "lower" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(val.to_lowercase()))
        }
        "upper" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(val.to_uppercase()))
        }
        "trim" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(val.trim().to_string()))
        }
        "default" => {
            require_args(name, &args, 2)?;
            let first = eval_expr(args[0].trim(), dirname, user_config, system_config)?;
            match first {
                Some(v) if !v.is_empty() => Ok(Some(v)),
                _ => {
                    let fallback =
                        eval_expr_required(args[1], dirname, user_config, system_config)?;
                    Ok(Some(fallback))
                }
            }
        }

        // -- Auth functions --
        "basicAuth" => {
            require_args(name, &args, 2)?;
            let user = eval_expr_required(args[0], dirname, user_config, system_config)?;
            let pass = eval_expr_required(args[1], dirname, user_config, system_config)?;
            Ok(Some(format!(
                "Basic {}",
                base64_encode(&format!("{}:{}", user, pass))
            )))
        }
        "bearer" => {
            require_args(name, &args, 1)?;
            let token = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(format!("Bearer {}", token)))
        }

        // -- Utility functions --
        "timestamp" => Ok(Some(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string()),
        )),
        "uuid" => Ok(Some(uuid::Uuid::new_v4().to_string())),
        "jsonEncode" => {
            require_args(name, &args, 1)?;
            let val = eval_expr_required(args[0], dirname, user_config, system_config)?;
            Ok(Some(serde_json::Value::String(val).to_string()))
        }

        _ => Err(ToolError::Generic(format!(
            "Unknown template function: {}",
            name
        ))),
    }
}

/// Evaluate an expression, returning an error if the variable is undefined.
fn eval_expr_required(
    expr: &str,
    dirname: &str,
    user_config: &BTreeMap<String, String>,
    system_config: &BTreeMap<String, String>,
) -> Result<String, ToolError> {
    eval_expr(expr, dirname, user_config, system_config)?
        .ok_or_else(|| ToolError::Generic(format!("Undefined variable: {}", expr.trim())))
}

/// Validate the number of arguments for a template function.
fn require_args(func_name: &str, args: &[&str], min: usize) -> Result<(), ToolError> {
    if args.len() < min {
        return Err(ToolError::Generic(format!(
            "{}() requires at least {} argument(s), got {}",
            func_name,
            min,
            args.len()
        )));
    }
    Ok(())
}

/// Base64 encode a string.
fn base64_encode(s: &str) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.encode(s.as_bytes())
}

/// URL-safe Base64 encode a string (no padding).
fn base64url_encode(s: &str) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(s.as_bytes())
}

/// Hex encode a string's bytes.
fn hex_encode(s: &str) -> String {
    s.as_bytes().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Check if a variable name is a built-in variable.
pub fn is_builtin_var(name: &str) -> bool {
    BUILTIN_VARS.contains(&name)
}

/// Validate user_config values against the schema.
pub fn validate_user_config(
    schema: &BTreeMap<String, McpbUserConfigField>,
    values: &BTreeMap<String, String>,
) -> ToolResult<()> {
    for (name, field) in schema {
        let value = values.get(name);

        // Check required fields
        if field.required.unwrap_or(false) && value.is_none() {
            return Err(ToolError::Generic(format!(
                "Required config field '{}' is missing",
                name
            )));
        }

        // Validate type and constraints
        if let Some(v) = value {
            match field.field_type {
                McpbUserConfigType::Number => {
                    let num: f64 = v
                        .parse()
                        .map_err(|_| ToolError::Generic(format!("'{}' must be a number", name)))?;
                    if let Some(min) = field.min
                        && num < min
                    {
                        return Err(ToolError::Generic(format!("'{}' must be >= {}", name, min)));
                    }
                    if let Some(max) = field.max
                        && num > max
                    {
                        return Err(ToolError::Generic(format!("'{}' must be <= {}", name, max)));
                    }
                }
                McpbUserConfigType::String => {
                    if let Some(ref enum_values) = field.enum_values
                        && !enum_values.contains(v)
                    {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be one of: {:?}",
                            name, enum_values
                        )));
                    }
                }
                McpbUserConfigType::Boolean => {
                    if v != "true" && v != "false" {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be 'true' or 'false'",
                            name
                        )));
                    }
                }
                McpbUserConfigType::Directory | McpbUserConfigType::File => {}
            }
        }
    }
    Ok(())
}

/// Validate system_config values against the schema.
pub fn validate_system_config(
    schema: &BTreeMap<String, McpbSystemConfigField>,
    values: &BTreeMap<String, String>,
) -> ToolResult<()> {
    for (name, field) in schema {
        let value = values.get(name);

        // Check required fields
        if field.required.unwrap_or(false) && value.is_none() {
            return Err(ToolError::Generic(format!(
                "Required system_config field '{}' is missing",
                name
            )));
        }

        // Validate type and constraints
        if let Some(v) = value {
            match field.field_type {
                McpbSystemConfigType::Port => {
                    let num: f64 = v
                        .parse()
                        .map_err(|_| ToolError::Generic(format!("'{}' must be a number", name)))?;
                    if !(1.0..=65535.0).contains(&num) {
                        return Err(ToolError::Generic(format!(
                            "'{}' must be a valid port (1-65535)",
                            name
                        )));
                    }
                }
                McpbSystemConfigType::TempDirectory | McpbSystemConfigType::DataDirectory => {}
            }
        }
    }
    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let mut user = BTreeMap::new();
        user.insert("username".to_string(), "alice".to_string());
        user.insert("password".to_string(), "s3cret".to_string());
        user.insert("api_key".to_string(), "key-123".to_string());
        user.insert("empty_val".to_string(), String::new());

        let mut system = BTreeMap::new();
        system.insert("port".to_string(), "3000".to_string());

        (user, system)
    }

    // -- eval_expr basics --

    #[test]
    fn eval_string_literal() {
        let (user, system) = make_config();
        let result = eval_expr("'hello'", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn eval_empty_string_literal() {
        let (user, system) = make_config();
        let result = eval_expr("''", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some(String::new()));
    }

    #[test]
    fn eval_user_config_var() {
        let (user, system) = make_config();
        let result = eval_expr("user_config.username", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("alice".to_string()));
    }

    #[test]
    fn eval_system_config_var() {
        let (user, system) = make_config();
        let result = eval_expr("system_config.port", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("3000".to_string()));
    }

    #[test]
    fn eval_dirname_var() {
        let (user, system) = make_config();
        let result = eval_expr("__dirname", "/my/tool", &user, &system).unwrap();
        assert_eq!(result, Some("/my/tool".to_string()));
    }

    #[test]
    fn eval_undefined_var() {
        let (user, system) = make_config();
        let result = eval_expr("user_config.nonexistent", "/dir", &user, &system).unwrap();
        assert_eq!(result, None);
    }

    // -- split_args --

    #[test]
    fn split_args_simple() {
        let args = split_args("a, b, c");
        assert_eq!(args, vec!["a", " b", " c"]);
    }

    #[test]
    fn split_args_with_quotes() {
        let args = split_args("user_config.user, ':'");
        assert_eq!(args, vec!["user_config.user", " ':'"]);
    }

    #[test]
    fn split_args_nested_parens() {
        let args = split_args("concat(user_config.a, user_config.b), 'suffix'");
        assert_eq!(
            args,
            vec!["concat(user_config.a, user_config.b)", " 'suffix'"]
        );
    }

    #[test]
    fn split_args_empty() {
        let args = split_args("");
        assert!(args.is_empty());
    }

    // -- Encoding functions --

    #[test]
    fn func_base64() {
        let (user, system) = make_config();
        let result = eval_expr("base64(user_config.username)", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some(base64_encode("alice")));
    }

    #[test]
    fn func_base64_literal() {
        let (user, system) = make_config();
        let result = eval_expr("base64('hello world')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some(base64_encode("hello world")));
    }

    #[test]
    fn func_base64url() {
        let (user, system) = make_config();
        let result = eval_expr("base64url('hello world')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some(base64url_encode("hello world")));
        // URL-safe base64 should not contain +, /, or =
        let val = result.unwrap();
        assert!(!val.contains('+'));
        assert!(!val.contains('/'));
        assert!(!val.contains('='));
    }

    #[test]
    fn func_url_encode() {
        let (user, system) = make_config();
        let result = eval_expr("urlEncode('hello world&foo=bar')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("hello%20world%26foo%3Dbar".to_string()));
    }

    #[test]
    fn func_hex() {
        let (user, system) = make_config();
        let result = eval_expr("hex('abc')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("616263".to_string()));
    }

    // -- String functions --

    #[test]
    fn func_concat() {
        let (user, system) = make_config();
        let result = eval_expr(
            "concat(user_config.username, ':', user_config.password)",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, Some("alice:s3cret".to_string()));
    }

    #[test]
    fn func_lower() {
        let (user, system) = make_config();
        let result = eval_expr("lower('HELLO')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn func_upper() {
        let (user, system) = make_config();
        let result = eval_expr("upper('hello')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("HELLO".to_string()));
    }

    #[test]
    fn func_trim() {
        let (user, system) = make_config();
        let result = eval_expr("trim('  hello  ')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("hello".to_string()));
    }

    #[test]
    fn func_default_with_defined_var() {
        let (user, system) = make_config();
        let result = eval_expr(
            "default(user_config.username, 'fallback')",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, Some("alice".to_string()));
    }

    #[test]
    fn func_default_with_undefined_var() {
        let (user, system) = make_config();
        let result = eval_expr(
            "default(user_config.nonexistent, 'fallback')",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, Some("fallback".to_string()));
    }

    #[test]
    fn func_default_with_empty_var() {
        let (user, system) = make_config();
        let result = eval_expr(
            "default(user_config.empty_val, 'fallback')",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, Some("fallback".to_string()));
    }

    // -- Auth functions --

    #[test]
    fn func_basic_auth() {
        let (user, system) = make_config();
        let result = eval_expr(
            "basicAuth(user_config.username, user_config.password)",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        let expected = format!("Basic {}", base64_encode("alice:s3cret"));
        assert_eq!(result, Some(expected));
    }

    #[test]
    fn func_bearer() {
        let (user, system) = make_config();
        let result = eval_expr("bearer(user_config.api_key)", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("Bearer key-123".to_string()));
    }

    // -- Utility functions --

    #[test]
    fn func_timestamp() {
        let (user, system) = make_config();
        let result = eval_expr("timestamp()", "/dir", &user, &system)
            .unwrap()
            .unwrap();
        let ts: u64 = result.parse().expect("timestamp should be a number");
        // Should be a reasonable Unix timestamp (after 2020)
        assert!(ts > 1_577_836_800);
    }

    #[test]
    fn func_uuid() {
        let (user, system) = make_config();
        let result = eval_expr("uuid()", "/dir", &user, &system)
            .unwrap()
            .unwrap();
        // UUID v4 format: 8-4-4-4-12 hex digits
        assert_eq!(result.len(), 36);
        assert_eq!(result.chars().filter(|&c| c == '-').count(), 4);
    }

    #[test]
    fn func_json_encode() {
        let (user, system) = make_config();
        let result = eval_expr("jsonEncode('hello \"world\"')", "/dir", &user, &system).unwrap();
        assert_eq!(result, Some("\"hello \\\"world\\\"\"".to_string()));
    }

    // -- Nested function calls --

    #[test]
    fn nested_base64_concat() {
        let (user, system) = make_config();
        let result = eval_expr(
            "base64(concat(user_config.username, ':', user_config.password))",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, Some(base64_encode("alice:s3cret")));
    }

    #[test]
    fn nested_upper_concat() {
        let (user, system) = make_config();
        let result = eval_expr(
            "upper(concat('hello', ' ', 'world'))",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, Some("HELLO WORLD".to_string()));
    }

    // -- substitute_vars integration --

    #[test]
    fn substitute_simple_var() {
        let (user, system) = make_config();
        let result =
            substitute_vars("Hello ${user_config.username}!", "/dir", &user, &system).unwrap();
        assert_eq!(result, "Hello alice!");
    }

    #[test]
    fn substitute_dirname() {
        let (user, system) = make_config();
        let result =
            substitute_vars("${__dirname}/server.js", "/opt/tool", &user, &system).unwrap();
        assert_eq!(result, "/opt/tool/server.js");
    }

    #[test]
    fn substitute_function_call() {
        let (user, system) = make_config();
        let result = substitute_vars(
            "Bearer ${base64(user_config.api_key)}",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, format!("Bearer {}", base64_encode("key-123")));
    }

    #[test]
    fn substitute_default_function() {
        let (user, system) = make_config();
        let result = substitute_vars(
            "${default(user_config.nonexistent, 'none')}",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, "none");
    }

    #[test]
    fn substitute_multiple_vars() {
        let (user, system) = make_config();
        let result = substitute_vars(
            "${user_config.username}:${system_config.port}",
            "/dir",
            &user,
            &system,
        )
        .unwrap();
        assert_eq!(result, "alice:3000");
    }

    // -- Error cases --

    #[test]
    fn error_undefined_variable() {
        let (user, system) = make_config();
        let result = substitute_vars("${user_config.missing}", "/dir", &user, &system);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Undefined variable"));
    }

    #[test]
    fn error_unknown_function() {
        let (user, system) = make_config();
        let result = eval_expr("unknownFunc('a')", "/dir", &user, &system);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown template function"));
    }

    #[test]
    fn error_missing_args() {
        let (user, system) = make_config();
        let result = eval_expr("base64()", "/dir", &user, &system);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("requires at least"));
    }
}
