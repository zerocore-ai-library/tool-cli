# Plan: `tool config` Command

## Overview

Add a `tool config` command with subcommands to persistently store user configuration for tools. Configuration is saved to `~/.tool/config/[<namespace>/]<tool-name>/config.json` and automatically loaded by `tool info` and `tool call`.

## Command Specification

```
tool config set <tool-ref> [key=value...]   # Set config (interactive if no values)
tool config get <tool-ref> [key]            # Show all config or specific key
tool config list                            # List all configured tools
tool config unset <tool-ref> <key>          # Remove a specific key
tool config reset <tool-ref>                # Remove all config for tool
```

### Subcommands

| Subcommand | Description |
|------------|-------------|
| `set` | Set configuration values (interactive or non-interactive) |
| `get` | Display saved configuration |
| `list` | List all tools with saved configuration |
| `unset` | Remove a specific configuration key |
| `reset` | Remove all configuration for a tool |

### Flags

| Flag | Short | Applies To | Description |
|------|-------|------------|-------------|
| `--yes` | `-y` | `set` | Skip interactive prompts, use provided values only |
| `--config` | `-k` | `set` | Specify config as KEY=VALUE (repeatable) |
| `--concise` | `-c` | all | Machine-parseable output (global flag) |
| `--no-header` | `-H` | all | Suppress header in concise mode (global flag) |

### Usage Examples

```bash
# Interactive mode - prompts for all user_config fields with defaults shown
tool config set appcypher/service

# Non-interactive with trailing args
tool config set appcypher/service -y workspace=/home/user/code

# Non-interactive with -k flags
tool config set appcypher/service --yes -k api_key=xxx -k max_results=50

# Mixed: -k flags + trailing args (both work)
tool config set appcypher/service -y -k api_key=xxx workspace=/code

# Show all config for a tool
tool config get appcypher/service

# Show specific key
tool config get appcypher/service api_key

# List all configured tools
tool config list

# Remove specific key
tool config unset appcypher/service api_key

# Reset all config for a tool
tool config reset appcypher/service

# Concise output
tool config get appcypher/service -c
tool config list -c
```

## Storage

### Directory Structure

Config is stored **per-tool name** (without version), so upgrades preserve config:

```
~/.tool/config/
├── appcypher/
│   └── filesystem/           # Config for appcypher/service (any version)
│       └── config.json
├── github-mcp/
│   └── config.json
└── local-tool/
    └── config.json
```

**Note:** If user has `appcypher/service@1.0.0` and `appcypher/service@2.0.0`, they share the same config. This is intentional - config should survive upgrades.

### Config File Format

```json
{
  "api_key": "sk-xxx",
  "workspace": "/home/user/code",
  "max_results": "50"
}
```

All values stored as strings (matching current `BTreeMap<String, String>` pattern in call.rs).

## Type-Specific Handling

The MCPB `user_config` schema supports these types:

| Type | Interactive Prompt | Validation | Storage |
|------|-------------------|------------|---------|
| `string` | `cliclack::input()` | None | As-is |
| `number` | `cliclack::input()` | Validate `min`/`max` if specified | As string |
| `boolean` | `cliclack::confirm()` | Must be `true`/`false` | `"true"` or `"false"` |
| `directory` | `cliclack::input()` | Expand `~` to home dir | Expanded path |
| `file` | `cliclack::input()` | Expand `~` to home dir | Expanded path |

### Variable Substitution in Defaults

Schema defaults can contain variables like `${HOME}`, `${DOCUMENTS}`. These must be resolved before displaying:

```json
"workspace": {
  "type": "directory",
  "default": "${HOME}/Documents"
}
```

When prompting, show the resolved value:
```
Workspace Directory (/home/steve/Documents):
```

Supported variables (from MCPB spec):
- `${HOME}` - User home directory
- `${DESKTOP}` - Desktop folder
- `${DOCUMENTS}` - Documents folder
- `${DOWNLOADS}` - Downloads folder

### Numeric Validation

For `number` type with `min`/`max`:

```json
"max_results": {
  "type": "number",
  "min": 1,
  "max": 100
}
```

Validate on set:
```
error: 'max_results' must be between 1 and 100, got 500
```

## Implementation Steps

### Step 1: Add Config Path Constant

**File: `lib/constants.rs`**

```rust
/// Default path for tool configuration storage.
pub static DEFAULT_CONFIG_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| DEFAULT_HOME_PATH.join("config"));
```

### Step 2: Add Command Definition

**File: `lib/commands.rs`**

Add `Config` variant with subcommand enum:

```rust
/// Configure tool user settings.
#[command(subcommand)]
Config(ConfigCommand),
```

Add new `ConfigCommand` enum:

