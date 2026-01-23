# Plan: `tool run` Proxy Mode with Protocol Bridging

## Summary

Repurpose `tool run` from running manifest scripts to running MCP servers in proxy mode with protocol bridging support (`--expose` flag). Scripts continue to work via `tool build`, `tool test`, etc. (External subcommand catch-all).

## Usage

```bash
tool run                          # Native transport (stdio→stdio, http→http)
tool run --expose stdio           # Expose as stdio (bridge if backend is HTTP)
tool run --expose http            # Expose as HTTP (bridge if backend is stdio)
tool run --expose http --port 8080 --host 0.0.0.0
tool run -k API_KEY=xxx           # With user config
```

## Architecture

```
[Client] ←─expose─→ [tool run proxy] ←─backend─→ [MCP Server]
                         │
                         ├─ Connects to backend as MCP client (existing code)
                         └─ Serves frontend as MCP server (new ServerHandler)
```

## Files to Modify

### 1. `Cargo.toml` - Enable rmcp server feature

Add `"server"` to rmcp features:
```toml
rmcp = { ..., features = [
    "client",
    "server",  # ADD
    ...
] }
```

### 2. `lib/commands.rs` - Redefine Run command

Replace current `Run` (scripts) with proxy mode. Config flags match `info`/`call` exactly:
```rust
Run {
    #[arg(default_value = ".")]
    tool: String,
    #[arg(long)]
    expose: Option<String>,  // "stdio" or "http"
    #[arg(long, default_value = "3000")]
    port: u16,
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(short = 'k', long)]
    config: Vec<String>,       // KEY=VALUE config
    #[arg(long)]
    config_file: Option<String>, // JSON config file
    #[arg(long)]
    no_save: bool,             // Don't auto-save config
    #[arg(short, long)]
    verbose: bool,
}
```

**Config flow (same as info/call):**
1. Load saved config from `~/.tool/config/...`
2. Merge config file (if `--config-file`)
3. Merge `-k` flags (highest priority)
4. Prompt interactively for missing required fields (if TTY)
5. Apply defaults from manifest
6. Auto-save to `~/.tool/config/...` (unless `--no-save`)

### 3. `lib/proxy.rs` - NEW: Core proxy implementation

New module with:
- `ExposeTransport` enum (Stdio, Http)
- `HttpExposeConfig` struct (port, host)
- `ProxyHandler` implementing rmcp's `ServerHandler` trait
- `run_proxy()` function to start server with specified transport
- `run_stdio_server()` - stdio frontend using `tokio::io::{stdin, stdout}`
- `run_http_server()` - HTTP frontend (axum already in deps)

The `ProxyHandler` forwards all MCP methods to the backend:
- `list_tools()` → `backend.peer().list_tools()`
- `call_tool()` → `backend.peer().call_tool()`
- `list_prompts()` → `backend.peer().list_prompts()`
- `get_prompt()` → `backend.peer().get_prompt()`
- `list_resources()` → `backend.peer().list_resources()`
- `read_resource()` → `backend.peer().read_resource()`

### 4. `lib/handlers/tool/run.rs` - NEW: Command handler

New handler `tool_run()` that:
1. Resolves tool path
2. Loads manifest
3. Parses user config (reuse `parse_user_config`, `prompt_missing_user_config`, `apply_user_config_defaults`)
4. Allocates system config (reuse `allocate_system_config`)
5. Resolves manifest with config
6. Connects to backend via `connect_with_oauth()`
7. Calls `run_proxy()` with backend connection and expose settings

### 5. `lib/handlers/tool/mod.rs` - Add run module

```rust
mod run;
pub use run::tool_run;
```

### 6. `lib/lib.rs` - Add proxy module

```rust
pub mod proxy;
```

### 7. `bin/tool.rs` - Update dispatch

```rust
Command::Run { tool, expose, port, host, config, config_file, verbose } => {
    handlers::tool_run(tool, expose, port, host, config, config_file, verbose).await
}
```

### 8. `lib/handlers/tool/scripts.rs` - Remove Run handling (optional cleanup)

The `run_script()` function can remain for `run_external_script()` which handles `tool build`, `tool test`, etc. via External catch-all. Just remove explicit `tool run <script>` support.

## Header Handling (HTTP→Stdio bridging)

