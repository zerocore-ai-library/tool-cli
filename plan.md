# CLI Output Formatting Refactor Plan

## Goal

Flatten decorative indentation in human-readable CLI output to a single consistent level, using middle dots (`·`) to denote detail/metadata lines.

## Rules

1. **Single indentation level**: All human-readable output uses 2-space base indentation
2. **Middle dot prefix** (`·`): Used for unmarked detail/metadata lines that follow a header
3. **No dot for marked lines**: Lines with existing markers (`✓`, `✗`, `→`, `!`, `+`, `-`, `~`, `1.`, `2.`, etc.) do NOT get a dot
4. **Preserved formatting**: Tree structures (`├──`, `└──`), `--json`, `-c` concise mode, and semantic schema nesting remain unchanged

## Marker Reference

| Marker | Meaning |
|--------|---------|
| `✓` | Success |
| `✗` | Error/failure |
| `→` | In progress / action |
| `!` | Warning |
| `+` | Addition |
| `-` | Removal |
| `~` | Update/change |
| `·` | Detail/metadata (new) |
| `1.` `2.` | Numbered steps |

---

## Files to Modify

### 1. `lib/handlers/tool/registry.rs`

**Lines**: 85-93, 104-108, 152-156, 191-196, and surrounding output

**Current:**
```
  → Resolving appcypher/bash
    Version: 1.0.0
    Source: registry

  → Downloading appcypher/bash@1.0.0
  ✓ Downloaded to ~/.tool/packages/appcypher/bash (4.3 MB)
```

**New:**
```
  → Resolving appcypher/bash
  · Version: 1.0.0
  · Source: registry

  → Downloading appcypher/bash@1.0.0
  ✓ Downloaded to ~/.tool/packages/appcypher/bash (4.3 MB)
```

**Changes:**
- Metadata lines after headers: `"    {}"` → `"  · {}"`

---

### 2. `lib/handlers/tool/init.rs`

**Lines**: 355-372, 420-465, 543-545, 684-689, 711-756, 804, 819-842, 898-909

**Current:**
```
  ✓ Detected Python MCP server

    Type         python
    Transport    stdio
    Entry        src/main.py
    Package      uv
    Confidence   85%
    Build        uv run python src/main.py

  Files to create:
    manifest.json
    .mcpbignore

  ✓ Created manifest.json (mcpbx)
  ✓ Created .mcpbignore

  Next steps:
    1. tool build ./my-tool
    2. tool info ./my-tool
    3. tool call ./my-tool -m hello
```

**New:**
```
  ✓ Detected Python MCP server
  · Type         python
  · Transport    stdio
  · Entry        src/main.py
  · Package      uv
  · Confidence   85%
  · Build        uv run python src/main.py

  Files to create:
  · manifest.json
  · .mcpbignore

  ✓ Created manifest.json (mcpbx)
  ✓ Created .mcpbignore

  Next steps:
  1. tool build ./my-tool
  2. tool info ./my-tool
  3. tool call ./my-tool -m hello
```

**Changes:**
- Metadata labels: `"    {:<12} {}"` → `"  · {:<12} {}"`
- File list items: `"    manifest.json"` → `"  · manifest.json"`
- Section headers: `"  {}:"` stays as-is (no dot)
- Numbered steps: `"    {}. {}"` → `"  {}. {}"` (no dot, already has number marker)
- Success lines with `✓`: no change (already has marker)

---

### 3. `lib/handlers/tool/info.rs`

**Lines**: 64-85, 178-192, 215, 336, 392

**Current:**
```
  ✓ Connected to bash v1.0.0

    Type       stdio
    Location   ~/.tool/packages/appcypher/bash

    Tools:
      exec
      read_file

  ✗ Entry point not found: target/release/my-tool

    The tool needs to be built before it can be run.

    To build:
      cd /path/to/my-tool && tool build

    Runs: cargo build --release
```