```rust
/// Config subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Set configuration values for a tool.
    Set {
        /// Tool reference.
        tool: String,

        /// Configuration values as trailing args (KEY=VALUE).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        values: Vec<String>,

        /// Skip interactive prompts, use provided values only.
        #[arg(short, long)]
        yes: bool,

        /// Configuration values (KEY=VALUE), repeatable.
        #[arg(short = 'C', long = "config")]
        config: Vec<String>,
    },

    /// Show configuration for a tool.
    Get {
        /// Tool reference.
        tool: String,

        /// Specific key to show (shows all if omitted).
        key: Option<String>,
    },

    /// List all tools with saved configuration.
    List,

    /// Remove a specific configuration key.
    Unset {
        /// Tool reference.
        tool: String,

        /// Key to remove.
        key: String,
    },

    /// Remove all configuration for a tool.
    Reset {
        /// Tool reference.
        tool: String,
    },
}
```

### Step 3: Create Config Handler Module

**File: `lib/handlers/tool/config_cmd.rs`** (new file)

Core functions:

```rust
/// Main entry point for config command.
pub async fn config_tool(
    cmd: ConfigCommand,
    concise: bool,
    no_header: bool,
) -> ToolResult<()>

/// Handle `config set` subcommand.
async fn config_set(
    tool: String,
    values: Vec<String>,
    yes: bool,
    config_flags: Vec<String>,
    concise: bool,
) -> ToolResult<()>

/// Handle `config get` subcommand.
async fn config_get(
    tool: String,
    key: Option<String>,
    concise: bool,
    no_header: bool,
) -> ToolResult<()>

/// Handle `config list` subcommand.
async fn config_list(
    concise: bool,
    no_header: bool,
) -> ToolResult<()>

/// Handle `config unset` subcommand.
async fn config_unset(
    tool: String,
    key: String,
    concise: bool,
) -> ToolResult<()>

/// Handle `config reset` subcommand.
async fn config_reset(
    tool: String,
    concise: bool,
) -> ToolResult<()>

/// Get config directory path for a tool reference (without version).
fn get_config_dir(tool_ref: &PluginRef) -> PathBuf

/// Load saved config for a tool.
pub fn load_tool_config(tool_ref: &PluginRef) -> ToolResult<BTreeMap<String, String>>

/// Load saved config by name (for use when we only have name, not full ref).
pub fn load_tool_config_by_name(namespace: Option<&str>, name: &str) -> ToolResult<BTreeMap<String, String>>

/// Save config for a tool.
fn save_tool_config(tool_ref: &PluginRef, config: &BTreeMap<String, String>) -> ToolResult<()>

/// Delete config directory for a tool.
fn delete_tool_config(tool_ref: &PluginRef) -> ToolResult<()>

/// List all tools with saved config.
fn list_configured_tools() -> ToolResult<Vec<(String, PathBuf, usize)>>

/// Interactive prompt for all user_config fields.
fn prompt_all_user_config(
    schema: &BTreeMap<String, McpbUserConfigField>,
    existing: &BTreeMap<String, String>,
) -> ToolResult<BTreeMap<String, String>>

/// Resolve variable substitution in a string (${HOME}, etc.).
fn resolve_variables(s: &str) -> String

/// Validate a value against its schema field.
fn validate_field_value(key: &str, value: &str, field: &McpbUserConfigField) -> ToolResult<()>
```

### Step 4: Update Handler Module Exports

**File: `lib/handlers/tool/mod.rs`**

```rust
mod config_cmd;
pub use config_cmd::{config_tool, load_tool_config, load_tool_config_by_name};
```

### Step 5: Wire Up Command in Main

**File: `bin/tool.rs`**

Add match arm for `Command::Config`:

```rust
Command::Config(cmd) => {
    handlers::config_tool(cmd, cli.concise, cli.no_header).await
}
```

### Step 6: Integrate Config Loading into `tool info` and `tool call`

**File: `lib/handlers/tool/call.rs`**

Update `parse_user_config()` to load saved config first:

```rust
pub(super) fn parse_user_config(
    config_flags: &[String],
    config_file: Option<&str>,
    tool_ref: Option<&PluginRef>,  // NEW parameter
) -> ToolResult<BTreeMap<String, String>> {
    let mut config = BTreeMap::new();

    // 1. Load saved config (lowest priority)
    if let Some(ref_) = tool_ref {
        if let Ok(saved) = load_tool_config(ref_) {
            config.extend(saved);
        }
    }

    // 2. Load from config file
    if let Some(file_path) = config_file {
        let content = std::fs::read_to_string(file_path)?;
        let file_config: BTreeMap<String, String> = serde_json::from_str(&content)
            .or_else(|_| toml::from_str(&content))
            .map_err(|e| ToolError::Generic(format!("Failed to parse config file: {}", e)))?;
        config.extend(file_config);
    }

    // 3. Parse -C flags (highest priority)
    for flag in config_flags {
        if let Some((key, value)) = flag.split_once('=') {
            config.insert(key.to_string(), value.to_string());
        } else {
            return Err(ToolError::Generic(format!(
                "Invalid config format '{}'. Expected key=value",
                flag
            )));
        }
    }

    Ok(config)
}
```

