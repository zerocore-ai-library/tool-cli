# Tool Grep Redesign Plan

## Status: Implemented

All parts of this plan have been implemented.

## Overview

Redesign `tool grep` to search the unified JSON structure from `tool list --json --full`, returning matches with JavaScript accessor paths as locators.

---

## Part 1: Update `tool list --json` Structure

Change from array to object-keyed structure.

### Before
```json
[
  {
    "name": "steve/system",
    "type": "stdio",
    "description": "System utilities...",
    "location": "/path/to/..."
  }
]
```

### After
```json
{
  "steve/system": {
    "type": "stdio",
    "description": "System utilities...",
    "location": "/path/to/..."
  }
}
```

---

## Part 2: Update `tool info --json` Structure

Change from arrays to object-keyed structure.

### Before
```json
{
  "server": {"name": "rmcp", "version": "0.12.0"},
  "type": "manifest.json (stdio)",
  "manifest_path": "/path/to/manifest.json",
  "tools": [
    {
      "name": "system__sleep",
      "description": "Pause execution...",
      "input_schema": {...},
      "output_schema": {...}
    }
  ],
  "prompts": [],
  "resources": []
}
```

### After
```json
{
  "server": {"name": "rmcp", "version": "0.12.0"},
  "type": "manifest.json (stdio)",
  "manifest_path": "/path/to/manifest.json",
  "tools": {
    "system__sleep": {
      "description": "Pause execution...",
      "input_schema": {...},
      "output_schema": {...}
    }
  },
  "prompts": {},
  "resources": {}
}
```

### Changes
- `tools`: array → object keyed by tool name
- `prompts`: array → object keyed by prompt name
- `resources`: array → object keyed by resource name
- Remove `name` field from each item (now the key)

---

## Part 3: Add `tool list --full` Flag

Combine `tool list` and `tool info` into unified structure. Works with both `--json` and human-readable output.

### JSON Output (`tool list --json --full`)
```json
{
  "steve/system": {
    "type": "stdio",
    "description": "System utilities for AI agents (sleep, datetime, random)",
    "location": "/path/to/...",
    "server": {"name": "rmcp", "version": "0.12.0"},
    "tools": {
      "system__sleep": {
        "description": "Pause execution for a specified duration in milliseconds.",
        "input_schema": {...},
        "output_schema": {...}
      },
      "system__get_random_integer": {
        "description": "Generate a random integer within an inclusive range [min, max].",
        "input_schema": {...},
        "output_schema": {...}
      }
    },
    "prompts": {},
    "resources": {}
  }
}
```

### Human-Readable Output (`tool list --full`)
```
  ✓ Found 2 tools

    steve/system  System utilities for AI agents (sleep, datetime, random)
    stdio  /path/to/...

    Tools:
      system__sleep  Pause execution for a specified duration in milliseconds.
      system__get_random_integer  Generate a random integer within an inclusive range...
      system__get_datetime  Get the current UTC date and time.

    steve/filesystem  File system operations for AI agents
    stdio  /path/to/...

    Tools:
      filesystem__read  Read a file from the local filesystem.
      filesystem__write  Write content to a file.
```

### Implementation
- Add `--full` flag to `list` command (no short form)
- For each server, call `get_tool_info()` to fetch tools/prompts/resources
- Merge into unified object-keyed structure (JSON) or expanded list (human-readable)

---

## Part 4: Redesign `tool grep`

Search the `--full` structure and return matches with JavaScript accessor paths.

### What to Search

Grep matches on **specific keys** (names) and **specific values** (descriptions, types).

#### Searchable Keys (Names)

| Path Pattern | What it Represents |
|--------------|-------------------|
| `$.*` | Server names |
| `$.*.tools.*` | Tool names |
| `$.*.tools.*.input_schema.properties.*` | Input field names |
| `$.*.tools.*.output_schema.properties.*` | Output field names |

#### Searchable Values

| Path Pattern | What it Represents |
|--------------|-------------------|
| `$.*.description` | Server description |
| `$.*.tools.*.description` | Tool description |
| `$.*.tools.*.input_schema.properties.*.description` | Input field description |
| `$.*.tools.*.input_schema.properties.*.type` | Input field type |
| `$.*.tools.*.output_schema.properties.*.description` | Output field description |
| `$.*.tools.*.output_schema.properties.*.type` | Output field type |

#### Grep Logic

1. Traverse the `--full` JSON structure
2. At known "name" key paths → match pattern against the key
3. At known value paths → match pattern against the value
4. Build JavaScript accessor path for each match

### Path Format (JavaScript Accessor)

```
['steve/system']                                                          # server name (key match)
['steve/system'].description                                              # server description (value match)
['steve/system'].tools.system__get_random_integer                         # tool name (key match)
['steve/system'].tools.system__get_random_integer.description             # tool description (value match)
['steve/system'].tools.system__get_random_integer.input_schema.properties.min                 # field name (key match)
['steve/system'].tools.system__get_random_integer.input_schema.properties.min.description     # field description (value match)
['steve/system'].tools.system__get_random_integer.input_schema.properties.min.type            # field type (value match)
```

