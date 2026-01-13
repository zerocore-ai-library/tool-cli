<div align="center">

  <h3>[ tool ]</h3>
  <h4>———&nbsp;&nbsp;&nbsp;The Package Manager for MCP Tools&nbsp;&nbsp;&nbsp;———</h4>

</div>

<br />

<div align="center">
  <video autoplay src="https://github.com/user-attachments/assets/23618f92-5897-44d1-bfa6-1058f30c09e" width="800" controls></video>
</div>

<!--
<div align="center">
  <a href="https://asciinema.org/a/itQE92vIJiyq1PAPnaGURzDp" target="_blank"><img src="https://cdn.simpleicons.org/gnometerminal" height="12"/></a> <sup><sub><a href="https://asciinema.org/a/itQE92vIJiyq1PAPnaGURzDp" target="_blank">SEE HOW IT WORKS HERE →</a></sub></sup>
</div> -->

<br />

> [MCP](https://github.com/modelcontextprotocol) solved how AI agents integrate with other systems. [MCPB](https://github.com/modelcontextprotocol/mcpb) solved how users install them. But if you're building MCP tools, you're still copying JSON configs, wrestling with dependencies, and manually testing against clients.
>
> `tool-cli` is the missing piece. It handles the entire lifecycle, from scaffolding to publishing, so you can focus on developing your tool.

<br />

<div align='center'>• • •</div>

<br />

## INSTALL

```sh
curl -fsSL https://raw.githubusercontent.com/zerocore-ai/tool-cli/main/install.sh | sh
```

<br />

## QUICK START

<h4>1&nbsp;&nbsp;⏵&nbsp;&nbsp;Create a New Tool</h4>

```sh
tool init
```

Interactive prompts walk you through creating an MCPB package. You get a working scaffold with `manifest.json` configured correctly.

> <details>
> <summary>&nbsp;Want to skip the prompts?</summary>
>
> ```sh
> tool init my-tool --type node --yes
> ```
>
> </details>

##

<h4>2&nbsp;&nbsp;⏵&nbsp;&nbsp;Or Detect an Existing MCP Server</h4>

Most MCP servers already exist. They're sitting in repos, working fine, but not packaged for distribution.

```sh
tool detect
```

Run this in your project. It scans for patterns and shows what kind of MCP server it detected (type, transport, entry point, package manager, and confidence score).

```sh
tool init
```

Running tool init on an existing MCP project shows the detected configuration and prompts you to confirm creating manifest.json and .mcpbignore. Your MCP server is now an MCPB project.

##

<h4>3&nbsp;&nbsp;⏵&nbsp;&nbsp;Develop</h4>

Define scripts in your manifest:

```jsonc
{
  // ...
  "_meta": {
    "store.tool.mcpb": {
      "scripts": {
        "build": "npm run build",
        "test": "npm test",
        "dev": "npm run dev"
      }
    }
  }
}
```

Run them directly:

```sh
tool build
tool test
tool run dev
```

Same muscle memory as npm. Different manifest.

##

<h4>4&nbsp;&nbsp;⏵&nbsp;&nbsp;Test</h4>

Inspect what your server exposes:

```sh
tool info
```

Shows tools, prompts, resources. This is what clients see when they connect.

Call a tool directly:

```sh
tool call my-tool -m get_weather -p location="San Francisco"
```

Invokes a specific method with parameters. No client needed. Useful for debugging before you ship.

Validate your manifest:

```sh
tool validate
```

Catches missing fields, type mismatches, invalid paths. Better to find these now than after publishing.

##

<h4>5&nbsp;&nbsp;⏵&nbsp;&nbsp;Pack</h4>

```sh
tool pack
```

Creates a `.mcpb` file, a zipped file with your server, dependencies, and manifest. This is what gets distributed.

The packer validates your manifest first and respects `.mcpbignore` (same syntax as `.gitignore`).

##

<h4>6&nbsp;&nbsp;⏵&nbsp;&nbsp;Publish</h4>

```sh
tool login
tool publish
```

Authenticate once with tool.store, then publish. Your tool becomes discoverable and installable by anyone.

> <details>
> <summary>&nbsp;Preview first</summary>
>
> ```sh
> tool publish --dry-run
> ```
>
> </details>

<br />

<div align='center'>• • •</div>

<br />

## THE MANIFEST

Everything about your tool lives in `manifest.json`. Minimal example:

```jsonc
{
  "manifest_version": "0.3",
  "name": "weather-tool",
  "version": "1.0.0",
  "description": "Get weather data for any location",
  "author": {
    "name": "Your Name"
  },
  "server": {
    "type": "node",
    "transport": "stdio",
    "entry_point": "dist/index.js"
  },
  "tools": [
    {
      "name": "get_weather",
      "description": "Fetches current weather for a location"
    }
  ]
}
```

### Server Types

| Type | Use Case |
|------|----------|
| `node` | JavaScript/TypeScript servers |
| `python` | Python servers |
| `binary` | Pre-compiled executables (Rust, Go, etc.) |

### Transports

| Transport | Description |
|-----------|-------------|
| `stdio` | Runs as child process, communicates over stdin/stdout |
| `http` | Runs as service, communicates over HTTP |

The `http` transport is a tool.store extension to MCPB. It enables remote MCP servers, tools that live on the network rather than the local machine.

### User Configuration

If your tool needs API keys or user-provided settings:

```jsonc
{
  // ...
  "user_config": {
    "api_key": {
      "type": "string",
      "title": "API Key",
      "description": "Your weather service API key",
      "required": true,
      "sensitive": true
    }
  }
}
```

MCP hosts handle the UI. They prompt users during installation, validate inputs, and store sensitive values in the system keychain.

Variables become available in your server config:

```jsonc
{
  // ...
  "server": {
    "mcp_config": {
      "command": "node",
      "args": ["${__dirname}/server/index.js"],
      "env": {
        "API_KEY": "${user_config.api_key}"
      }
    }
  }
}
```

### Reference Mode

Not all tools need bundled code. Some point to existing commands or remote servers:

```jsonc
{
  // ...
  "server": {
    "transport": "http",
    "mcp_config": {
      "url": "https://api.example.com/mcp/",
      "headers": {
        "Authorization": "Bearer ${user_config.token}"
      }
    }
  }
}
```

No `entry_point`, no bundled code. The manifest just describes how to connect. Useful for wrapping system-installed MCP servers, connecting to remote endpoints, or creating thin clients over existing infrastructure.

<br />

<div align='center'>• • •</div>

<br />

## MANAGING TOOLS

On the consumer side:

```sh
tool add weather-tool        # Install from registry
tool list                    # See what's installed
tool remove weather-tool     # Uninstall
tool search weather          # Find tools
```


## COMMANDS

**Create**
- `init` — Scaffold a new tool project
- `detect` — Generate manifest from existing MCP server

**Develop**
- `run <script>` — Execute a manifest script
- `validate` — Check manifest against spec
- `info` — Display tool capabilities
- `call` — Invoke a tool method directly

**Distribute**
- `pack` — Create .mcpb bundle
- `publish` — Upload to registry

**Manage**
- `add` — Install a tool
- `remove` — Uninstall a tool
- `list` — Show installed tools
- `search` — Find tools in registry
- `download` — Download without installing

**Auth**
- `login` — Authenticate with registry
- `logout` — Clear authentication
- `whoami` — Show current user

<br />

<div align='center'>• • •</div>

<br />

## WHY THIS EXISTS

MCP is becoming the standard for AI tool integration. But standards only matter if people can actually use them.

Anthropic's MCPB format solved the installation problem: users can now install MCP tools with one click. But developers still need to create those packages. They need to validate manifests, bundle dependencies, test locally, and publish somewhere discoverable.

tool-cli is that toolchain. And tool.store is that registry.

The goal is simple: make building and sharing MCP tools as easy as publishing an npm package.

<br />

## LINKS

- [tool.store](https://tool.store) — The MCP tool registry
- [MCPB Specification](https://github.com/modelcontextprotocol/mcpb) — The bundle format
- [MCP Protocol](https://modelcontextprotocol.io) — Model Context Protocol docs
