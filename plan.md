# Tool Info & Grep Drill-Down Plan

## Status: Implemented

## Overview

Add drill-down capabilities to `tool info` and `tool grep` commands, allowing users to focus on specific tools/methods and specific schema areas (input, output, description, name).

---

## Part 1: `tool info` Drill-Down

### Current Behavior
```bash
tool info open-data-mcp              # shows all tools, prompts, resources
tool info open-data-mcp --tools      # shows only tools section
tool info open-data-mcp --prompts    # shows only prompts section
tool info open-data-mcp --resources  # shows only resources section
```

### New Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--method` | `-m` | Focus on a specific tool/method by name |
| `--input` | | Show only input schema |
| `--output` | | Show only output schema |
| `--description` | `-d` | Show only description |
| `--name` | `-n` | Show only name (useful in concise mode) |

### Usage Examples

```bash
# Drill down to specific method
tool info open-data-mcp -m convert_currency

# Show only input schema for a method
tool info open-data-mcp -m convert_currency --input

# Show only output schema
tool info open-data-mcp -m convert_currency --output

# Show only description
tool info open-data-mcp -m convert_currency -d

# Combine with concise mode
tool info open-data-mcp -m convert_currency --input -c
```

### Output Examples

**Default (no focus flags):**
```
  ✓ Connected to open-data-mcp v0.1.0

    Tool: convert_currency

      Converts currency from one to another.

      ├── Input
      │   ├── amount*              number      The amount to convert
      │   ├── from_currency*       string      Source currency code
      │   └── to_currency*         string      Target currency code
      └── Output
          └── converted_amount*    number      The converted amount
```

**With `--input`:**
```
  convert_currency Input

    ├── amount*              number      The amount to convert
    ├── from_currency*       string      Source currency code
    └── to_currency*         string      Target currency code
```

**With `--input -c` (concise):**
```
#param	type	required	description
amount	number	true	The amount to convert
from_currency	string	true	Source currency code
to_currency	string	true	Target currency code
```

**With `-d` (description only):**
```
Converts currency from one to another.
```

### Implementation

1. Add new flags to `tool info` command in `commands.rs`
2. Update `tool_info()` in `info.rs`:
   - If `-m` specified, filter to that tool only
   - If `--input`/`--output`/`-d`/`-n` specified, show only that part
3. Update concise output to respect focus flags

---

## Part 2: `tool grep` Drill-Down

### Current Flags (to be replaced)
```
-n, --name           Search tool names only
-d, --description    Search descriptions only
-p, --params         Search parameter names only
-l, --list           List matching paths only (no values)
```

### New Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--method` | `-m` | Search within a specific tool/method only |
| `--input` | | Search only in input schemas |
| `--output` | | Search only in output schemas |
| `--description` | `-d` | Search only in description fields |
| `--name` | `-n` | Search only in names/keys |
| `--list` | `-l` | List matching paths only (keep existing) |

### Flag Behavior

The focus flags (`--input`, `--output`, `-d`, `-n`) filter which paths are searched:

| Flag | Paths Searched |
|------|----------------|
| (none) | All searchable paths |
| `-n` | `$.*`, `$.*.tools.*`, `$.*.tools.*.input_schema.properties.*`, `$.*.tools.*.output_schema.properties.*` |
| `-d` | `$.*.description`, `$.*.tools.*.description`, `$.*.tools.*.input_schema.properties.*.description`, `$.*.tools.*.output_schema.properties.*.description` |
| `--input` | `$.*.tools.*.input_schema.properties.*`, `$.*.tools.*.input_schema.properties.*.description`, `$.*.tools.*.input_schema.properties.*.type` |
| `--output` | `$.*.tools.*.output_schema.properties.*`, `$.*.tools.*.output_schema.properties.*.description`, `$.*.tools.*.output_schema.properties.*.type` |

Flags can be combined:
- `--input -d` → search descriptions within input schemas only
- `--input -n` → search field names within input schemas only

### Usage Examples