When bridging HTTP backend to stdio frontend:
- Headers come from manifest `mcp_config.headers` (already resolved with variable substitution)
- OAuth handled by existing `connect_with_oauth()` flow
- Tokens stored/loaded automatically

Stdio clients don't need to know about headers - the proxy handles it.

## Key Code Reuse

### Shared Connection Setup Pattern

`info`, `call`, and `run` all follow the same setup pattern. Consider extracting to a shared helper:

```rust
// lib/handlers/tool/common.rs (NEW - optional refactor)
pub struct ResolvedTool {
    pub connection: McpConnection,
    pub manifest: McpbManifest,
    pub tool_name: String,
    pub transport: McpbTransport,
}

pub async fn setup_tool_connection(
    tool: &str,
    config: &[String],
    config_file: Option<&str>,
    no_save: bool,
    verbose: bool,
) -> ToolResult<ResolvedTool> {
    // 1. resolve_tool_path()
    // 2. load_tool_from_path()
    // 3. parse_user_config()
    // 4. prompt_missing_user_config()
    // 5. apply_user_config_defaults()
    // 6. allocate_system_config()
    // 7. manifest.resolve()
    // 8. connect_with_oauth()
}
```

This would let `run.rs` be very simple:
```rust
pub async fn tool_run(...) -> ToolResult<()> {
    let resolved = setup_tool_connection(&tool, &config, config_file.as_deref(), no_save, verbose).await?;
    run_proxy(resolved.connection, expose_transport, http_config, resolved.transport, verbose).await
}
```

### Existing Reusable Functions

| Function | Location | Purpose |
|----------|----------|---------|
| `connect_with_oauth()` | `lib/mcp.rs` | Connect to backend with OAuth |
| `McpConnection` | `lib/mcp.rs` | Backend connection wrapper |
| `parse_user_config()` | `lib/handlers/tool/call.rs` | Parse -k flags, config file, saved config |
| `prompt_missing_user_config()` | `lib/handlers/tool/call.rs` | Interactive config prompts |
| `apply_user_config_defaults()` | `lib/handlers/tool/call.rs` | Apply manifest defaults |
| `allocate_system_config()` | `lib/system_config.rs` | Allocate ports, dirs |
| `resolve_tool_path()` | `lib/handlers/tool/list.rs` | Resolve tool reference to path |
| `load_tool_from_path()` | `lib/resolver.rs` | Load manifest from path |
| `parse_tool_ref_for_config()` | `lib/handlers/tool/config_cmd.rs` | Parse tool ref for config storage |
| `save_tool_config()` | `lib/handlers/tool/config_cmd.rs` | Save config to disk |

**Note:** The helper functions in `call.rs` (`parse_user_config`, `prompt_missing_user_config`, `apply_user_config_defaults`) are already `pub(super)` so they're accessible within the handlers module.

## Implementation Order

1. Add rmcp "server" feature to Cargo.toml
2. Create `lib/proxy.rs` with types and ServerHandler impl
3. Create `lib/handlers/tool/run.rs` with handler
4. Update `lib/handlers/tool/mod.rs` and `lib/lib.rs`
5. Update `lib/commands.rs` with new Run definition
6. Update `bin/tool.rs` dispatch
7. Test stdio passthrough first, then bridging
8. Update README.md and docs

## Documentation Updates

### README.md

Update the "All Commands" table - change `run` description:

| Command | Description |
|---------|-------------|
| `run` | Run MCP server in proxy mode |

Add new section under "Test" or create "Run" section:

```markdown
## Run

Run your MCP server in proxy mode:

```sh
tool run
```

Starts the server with native transport (stdio or HTTP based on manifest).

### Protocol Bridging

Expose a server via a different transport:

```sh
# Expose an HTTP backend as stdio (for Claude Desktop)
tool run --expose stdio

# Expose a stdio backend as HTTP
tool run --expose http --port 3000

# With custom host binding
tool run --expose http --port 8080 --host 0.0.0.0
```

This is useful for:
- Connecting Claude Desktop (stdio) to remote HTTP MCP servers
- Exposing local stdio tools over the network
- Testing tools with different clients
```

### Remove script examples

Remove any references to `tool run <script>` - users should use `tool build`, `tool test` directly.