**New:**
```
  ✓ Connected to bash v1.0.0
  · Type       stdio
  · Location   ~/.tool/packages/appcypher/bash

  Tools:
  · exec
  · read_file

  ✗ Entry point not found: target/release/my-tool
  · The tool needs to be built before it can be run.

  To build:
  · cd /path/to/my-tool && tool build
  · Runs: cargo build --release
```

**Changes:**
- Metadata: `"    {}       {}"` → `"  · {}       {}"`
- Section headers: `"    {}:"` → `"  {}:"`
- List items under sections: `"      {}"` → `"  · {}"`
- Explanation text: `"    The tool..."` → `"  · The tool..."`
- Command hints: `"      cd ..."` → `"  · cd ..."`

---

### 4. `lib/handlers/tool/call.rs`

**Lines**: 100-120, 209-222, 235-246, 252-261

**Current:**
```
  ✗ Entry point not found: target/release/my-tool

    The tool needs to be built before it can be run.

    To build:
      cd /path/to/my-tool && tool build

    Runs: cargo build --release

  ✓ Called exec on bash

    {
      "output": "file1.txt\nfile2.txt"
    }

    [Image: 1024 bytes]
    [Audio: 2048 bytes]
```

**New:**
```
  ✗ Entry point not found: target/release/my-tool
  · The tool needs to be built before it can be run.

  To build:
  · cd /path/to/my-tool && tool build
  · Runs: cargo build --release

  ✓ Called exec on bash
  · {
  ·   "output": "file1.txt\nfile2.txt"
  · }
  · [Image: 1024 bytes]
  · [Audio: 2048 bytes]
```

**Changes:**
- Explanation: `"    The tool..."` → `"  · The tool..."`
- Section headers: `"    To build:"` → `"  To build:"`
- Commands: `"      cd ..."` → `"  · cd ..."`
- Output content lines: `"    {}"` → `"  · {}"`
- Media annotations: `"    [Image: ...]"` → `"  · [Image: ...]"`

---

### 5. `lib/handlers/tool/validate_cmd.rs`

**Lines**: 77-83, 93, 117-131, 141-184

**Current:**
```
  Validating my-tool (mcpbx)

  error: → manifest.json
      ├─ Missing required field 'name'
      └─ help: Add "name": "your-tool-name"

  ✗ 1 error

  ✓ valid
```

**New:**
```
  Validating my-tool (mcpbx)

  error: → manifest.json
  · Missing required field 'name'
  · help: Add "name": "your-tool-name"

  ✗ 1 error

  ✓ valid
```

**Changes:**
- Issue details: `"      ├─ {}"` → `"  · {}"`
- Help text: `"      └─ help: {}"` → `"  · help: {}"`
- Remove tree characters (`├─`, `└─`) from validation output — not semantic here

---

### 6. `lib/handlers/tool/detect_cmd.rs`

**Lines**: 57-76, 147-193, 224-230, 246-293, 324-411

**Current:**
```
  ✓ Detected Python MCP server

    Type         python
    Transport    stdio
    Entry        src/main.py
    Package      uv
    Confidence   85%

    Signals
      ✓ Found pyproject.toml                     +dependency
      ✓ Uses mcp SDK                             +import
      ✗ No manifest.json found                   -missing

  Files to create:
    manifest.json
    .mcpbignore

  ✓ Created manifest.json
  ✓ Created .mcpbignore

  Next steps:
    1. tool build ./my-tool
    2. tool info ./my-tool
```

**New:**
```
  ✓ Detected Python MCP server
  · Type         python
  · Transport    stdio
  · Entry        src/main.py
  · Package      uv
  · Confidence   85%

  Signals
  ✓ Found pyproject.toml                     +dependency
  ✓ Uses mcp SDK                             +import
  ✗ No manifest.json found                   -missing

  Files to create:
  · manifest.json
  · .mcpbignore

  ✓ Created manifest.json
  ✓ Created .mcpbignore

  Next steps:
  1. tool build ./my-tool
  2. tool info ./my-tool
```

