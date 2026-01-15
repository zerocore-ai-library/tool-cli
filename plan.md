# Improved Detection Confidence Scoring

## Problem

Current detection confidence is shallow - it primarily checks for SDK/dependency presence and entry point existence. This can lead to:
- False positives (SDK installed but not actually an MCP server)
- Low confidence on valid projects that just haven't been built yet
- No differentiation between mature servers and scaffolded stubs

## Current Approach

| Detector | Base Score | Additional Signals |
|----------|-----------|-------------------|
| Node | 0.70 (SDK) | +0.20 entry exists, +0.10 inferred, +0.05 TypeScript |
| Python | 0.70 (MCP dep) | +0.20 entry/script, +0.05 pyproject.toml |
| Rust | 0.80 (rmcp) | +0.15 built binary |

**Max possible scores:** Node ~0.95, Python ~0.95, Rust ~0.95

## Proposed Approach

### Design Principles

1. **SDK presence is necessary but not sufficient** - Lower base score, require actual usage
2. **Actual MCP patterns are strong indicators** - Grep for server creation, tool/prompt definitions
3. **Entry point confidence is tiered** - Existing file > inferred from config > common patterns
4. **Negative signals matter** - Penalize ambiguous or test-only projects

### Signal Categories

#### Tier 1: Required (Gates Detection)
Without these, return `None` (no detection):
- Has MCP SDK/dependency in project config

#### Tier 2: Core Signals (High Weight)

| Signal | Weight | Description |
|--------|--------|-------------|
| Creates server instance | +0.20 | `FastMCP()`, `Server.create()`, `Server::new()` |
| Entry point file exists | +0.15 | The detected entry point is present on disk |
| Defines MCP tools | +0.10 | `@mcp.tool`, `.tool()`, `#[tool]` patterns |

#### Tier 3: Supporting Signals (Medium Weight)

| Signal | Weight | Description |
|--------|--------|-------------|
| Entry point inferred | +0.08 | From package.json/pyproject.toml but file missing |
| Defines MCP prompts | +0.05 | `@mcp.prompt`, `.prompt()`, `#[prompt]` |
| Defines MCP resources | +0.05 | Resource handlers present |
| Has CLI entry configured | +0.05 | bin in package.json, [project.scripts], [[bin]] |
| Transport explicitly configured | +0.05 | Not relying on default stdio |

#### Tier 4: Weak Signals (Low Weight)

| Signal | Weight | Description |
|--------|--------|-------------|
| Modern project structure | +0.03 | TypeScript, pyproject.toml, workspace Cargo.toml |
| Multiple tools/prompts | +0.02 | More than 2 tools or prompts defined |

#### Negative Signals (Reduce Confidence)

| Signal | Weight | Description |
|--------|--------|-------------|
| MCP in devDependencies only | -0.15 | Likely for testing, not production server |
| Entry point in test/ directory | -0.15 | Probably test code |
| No server instantiation found | -0.10 | Has SDK but doesn't create server |
| Multiple runtimes with MCP | -0.10 | Both package.json and Cargo.toml have MCP deps |

### Proposed Base Scores

| Detector | New Base | Rationale |
|----------|----------|-----------|
| Node | 0.50 | Lower base, earn confidence through usage patterns |
| Python | 0.50 | Same |
| Rust | 0.55 | Slightly higher - rmcp is more specific than generic "mcp" |

### Confidence Formula

