# Implementation Plan: `tool host` Command

## Overview

The `tool host` command configures external MCP hosts to use tools installed via tool-cli. It generates MCP server entries that invoke `tool run <tool-ref>`.

**Tier 1 Hosts:**
- Claude Desktop
- Cursor
- Claude Code

---

## Command Summary

```
tool host
├── add <host> [tools...]      # Register tools with a host
├── remove <host> [tools...]   # Unregister tools from a host
├── list                       # List all supported hosts
├── show <host>                # Show generated config preview
└── path <host>                # Print config file path
```

---

## Supported Hosts & Paths (Cross-Platform)

| Host | macOS | Windows | Linux |
|------|-------|---------|-------|
| **Claude Desktop** | `~/Library/Application Support/Claude/claude_desktop_config.json` | `%APPDATA%\Claude\claude_desktop_config.json` | `~/.config/Claude/claude_desktop_config.json` |
| **Cursor** | `~/.cursor/mcp.json` | `%USERPROFILE%\.cursor\mcp.json` | `~/.cursor/mcp.json` |
| **Claude Code** | `~/.claude.json` | `%USERPROFILE%\.claude.json` | `~/.claude.json` |

**Aliases:**
- `claude-desktop`, `cd` → Claude Desktop
- `cursor` → Cursor
- `claude-code`, `cc` → Claude Code

---

## Generated Config

MCP hosts only support **stdio** transport. MCPB tools can be stdio or HTTP.

`tool run --expose stdio` bridges any transport to stdio, making all tools compatible:

```json
{
  "mcpServers": {
    "namespace__toolname": {
      "command": "tool",
      "args": ["run", "--expose", "stdio", "namespace/toolname", "--yes"]
    }
  }
}
```

| Flag | Purpose |
|------|---------|
| `--expose stdio` | Bridge any transport (stdio/http) to stdio output |
| `--yes` | Skip interactive prompts (required for automated startup) |

---

## Safety Requirements

### 1. Backup Before Modification

```
~/.tool/backups/
├── claude-desktop/
│   └── 2024-01-15T10-30-00.json
├── cursor/
│   └── ...
└── claude-code/
    └── ...
```

- Create timestamped backup before ANY write
- Keep last 5 backups per host
- User can manually restore from `~/.tool/backups/<host>/`

### 2. Atomic Writes

- Write to temp file first (`config.json.tmp`)
- Validate temp file is valid JSON
- Rename temp → target (atomic)
- Clean up on failure

### 3. Preserve Existing Config

- Only modify `mcpServers` key
- Preserve all other keys untouched
- Preserve non-tool-cli entries in mcpServers

### 4. Confirmation

- `--dry-run` shows what would change
- Confirmation prompt before modification
- `--yes` to skip confirmation

---

## Files to Create

```
lib/
├── hosts.rs                      # Host definitions, paths, safety utilities
└── handlers/tool/
    └── host_cmd.rs               # Command handlers
```

## Files to Modify

```
lib/
├── commands.rs                   # Add HostCommand enum
├── handlers/tool/mod.rs          # Export host_cmd
├── constants.rs                  # Add backup paths
├── error.rs                      # Add host errors
├── lib.rs                        # Re-export hosts
bin/
└── tool.rs                       # Route command
```

---

## Command Definitions (`lib/commands.rs`)

```rust
#[derive(Debug, Clone, Subcommand)]
pub enum HostCommand {
    /// Register tools with an MCP host
    Add {
        /// Target host (claude-desktop, cursor, claude-code)
        host: String,

        /// Specific tools to register (default: all installed)
        tools: Vec<String>,

        /// Preview changes without modifying files
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing entries for these tools
        #[arg(long)]
        overwrite: bool,

        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,
    },

    /// Remove tools from an MCP host
    Remove {
        /// Target host
        host: String,

        /// Specific tools to remove (default: all tool-cli managed)
        tools: Vec<String>,

        /// Preview changes without modifying files
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,
    },

    /// List supported hosts and their status
    List,

    /// Show the MCP config that would be generated
    Show {
        /// Target host
        host: String,

        /// Specific tools (default: all installed)
        tools: Vec<String>,
    },

    /// Print config file path for a host
    Path {
        /// Target host
        host: String,
    },
}
```

---

## Host Definitions (`lib/hosts.rs`)