Rules:
- `['key']` for keys with special characters (e.g., `/`)
- `.key` for simple keys
- Nested schemas follow the actual JSON structure
- Path ending without `.property` indicates a key match (name)
- Path ending with `.description`, `.type` indicates a value match

---

## Part 5: Output Formats

### JSON (`tool grep random --json`)
```json
{
  "pattern": "random",
  "matches": [
    {
      "path": "['steve/system'].description",
      "value": "System utilities for AI agents (sleep, datetime, random)"
    },
    {
      "path": "['steve/system'].tools.system__get_random_integer",
      "value": "system__get_random_integer"
    },
    {
      "path": "['steve/system'].tools.system__get_random_integer.description",
      "value": "Generate a random integer within an inclusive range [min, max]."
    }
  ]
}
```

### Human-Readable (`tool grep random`)

Hierarchical output grouped by parent entity, with `[key]` markers for key matches:

```
  ✓ Found 10 matches for pattern: random

  open-data-mcp
    .tools.random_recipe
      [key]
        "random_recipe"
      .description
        "Get a random recipe."
    .tools.random_user
      [key]
        "random_user"
      .description
        "Generate random user profiles."

  steve/system
    .description
      "System utilities for AI agents (sleep, datetime, random)"
    .tools.system__get_random_integer
      [key]
        "system__get_random_integer"
      .description
        "Generate a random integer within an inclusive range [min,..."
```

Server-level key match:
```
  open-data-mcp
    [key]
      "open-data-mcp"
```

### Concise (`tool grep random -c`)
```
#path	value
['steve/system'].description	System utilities for AI agents (sleep, datetime, random)
['steve/system'].tools.system__get_random_integer	system__get_random_integer
['steve/system'].tools.system__get_random_integer.description	Generate a random integer...
```

### List Only (`tool grep random -l`)
```
['open-data-mcp'].tools.random_recipe
['open-data-mcp'].tools.random_recipe.description
['steve/system'].description
['steve/system'].tools.system__get_random_integer
['steve/system'].tools.system__get_random_integer.description
```

---

## Part 6: Update `lib/output.rs`

Update the reusable output types to use object-keyed structures.

### Changes to `ToolListOutput`
- Change from struct to `BTreeMap<String, ServerOutput>`

### New `ServerOutput`
```rust
pub struct ServerOutput {
    #[serde(rename = "type")]
    pub server_type: String,
    pub description: Option<String>,
    pub location: String,
}
```

### Changes to `ToolInfoOutput`
- `tools`: `Vec<ToolOutput>` → `BTreeMap<String, ToolOutput>`
- `prompts`: `Vec<PromptOutput>` → `BTreeMap<String, PromptOutput>`
- `resources`: `Vec<ResourceOutput>` → `BTreeMap<String, ResourceOutput>`
- Remove `name` field from `ToolOutput`, `PromptOutput`, `ResourceOutput`

### New `FullServerOutput`
```rust
/// Full server info for --full flag
pub struct FullServerOutput {
    #[serde(rename = "type")]
    pub server_type: String,
    pub description: Option<String>,
    pub location: String,
    pub server: ToolServerInfo,
    pub tools: BTreeMap<String, ToolOutput>,
    pub prompts: BTreeMap<String, PromptOutput>,
    pub resources: BTreeMap<String, ResourceOutput>,
}
```

### New Grep Types
```rust
/// Grep match result
pub struct GrepMatch {
    pub path: String,
    pub value: String,
}

/// Grep output
pub struct GrepOutput {
    pub pattern: String,
    pub matches: Vec<GrepMatch>,
}
```

### Path Building Utilities
```rust
fn js_path_server(server_name: &str) -> String
fn js_path_server_prop(server_name: &str, prop: &str) -> String
fn js_path_tool(server_name: &str, tool_name: &str) -> String
fn js_path_tool_prop(server_name: &str, tool_name: &str, prop: &str) -> String
fn js_path_schema_field(server_name: &str, tool_name: &str, schema_type: &str, field_path: &str) -> String
fn js_path_schema_field_prop(server_name: &str, tool_name: &str, schema_type: &str, field_path: &str, prop: &str) -> String
```

---

## Implementation Order

1. ✅ **Update `lib/output.rs`**
   - Change types to object-keyed structures
   - Add new types for grep output
   - Add path building utilities

2. ✅ **Update `tool list --json`**
   - Use new object-keyed structure
   - Update JSON output in `list.rs`

3. ✅ **Update `tool info --json`**
   - Use new object-keyed structure
   - Update `output_tool_info_json()` in `info.rs`

4. ✅ **Add `tool list --full`**
   - Add flag to command definition in `commands.rs`
   - Implement fetching and merging tool info in `list.rs`
   - Output unified structure (JSON and human-readable)

5. ✅ **Rewrite `tool grep`**
   - Remove old grep implementation
   - Implement new search against `--full` structure
   - Build JavaScript accessor paths
   - Implement JSON, human-readable (hierarchical), and concise output formats

6. ✅ **Fix ambiguous re-export**
   - Renamed `GrepMatch` in `detect/utils.rs` to `FileGrepMatch`

---

## Breaking Changes

- `tool list --json` changes from array to object
- `tool info --json` changes tools/prompts/resources from arrays to objects
- `tool grep` output format completely changes