```rust
fn calculate_confidence(&self, dir: &Path, base_signals: &BaseSignals) -> f32 {
    let mut score = self.base_score(); // 0.50-0.55

    // Tier 2: Core signals
    if self.creates_server_instance(dir) {
        score += 0.20;
    }
    if base_signals.entry_point_exists {
        score += 0.15;
    }
    if self.defines_tools(dir) {
        score += 0.10;
    }

    // Tier 3: Supporting signals
    if base_signals.entry_point_inferred && !base_signals.entry_point_exists {
        score += 0.08;
    }
    if self.defines_prompts(dir) {
        score += 0.05;
    }
    if self.defines_resources(dir) {
        score += 0.05;
    }
    if base_signals.has_cli_entry {
        score += 0.05;
    }
    if self.has_explicit_transport(dir) {
        score += 0.05;
    }

    // Tier 4: Weak signals
    if base_signals.modern_structure {
        score += 0.03;
    }
    if self.tool_count(dir) > 2 || self.prompt_count(dir) > 2 {
        score += 0.02;
    }

    // Negative signals
    if base_signals.is_dev_dependency_only {
        score -= 0.15;
    }
    if base_signals.entry_in_test_dir {
        score -= 0.15;
    }
    if !self.creates_server_instance(dir) {
        score -= 0.10;
    }

    score.clamp(0.0, 1.0)
}
```

### Detection Patterns by Runtime

#### Node.js

```rust
// Server instantiation
const SERVER_PATTERNS: &[&str] = &[
    r"new\s+Server\s*\(",
    r"Server\.create\s*\(",
    r"createServer\s*\(",
];

// Tool definitions
const TOOL_PATTERNS: &[&str] = &[
    r"\.tool\s*\(",
    r"server\.setRequestHandler\s*\(\s*ListToolsRequestSchema",
    r"tools:\s*\[",
];

// Prompt definitions
const PROMPT_PATTERNS: &[&str] = &[
    r"\.prompt\s*\(",
    r"server\.setRequestHandler\s*\(\s*ListPromptsRequestSchema",
];
```

#### Python

```rust
// Server instantiation
const SERVER_PATTERNS: &[&str] = &[
    r"FastMCP\s*\(",
    r"Server\s*\(",
    r"mcp\.server\.Server\s*\(",
];

// Tool definitions
const TOOL_PATTERNS: &[&str] = &[
    r"@\w+\.tool",
    r"@tool\s*\(",
    r"\.add_tool\s*\(",
];

// Prompt definitions
const PROMPT_PATTERNS: &[&str] = &[
    r"@\w+\.prompt",
    r"@prompt\s*\(",
    r"\.add_prompt\s*\(",
];
```

#### Rust

```rust
// Server instantiation
const SERVER_PATTERNS: &[&str] = &[
    r"Server::new\s*\(",
    r"ServerBuilder::new\s*\(",
    r"\.serve\s*\(",
];

// Tool definitions
const TOOL_PATTERNS: &[&str] = &[
    r"#\[tool\]",
    r"#\[derive\([^)]*Tool[^)]*\)\]",
    r"impl\s+Tool\s+for",
];

// Prompt definitions
const PROMPT_PATTERNS: &[&str] = &[
    r"#\[prompt\]",
    r"impl\s+Prompt\s+for",
];
```

### Expected Confidence Ranges

| Project State | Expected Score | User Experience |
|---------------|----------------|-----------------|
| SDK only, no usage | 0.40-0.50 | Warn: "Low confidence - no MCP server code detected" |
| SDK + server instance | 0.65-0.75 | Normal init flow |
| SDK + server + tools + entry exists | 0.85-0.95 | High confidence, minimal prompts |
| SDK in devDeps only | 0.35-0.45 | Warn: "MCP appears to be a dev dependency" |

### Implementation Plan

1. **Add pattern detection utilities**
   - `has_server_instantiation(dir, patterns) -> bool`
   - `count_tool_definitions(dir, patterns) -> usize`
   - `count_prompt_definitions(dir, patterns) -> usize`

2. **Refactor each detector**
   - Lower base scores
   - Add pattern-based signal detection
   - Implement negative signal checks

3. **Update DetectionResult**
   - Add `signals: Vec<DetectionSignal>` for transparency
   - Each signal explains why confidence was adjusted

4. **Improve user feedback**
   - Show detected signals in `tool detect` output
   - Warn on low confidence with specific reasons

### Open Questions

1. **Should we cache grep results?** Multiple pattern searches on same files could be expensive
2. **How to handle monorepos?** Multiple server implementations in subdirectories
3. **Threshold for proceeding?** Should `tool init` refuse below certain confidence?