Update callers in `tool_call()` and `tool_info()` to pass tool reference.

### Step 7: Implement Interactive Prompting

Use `title` field for display, handle types appropriately:

```rust
fn prompt_all_user_config(
    schema: &BTreeMap<String, McpbUserConfigField>,
    existing: &BTreeMap<String, String>,
) -> ToolResult<BTreeMap<String, String>> {
    if schema.is_empty() {
        return Err(ToolError::Generic("Tool has no configurable options".into()));
    }

    init_theme();
    cliclack::intro("Tool configuration")?;

    let mut result = BTreeMap::new();

    for (key, field) in schema {
        let field_type = field.field_type.as_deref().unwrap_or("string");
        let is_sensitive = field.sensitive.unwrap_or(false);
        let is_required = field.required.unwrap_or(false);

        // Use title for display, fall back to key
        let display_name = field.title.as_deref().unwrap_or(key);
        let description = field.description.as_deref().unwrap_or("");

        // Build prompt text: "API Key" or "API Key (Your API key for auth)"
        let prompt_text = if description.is_empty() {
            display_name.to_string()
        } else {
            format!("{} ({})", display_name, description)
        };

        // Get default: existing value > schema default (with variables resolved)
        let default_value = existing.get(key).cloned().or_else(|| {
            field.default.as_ref().map(|d| {
                let raw = match d {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => d.to_string(),
                };
                resolve_variables(&raw)  // Expand ${HOME}, etc.
            })
        });

        let value: String = match field_type {
            "boolean" => {
                // Use confirm prompt for booleans
                let default_bool = default_value
                    .as_ref()
                    .map(|v| v == "true")
                    .unwrap_or(false);
                let confirmed = cliclack::confirm(&prompt_text)
                    .initial_value(default_bool)
                    .interact()?;
                confirmed.to_string()
            }
            "number" => {
                // Text input with validation
                let input: String = if let Some(default) = &default_value {
                    cliclack::input(&prompt_text)
                        .default_input(default)
                        .validate(|v: &String| validate_number(v, field))
                        .interact()?
                } else {
                    cliclack::input(&prompt_text)
                        .required(is_required)
                        .validate(|v: &String| validate_number(v, field))
                        .interact()?
                };
                input
            }
            _ => {
                // string, directory, file
                if is_sensitive {
                    cliclack::password(&prompt_text).interact()?
                } else if let Some(default) = &default_value {
                    cliclack::input(&prompt_text)
                        .default_input(default)
                        .interact()?
                } else {
                    cliclack::input(&prompt_text)
                        .required(is_required)
                        .interact()?
                }
            }
        };

        // Expand ~ for directory/file types
        let final_value = if matches!(field_type, "directory" | "file") {
            expand_tilde(&value)
        } else {
            value
        };

        if !final_value.is_empty() {
            result.insert(key.clone(), final_value);
        }
    }

    cliclack::outro("Configuration saved!")?;
    Ok(result)
}

/// Validate number against min/max constraints.
fn validate_number(value: &str, field: &McpbUserConfigField) -> Result<(), &'static str> {
    if value.is_empty() {
        return Ok(());  // Empty is OK if not required (handled elsewhere)
    }

    let num: f64 = value.parse().map_err(|_| "Must be a valid number")?;

    if let Some(min) = field.min {
        if num < min {
            return Err("Value is below minimum");
        }
    }
    if let Some(max) = field.max {
        if num > max {
            return Err("Value is above maximum");
        }
    }
    Ok(())
}

/// Expand ~ to home directory.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

/// Resolve ${VAR} placeholders in strings.
fn resolve_variables(s: &str) -> String {
    let mut result = s.to_string();

    if let Some(home) = dirs::home_dir() {
        result = result.replace("${HOME}", &home.to_string_lossy());
    }
    if let Some(desktop) = dirs::desktop_dir() {
        result = result.replace("${DESKTOP}", &desktop.to_string_lossy());
    }
    if let Some(docs) = dirs::document_dir() {
        result = result.replace("${DOCUMENTS}", &docs.to_string_lossy());
    }
    if let Some(downloads) = dirs::download_dir() {
        result = result.replace("${DOWNLOADS}", &downloads.to_string_lossy());
    }

    result
}
```

### Step 8: Handle Ambiguous References

When resolving tool reference, use the same ambiguity handling as `tool info`/`tool call`:

```rust
async fn resolve_tool_for_config(tool: &str) -> ToolResult<(PluginRef, PathBuf, McpbManifest)> {
    // Use existing resolve_tool_path which handles ambiguity
    let tool_path = resolve_tool_path(tool).await?;
    let resolved = load_tool_from_path(&tool_path)?;

    Ok((resolved.plugin_ref, tool_path, resolved.template))
}
```

If user runs `tool config set filesystem` and both `appcypher/service` and `other/filesystem` exist:

```
error: Ambiguous reference 'filesystem'

  Found multiple matches:
    - appcypher/service
    - other/filesystem

  hint: Did you mean one of: appcypher/service, other/filesystem?
```

### Step 9: Implement Output Formats

**Human-readable `get` output:**
```
  Tool: appcypher/service

    api_key      sk-xxx...xxx  (sensitive)
    workspace    /home/user/code
    max_results  50

    Path: ~/.tool/config/appcypher/service/config.json
```

**Human-readable `get` for specific key:**
```
  appcypher/service.workspace = /home/user/code
```

**Concise `get` output (-c):**
```
#key	value	sensitive
api_key	sk-***	true
workspace	/home/user/code	false
max_results	50	false
```

**Concise `get` for specific key (-c):**
```
/home/user/code
```

**Human-readable `list` output:**
```
  Configured tools:

    appcypher/service    3 keys    ~/.tool/config/appcypher/service/config.json
    github-mcp              1 key     ~/.tool/config/github-mcp/config.json
```

**Concise `list` output (-c):**
```
#tool	keys	path
appcypher/service	3	/Users/x/.tool/config/appcypher/service/config.json
github-mcp	1	/Users/x/.tool/config/github-mcp/config.json
```

**Human-readable `set` success:**
```
  ✓ Configuration saved for appcypher/service

    api_key      sk-xxx...xxx
    workspace    /home/user/code
```

**Concise `set` success (-c):**
```
ok
```

**Human-readable `unset` success:**
```
  ✓ Removed 'api_key' from appcypher/service
```

**Human-readable `reset` success:**
```
  ✓ Removed all configuration for appcypher/service
```

## File Changes Summary

| File | Change |
|------|--------|
| `lib/constants.rs` | Add `DEFAULT_CONFIG_PATH` constant |
| `lib/commands.rs` | Add `Config` variant and `ConfigCommand` enum |
| `lib/handlers/tool/mod.rs` | Add `config_cmd` module and exports |
| `lib/handlers/tool/config_cmd.rs` | **New file** - config command implementation |
| `lib/handlers/tool/call.rs` | Update `parse_user_config()` to load saved config |
| `lib/handlers/tool/info.rs` | Update to pass tool_ref to parse_user_config |
| `bin/tool.rs` | Add `Command::Config` match arm |
| `lib/lib.rs` | Export `ConfigCommand` |

## Edge Cases

1. **Tool not found** - Error with suggestion to install or check reference
2. **Ambiguous reference** - Error listing matches, same as `tool info`/`tool call`
3. **Tool has no user_config schema** - "Tool has no configurable options"
4. **Invalid key in -y mode** - Warn if key not in schema (allow but warn)
5. **Sensitive fields** - Mask in `get` output (show `***` or truncated), password input in interactive
6. **Config file permissions** - Create directories with 0755, files with 0644
7. **Non-interactive without -y** - If stdin is not a TTY and no values provided, error with hint
8. **Empty values** - Allow setting empty string to clear a value
9. **`get` on unconfigured tool** - Show "No configuration saved" with schema fields if available
10. **`unset` non-existent key** - Warn but don't error
11. **`reset` unconfigured tool** - Warn but don't error
12. **Versioned reference** - `tool config set foo@1.0.0` stores config as `foo` (without version)
13. **Number out of range** - Error with min/max bounds in message
14. **Invalid boolean** - Error if not `true`/`false`

## Behavior Matrix

| Scenario | Behavior |
|----------|----------|
| `tool config set foo` (TTY) | Prompt for all fields, show existing/defaults |
| `tool config set foo` (non-TTY) | Error: provide values with -y or run interactively |
| `tool config set foo -y` | Error: no values provided |
| `tool config set foo -y a=b` | Set a=b, no prompts |
| `tool config set foo a=b` (TTY) | Set a=b, prompt for remaining fields |
| `tool config set foo a=b` (non-TTY) | Error: use -y for non-interactive |
| `tool config set foo@1.0.0 -y a=b` | Set a=b for `foo` (version stripped) |
| `tool config get foo` | Display all saved config |
| `tool config get foo key` | Display specific key value |
| `tool config list` | List all configured tools |
| `tool config unset foo key` | Remove key from config |
| `tool config reset foo` | Delete config.json |