```rust
//--------------------------------------------------------------------------------------------------
// Types
//--------------------------------------------------------------------------------------------------

/// Supported MCP host applications
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpHost {
    ClaudeDesktop,
    Cursor,
    ClaudeCode,
}

/// Entry in the mcpServers config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub command: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
}

/// Metadata for tracking tool-cli managed entries
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostMetadata {
    pub managed_tools: Vec<String>,
}

//--------------------------------------------------------------------------------------------------
// Methods
//--------------------------------------------------------------------------------------------------

impl McpHost {
    pub fn from_str(s: &str) -> ToolResult<Self> {
        match s.to_lowercase().as_str() {
            "claude-desktop" | "claudedesktop" | "cd" => Ok(Self::ClaudeDesktop),
            "cursor" => Ok(Self::Cursor),
            "claude-code" | "claudecode" | "cc" => Ok(Self::ClaudeCode),
            _ => Err(ToolError::InvalidHost(s.to_string())),
        }
    }

    pub fn display_name(&self) -> &'static str { ... }
    pub fn canonical_name(&self) -> &'static str { ... }
    pub fn config_path(&self) -> ToolResult<PathBuf> { ... }  // Cross-platform
    pub fn all() -> &'static [McpHost] { ... }
    pub fn config_exists(&self) -> bool { ... }
}

//--------------------------------------------------------------------------------------------------
// Functions
//--------------------------------------------------------------------------------------------------

/// Create timestamped backup before modification
pub fn create_backup(host: &McpHost) -> ToolResult<Option<PathBuf>> { ... }

/// Load config, return empty object if file doesn't exist
pub fn load_config(host: &McpHost) -> ToolResult<Value> { ... }

/// Save config with atomic write (temp file + rename)
pub fn save_config(host: &McpHost, config: &Value) -> ToolResult<()> { ... }

/// Convert tool ref to server name: "ns/tool" -> "ns__tool"
pub fn tool_ref_to_server_name(tool_ref: &str) -> String {
    tool_ref.replace('/', "__").replace('@', "__")
}

/// Generate MCP server entry for a tool
/// Uses --expose stdio to bridge any transport (stdio/http) to stdio
/// Uses --yes to skip interactive prompts during automated startup
pub fn generate_server_entry(tool_ref: &str) -> McpServerEntry {
    McpServerEntry {
        command: "tool".to_string(),
        args: vec![
            "run".to_string(),
            "--expose".to_string(),
            "stdio".to_string(),
            tool_ref.to_string(),
            "--yes".to_string(),
        ],
        env: None,
    }
}
```

---

## Constants (`lib/constants.rs`)

```rust
pub static DEFAULT_BACKUPS_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    DEFAULT_HOME_PATH.join("backups")
});

pub static DEFAULT_HOSTS_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    DEFAULT_HOME_PATH.join("hosts")
});
```

---

## Error Types (`lib/error.rs`)

```rust
#[error("Unknown host '{0}'. Supported: claude-desktop, cursor, claude-code")]
InvalidHost(String),

#[error("Failed to parse {host} config: {message}")]
HostConfigParseError { host: String, message: String },
```

---

## Output Specification

### `tool host list`

**Normal mode:**
```
  ✓ Supported MCP hosts

    claude-desktop     2 tools    ~/Library/Application Support/Claude/claude_desktop_config.json
    cursor             0 tools    ~/.cursor/mcp.json (not found)
    claude-code        1 tool     ~/.claude.json
```

**Concise mode (`-c`):**
```
#host	tools	status	path
claude-desktop	2	configured	/Users/steve/Library/Application Support/Claude/claude_desktop_config.json
cursor	0	not_found	/Users/steve/.cursor/mcp.json
claude-code	1	configured	/Users/steve/.claude.json
```

---

### `tool host add <host> [tools...]`

**Normal mode:**
```
  ✓ Added 2 tool(s) to Claude Desktop

    + appcypher/filesystem
    + bash

    Backup: ~/.tool/backups/claude-desktop/2024-01-15T10-30-00.json
```

**With `--dry-run`:**
```
  → Would modify: ~/Library/Application Support/Claude/claude_desktop_config.json

    + appcypher/filesystem    (new)
    + bash                    (new)
    ~ existing-tool           (skip, already exists)

    Run without --dry-run to apply changes.
```

**Concise mode (`-c`):**
```
#action	tool
add	appcypher/filesystem
add	bash
skip	existing-tool
```

**Concise success (no `--dry-run`):**
```
ok	2
```

---

### `tool host remove <host> [tools...]`

**Normal mode:**
```
  ✓ Removed 2 tool(s) from Claude Desktop

    - appcypher/filesystem
    - bash

    Backup: ~/.tool/backups/claude-desktop/2024-01-15T10-35-00.json
```

**Concise mode (`-c`):**
```
#action	tool
remove	appcypher/filesystem
remove	bash
```

**Concise success:**
```
ok	2
```

---

### `tool host show <host>`

**Normal mode (pretty JSON):**
```json
{
  "mcpServers": {
    "appcypher__filesystem": {
      "command": "tool",
      "args": ["run", "--expose", "stdio", "appcypher/filesystem", "--yes"]
    },
    "bash": {
      "command": "tool",
      "args": ["run", "--expose", "stdio", "bash", "--yes"]
    }
  }
}
```

**Concise mode (`-c`) (minified JSON):**
```
{"mcpServers":{"appcypher__filesystem":{"command":"tool","args":["run","--expose","stdio","appcypher/filesystem","--yes"]}}}
```

---

### `tool host path <host>`

**Both modes (same output):**
```
/Users/steve/Library/Application Support/Claude/claude_desktop_config.json
```

---

### Error Output

**Normal mode:**
```
  ✗ Unknown host 'vscode'. Supported: claude-desktop, cursor, claude-code
```

**Concise mode (stderr):**
```
error	Unknown host 'vscode'. Supported: claude-desktop, cursor, claude-code
```

---

## Usage Examples

```bash
# List all supported hosts and status
tool host list
tool host list -c              # Concise TSV output

# Add all installed tools to Claude Desktop
tool host add claude-desktop
tool host add cd               # Using alias

# Add specific tools to Cursor
tool host add cursor appcypher/filesystem bash

# Preview changes (dry-run)
tool host add claude-code --dry-run

# Force overwrite existing entries
tool host add cursor --overwrite

# Skip confirmation
tool host add claude-desktop -y

# Show what config would be generated
tool host show claude-desktop
tool host show cd -c           # Minified JSON

# Get config file path
tool host path cursor

# Remove all tool-cli managed tools
tool host remove claude-desktop

# Remove specific tools
tool host remove cursor appcypher/filesystem
```

Backups stored in `~/.tool/backups/<host>/` for manual restore if needed.
