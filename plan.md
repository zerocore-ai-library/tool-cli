# Plan: Multi-Method Support for `tool info`

## Overview

Enable `-m` flag to accept multiple methods: `tool info . -m exec -m read_file`

## Changes

### 1. CLI Definition (`lib/commands.rs`)

Change the `method` argument from `Option<String>` to `Vec<String>`:

```rust
// Before
#[arg(short = 'm', long = "method")]
method: Option<String>,

// After
#[arg(short = 'm', long = "method")]
methods: Vec<String>,
```

Update the examples in `INFO_EXAMPLES` to show multi-method usage.

### 2. Binary Dispatch (`bin/tool.rs`)

Update the `Command::Info` match arm to pass `methods: Vec<String>` instead of `method: Option<String>`.

### 3. Handler Signature (`lib/handlers/tool/info.rs`)

Update `tool_info` function signature:

```rust
// Before
method: Option<String>,

// After
methods: Vec<String>,
```

### 4. Method Filtering Logic

Replace single method lookup with multi-method filtering:

```rust
// Before
if let Some(ref method_name) = method {
    let matching_tool = capabilities.tools.iter().find(|t| t.name == *method_name);
    ...
}

// After
if !methods.is_empty() {
    let matching_tools: Vec<&Tool> = capabilities.tools
        .iter()
        .filter(|t| methods.contains(&t.name))
        .collect();

    // Validate all requested methods exist
    for method_name in &methods {
        if !matching_tools.iter().any(|t| t.name == *method_name) {
            // Error: method not found
        }
    }
    ...
}
```

### 5. Output Functions

#### 5a. JSON Output (`output_method_json` -> `output_methods_json`)

Change from single object to object keyed by method name:

```rust
// Before: single method
fn output_method_json(tool: &Tool, concise: bool)

// After: multiple methods
fn output_methods_json(tools: &[&Tool], concise: bool)
```

Output structure:
```json
{
  "exec": {
    "description": "...",
    "input_schema": {...},
    "output_schema": {...}
  },
  "read_file": {
    "description": "...",
    "input_schema": {...},
    "output_schema": {...}
  }
}
```

#### 5b. Normal Output (`output_method_normal` -> `output_methods_normal`)

Loop through methods, output each with existing formatting:

```rust
fn output_methods_normal(
    tools: &[&Tool],
    input_only: bool,
    output_only: bool,
    description_only: bool,
    verbose: bool,
    level: usize,
)
```

Output remains the same per-method, just repeated for each.

#### 5c. Concise Output (`output_method_concise` -> `output_methods_concise`)

**Default (no drill-down flags):** One line per method, same as current.

```
#tool
appcypher/bash:exec(command*: string) -> {result*: string}
appcypher/bash:read_file(path*: string) -> {content*: string}
```

**With `--input` or `--output` (Option A):** Add method column to TSV:

```rust
// Before header
#param\ttype\trequired\tdescription

// After header
#method\tparam\ttype\trequired\tdescription
```

Output:
```
#method	param	type	required	description
exec	command	string	true	"The command to run"
read_file	path	string	true	"File path"
```

**With `-d` (description only):** Tab-separated method + description:

```
exec	Execute a command
read_file	Read contents of a file
```

### 6. Backward Compatibility

Single method usage remains the same:
- `tool info . -m exec` works as before
- Output format is consistent (always object-keyed for JSON, always has method column for concise drill-down)

## File Changes Summary

| File | Changes |
|------|---------|
| `lib/commands.rs` | Change `method: Option<String>` -> `methods: Vec<String>`, update examples |
| `bin/tool.rs` | Update dispatch to pass `methods` |
| `lib/handlers/tool/info.rs` | Update signature, filtering logic, all output functions |

## Testing

1. `tool info appcypher/bash -m exec` - single method (regression)
2. `tool info appcypher/bash -m exec -m read_file` - multiple methods
3. `tool info appcypher/bash -m exec -m read_file --json` - JSON object output
4. `tool info appcypher/bash -m exec -m read_file --input` - TSV with method column
5. `tool info appcypher/bash -m exec -m read_file --output` - TSV with method column
6. `tool info appcypher/bash -m exec -m read_file -d` - descriptions with method names
7. `tool info appcypher/bash -m nonexistent` - error handling
8. `tool info appcypher/bash -m exec -m nonexistent` - partial match error handling