**Changes:**
- Metadata: `"    {:<12} {}"` → `"  · {:<12} {}"`
- Section headers: `"\n    {}"` → `"\n  {}"`
- Signal lines with `✓`/`✗`: `"      {} ..."` → `"  {} ..."` (no dot, has marker)
- File list: `"    manifest.json"` → `"  · manifest.json"`
- Numbered steps: `"    {}. {}"` → `"  {}. {}"` (no dot, has number)

---

### 7. `lib/handlers/tool/config_cmd.rs`

**Lines**: 168-209, 314-326, 340, 365-381, 415-426

**Current:**
```
  ✓ Configuration saved for appcypher/bash

    API_KEY              ••••••••
    TIMEOUT              30

  No configuration saved for appcypher/weather

    Available config fields:
      api_key              API key for auth          (required)
      timeout              Request timeout           (optional)

  Tool: appcypher/bash

    API_KEY              ••••••••
    Path: ~/.tool/config/appcypher/bash.json

  Configured tools:

    appcypher/bash                 2 keys  ~/.tool/config/...
    appcypher/weather              1 keys  ~/.tool/config/...
```

**New:**
```
  ✓ Configuration saved for appcypher/bash
  · API_KEY              ••••••••
  · TIMEOUT              30

  No configuration saved for appcypher/weather

  Available config fields:
  · api_key              API key for auth          (required)
  · timeout              Request timeout           (optional)

  Tool: appcypher/bash
  · API_KEY              ••••••••
  · Path: ~/.tool/config/appcypher/bash.json

  Configured tools:
  · appcypher/bash                 2 keys  ~/.tool/config/...
  · appcypher/weather              1 keys  ~/.tool/config/...
```

**Changes:**
- Config key-value pairs: `"    {:<20} {}"` → `"  · {:<20} {}"`
- Section headers: `"    Available config fields:"` → `"  Available config fields:"`
- Field list: `"      {:<20} {}"` → `"  · {:<20} {}"`
- Tool list items: `"    {:<30} {}"` → `"  · {:<30} {}"`
- Path metadata: `"    {}: {}"` → `"  · {}: {}"`

---

### 8. `lib/handlers/tool/host_cmd.rs`

**Lines**: 74-78, 130-154, 207-220, and surrounding output

**Current:**
```
  ! No tools to add. Install tools first with tool install.

  → Would modify: ~/.config/claude/claude_desktop_config.json

    + appcypher/bash    (new)
    ~ appcypher/weather (update)

  ✓ Added 2 tool(s) to Claude Desktop

    + appcypher/bash
    + appcypher/weather

    Backup: ~/.config/claude/claude_desktop_config.json.bak
```

**New:**
```
  ! No tools to add. Install tools first with tool install.

  → Would modify: ~/.config/claude/claude_desktop_config.json
  + appcypher/bash    (new)
  ~ appcypher/weather (update)

  ✓ Added 2 tool(s) to Claude Desktop
  + appcypher/bash
  + appcypher/weather
  · Backup: ~/.config/claude/claude_desktop_config.json.bak
```

**Changes:**
- Lines with `+`/`~`/`-`: `"    {} {}"` → `"  {} {}"` (no dot, has marker)
- Backup info: `"    {}: {}"` → `"  · {}: {}"`

---

### 9. `lib/handlers/tool/scripts.rs`

**Lines**: 59, 103-106, 111-112

**Current:**
```
  Running: npm run build

  Available scripts:
    build    npm run build
    test     npm test
    lint     eslint .

  No scripts defined in manifest.json
  Add scripts to _meta.store.tool.mcpb.scripts
```

**New:**
```
  Running: npm run build

  Available scripts:
  · build    npm run build
  · test     npm test
  · lint     eslint .

  No scripts defined in manifest.json
  · Add scripts to _meta.store.tool.mcpb.scripts
```

**Changes:**
- Script list items: `"    {} {}"` → `"  · {} {}"`
- Hint text: `"  Add scripts..."` → `"  · Add scripts..."`

---

### 10. `lib/handlers/auth.rs`

**Lines**: 112-127, 282-310

**Current:**
```
  ✓ Authenticated
    User: @username
    Registry: https://registry.example.com

  1. Go to https://...
  2. Sign in
  3. Copy the token
```

