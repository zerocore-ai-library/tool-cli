# Bash MCP Server

MCP server providing shell command execution.

## Quick Start

```bash
uv sync
uv run server.py
```

## Tools

| Tool | Description |
|------|-------------|
| `exec` | Execute a bash command and return its output |

### exec

Execute a bash command and return its output.

**Parameters:**
- `command` (string, required): The shell command to execute
- `timeout` (int, optional): Timeout in seconds (default 120)

**Returns:** Command output (stdout + stderr)