```bash
# Search all (current behavior)
tool grep currency

# Search within specific method (across all servers)
tool grep amount -m convert_currency

# Search only in input schemas
tool grep amount --input

# Search only in output schemas
tool grep converted --output

# Search only descriptions
tool grep "random integer" -d

# Search only names/keys
tool grep currency -n

# Combine: search input field names only
tool grep amount --input -n

# Combine: search input field descriptions only
tool grep "the amount" --input -d

# With list mode
tool grep currency -l
tool grep currency --input -l
```

### Output Examples

**`tool grep amount --input`:**
```
  ✓ Found 3 matches for pattern: amount

  open-data-mcp
    .tools.convert_currency.input_schema.properties.amount
      [key]
        "amount"
      .description
        "The amount to convert"
```

**`tool grep amount --input -n`:**
```
  ✓ Found 1 match for pattern: amount

  open-data-mcp
    .tools.convert_currency.input_schema.properties.amount
      [key]
        "amount"
```

**`tool grep amount -m convert_currency`:**
```
  ✓ Found 2 matches for pattern: amount

  open-data-mcp
    .tools.convert_currency.input_schema.properties.amount
      [key]
        "amount"
      .description
        "The amount to convert"
    .tools.convert_currency.output_schema.properties.converted_amount
      [key]
        "converted_amount"
```

### Implementation

1. Update flags in `tool grep` command in `commands.rs`:
   - Remove old `-p/--params` flag
   - Add `-m/--method` flag
   - Add `--input` and `--output` flags
   - Keep `-n`, `-d`, `-l` but with new semantics

2. Update `tool_grep()` in `grep.rs`:
   - Add method filter logic (if `-m` specified, only search within that tool)
   - Add path filtering based on focus flags
   - Implement flag combination logic

3. Update search logic:
   - Before searching, build list of allowed path patterns based on flags
   - Filter matches to only include paths matching allowed patterns

---

## Part 3: Shared Logic

Both commands share the concept of "focus areas":
- `--input` → input schema
- `--output` → output schema
- `-d/--description` → descriptions
- `-n/--name` → names/keys

Consider extracting shared types/utilities:

```rust
/// Focus area flags for drilling down into tool info
#[derive(Default)]
pub struct FocusFlags {
    pub input: bool,
    pub output: bool,
    pub description: bool,
    pub name: bool,
}

impl FocusFlags {
    /// Returns true if no focus flags are set (show/search all)
    pub fn is_all(&self) -> bool {
        !self.input && !self.output && !self.description && !self.name
    }
}
```

---

## Implementation Order

1. ✅ **Update `commands.rs`**
   - Add `-m/--method` to `tool info`
   - Add `--input`, `--output` to `tool info`
   - Add `-d` to `tool info`
   - Update `tool grep` flags (remove `-p`, add `-m`, `--input`, `--output`)

2. ⏭️ **Add shared types** (skipped - not needed)
   - FocusFlags struct not required for current implementation

3. ✅ **Update `tool info`**
   - Implement method filtering with `-m`
   - Implement focus flag output filtering (`--input`, `--output`, `-d`)

4. ✅ **Update `tool grep`**
   - Implement method filtering with `-m`
   - Implement focus flag search filtering (`--input`, `--output`, `-n`, `-d`)
   - Update path matching logic

5. ✅ **Update concise output**
   - Focus flags work properly with `-c` mode

6. ⏭️ **Update tests**
   - Manual testing completed

---

## Breaking Changes

- `tool grep -p/--params` removed (use `--input -n` or `--output -n` instead)
- `tool grep -n` semantic change: now searches all name/key positions, not just tool names
- `tool grep -d` semantic change: now searches all description fields at all levels

---

## Migration Guide

| Old Command | New Command |
|-------------|-------------|
| `tool grep foo -n` | `tool grep foo -n` (same flag, broader scope) |
| `tool grep foo -d` | `tool grep foo -d` (same flag, broader scope) |
| `tool grep foo -p` | `tool grep foo --input -n` |