**New:**
```
  ✓ Authenticated
  · User: @username
  · Registry: https://registry.example.com

  1. Go to https://...
  2. Sign in
  3. Copy the token
```

**Changes:**
- Metadata after status: `"    {}: {}"` → `"  · {}: {}"`
- Numbered steps: keep as-is (already have markers)

---

### 11. `lib/handlers/tool/list.rs`

**Lines**: 306, 321-327, 376, 384, 391, 400, 410, 419, 441-447

**Current:**
```
  ✓ Found 3 tools

    bash
        Type       stdio
        Location   ~/.tool/packages/appcypher/bash

    Tools:
      exec
      read_file
    Prompts:
      shell_helper
    Resources:
      file://cwd
```

**New:**
```
  ✓ Found 3 tools

  bash
  · Type       stdio
  · Location   ~/.tool/packages/appcypher/bash

  Tools:
  · exec
  · read_file
  Prompts:
  · shell_helper
  Resources:
  · file://cwd
```

**Changes:**
- Tool name headers: `"    {}"` → `"  {}"`
- Metadata: `"        {}"` → `"  · {}"`
- Section headers: `"    {}:"` → `"  {}:"`
- List items: `"      {}"` → `"  · {}"`

---

### 12. `lib/handlers/tool/grep.rs`

**Lines**: 544-575

**Current:**
```
  bash
    tools.exec
      ├── command
      │     "Execute shell command"
      └── timeout
            "Timeout in milliseconds"
```

**Note:** This uses semantic tree structure for showing JSON paths. **Keep as-is** — this is not decorative indentation.

---

### 13. `lib/styles.rs`

**Lines**: 41-72 (Spinner)

The `Spinner::with_indent()` function uses variable indentation. Review usages to ensure spinners use consistent 2-space indent.

**Changes:**
- Default spinner indent should be 2
- Nested spinners (if any) should also be 2

---

## Implementation Checklist

- [ ] `lib/handlers/tool/registry.rs` — install/download/search output
- [ ] `lib/handlers/tool/init.rs` — project initialization output
- [ ] `lib/handlers/tool/info.rs` — tool inspection output
- [ ] `lib/handlers/tool/call.rs` — method invocation output
- [ ] `lib/handlers/tool/validate_cmd.rs` — validation output
- [ ] `lib/handlers/tool/detect_cmd.rs` — detection output
- [ ] `lib/handlers/tool/config_cmd.rs` — configuration output
- [ ] `lib/handlers/tool/host_cmd.rs` — host management output
- [ ] `lib/handlers/tool/scripts.rs` — script runner output
- [ ] `lib/handlers/auth.rs` — authentication output
- [ ] `lib/handlers/tool/list.rs` — list command output
- [ ] `lib/styles.rs` — spinner indentation

## Do NOT Modify

- Tree structure output (`├──`, `└──`) in `lib/tree.rs`
- Schema display with semantic nesting in `info.rs` (input/output trees)
- `--json` output paths
- `-c` concise/TSV output paths
- `lib/handlers/tool/grep.rs` — uses semantic tree for JSON paths

---

## Testing

After implementation, verify each command:

```bash
# Registry operations
tool search weather
tool install appcypher/bash
tool uninstall appcypher/bash

# Project operations
tool init test-project
tool detect ./some-project
tool validate
tool pack

# Tool operations
tool info appcypher/bash
tool call appcypher/bash -m exec command="ls"
tool list
tool list --full

# Config operations
tool config set appcypher/bash API_KEY=xxx
tool config get appcypher/bash
tool config list

# Host operations
tool host add claude-desktop --dry-run
tool host remove claude-desktop --dry-run

# Auth operations
tool whoami
tool login --token xxx

# Scripts
tool build
tool test
```

Verify that:
1. All output uses 2-space base indentation
2. Unmarked detail lines have `·` prefix
3. Marked lines (`✓`, `✗`, `→`, `+`, `-`, `1.`, etc.) have no dot
4. Tree structures and JSON output unchanged
