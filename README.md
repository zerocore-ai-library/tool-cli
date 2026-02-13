<div align="center">

  <h3>[ tool-cli ]</h3>
  <h4>———&nbsp;&nbsp;&nbsp;The Missing Package Manager for MCP Tools&nbsp;&nbsp;&nbsp;———</h4>

</div>

<br />

> [MCP](https://github.com/modelcontextprotocol) solved how AI agents integrate with other systems. [MCPB](https://github.com/modelcontextprotocol/mcpb) solved how users install them. But if you're building MCP tools, you're still copying JSON configs, wrestling with dependencies, and manually testing against clients.
>
> `tool-cli` is the missing piece. It handles the entire lifecycle from scaffolding to publishing, so you can focus on building your tool.

<br />

<div align="center">
  <video autoplay src="https://github.com/user-attachments/assets/643f0149-6ba8-4935-b899-e77c2ca64fb4" width="100%" controls></video>
</div>

<br />

<div align='center'>
  <a href="https://discord.com/invite/rd49qgj5" target="_blank">
    <img src="https://img.shields.io/badge/join discord-%2300acee.svg?color=mediumslateblue&style=for-the-badge&logo=discord&logoColor=white" alt=discord style="margin-bottom: 5px;"/>
  </a>
  <a href="https://tool.store/docs" target="_blank">
    <img src="https://img.shields.io/badge/read the docs-%2300acee.svg?color=ff4500&style=for-the-badge&logo=gitbook&logoColor=white" alt=documentation style="margin-bottom: 5px;"/>
  </a>
</div>

<br />

- <img src="https://octicons-col.vercel.app/download/f88349" height="14"/> &nbsp;**Growing marketplace:** discover and install MCPs from [tool.store](https://tool.store)
- <img src="https://octicons-col.vercel.app/plug/f88349" height="14"/> &nbsp;**Works with your stack:** Claude Code, Cursor, OpenCode, VS Code, and more
- <img src="https://octicons-col.vercel.app/rocket/f88349" height="14"/> &nbsp;**Ship your own MCP:** create your own MCP server interactively with `tool init`
- <img src="https://octicons-col.vercel.app/server/f88349" height="14"/> &nbsp;**Unified proxy:** run all your MCPs through a single `tool run` interface
- <img src="https://octicons-col.vercel.app/command-palette/f88349" height="14"/> &nbsp;**MCPs as CLIs:** invoke any tool directly from your terminal with `tool call`
- <img src="https://octicons-col.vercel.app/gear/f88349" height="14"/> &nbsp;**Configure once:** set it up once, use it everywhere
- <img src="https://octicons-col.vercel.app/dependabot/f88349" height="14"/> &nbsp;**Built for both humans and agents:** clean output that works in terminals and AI workflows
- <img src="https://octicons-col.vercel.app/shield-lock/f88349" height="14"/> &nbsp;**Encrypted by default:** API keys and secrets are encrypted at rest
- <img src="https://octicons-col.vercel.app/passkey-fill/f88349" height="14"/> &nbsp;**OAuth just works:** browser flow, token refresh, secure storage handled for you
- <img src="https://octicons-col.vercel.app/device-desktop/f88349" height="14"/> &nbsp;**Fully local:** your API keys and tokens never leave your machine
- <img src="https://octicons-col.vercel.app/broadcast/f88349" height="14"/> &nbsp;**Session support:** keep stdio servers alive and target them with multiple calls *(coming soon)*
- <img src="https://octicons-col.vercel.app/container/f88349" height="14"/> &nbsp;**Sandboxed execution:** run tools in isolated environments *(coming soon)*

<br />

<div align='center'>• • •</div>

<br />

## Install

> macOS / Linux:
>
> ```sh
> curl -fsSL https://cli.tool.store | sh
> ```
>
> Windows:
>
> ```powershell
> irm https://cli.tool.store/windows | iex
> ```
>
> Or with Cargo (any platform):
>
> ```sh
> cargo install --git https://github.com/zerocore-ai/tool-cli --locked
> ```

<br />

## Quick Start

Get your first MCP tool published in three steps.

<h4>1&nbsp;&nbsp;⏵&nbsp;&nbsp;Create</h4>

> ```sh
> tool init my-tool
> ```
>
> This gives you a working MCP server with a valid `manifest.json`. Just follow the prompts to pick your language and transport.
>
> For bundled packages, you need to run build.
>
> ```sh
> tool build my-tool
> ```
>
> <details>
> <summary>Already have an MCP server?</summary>
> <blockquote>
> Run `tool detect` in your project to see what tool-cli finds. Then `tool init` will generate a manifest from your existing code.
>
> ```sh
> tool detect my-tool      # shows detected type, transport, entry point
> ```
>
> ```sh
> tool init my-tool        # generates manifest.json
> ```
>
> </blockquote>
> </details>

##

<h4>2&nbsp;&nbsp;⏵&nbsp;&nbsp;Test</h4>

> ```sh
> tool info my-tool
> ```
>
> Shows you what your server exposes. Tools, prompts, resources. This is what clients will see when they connect.
>
> ```sh
> tool call my-tool -m hello -p name="Steve"
> ```
>
> You can call any method directly. No client needed.
>
> <details>
> <summary>Method shorthand</summary>
> <blockquote>
> MCP tools often use `toolname__method` naming. You can use `.` as shorthand.
>
> ```sh
> tool call bash -m .exec -p command="ls -la"  # expands to bash__exec
> ```
>
> ```sh
> tool call files -m .fs.read -p path="/tmp"   # expands to files__fs__read
> ```
>
> </blockquote>
> </details>
>
> ```sh
> tool run my-tool
> ```
>
> Starts the mcp server for connection.

##

<h4>3&nbsp;&nbsp;⏵&nbsp;&nbsp;Share</h4>

> ```sh
> tool login
> ```
>
> ```sh
> tool publish my-tool
> ```
>
> Log in once, then publish. Now anyone can install your tool.
>
> <details>
> <summary>Have native binaries or platform-specific deps?</summary>
> <blockquote>
>
> Use multi-platform publishing to create separate bundles for each OS/architecture:
>
> ```sh
> tool publish my-tool --multi-platform
> ```
>
> See [Multi-Platform Bundles](#multi-platform-bundles) for details.
>
> </blockquote>
> </details>

<br />

<div align='center'>• • •</div>

<br />

## Using Tools

`tool-cli` essentially turns your MCP servers into CLIs. You can inspect, call, and compose tools directly from the terminal. It is the foundation for building [code-mode agents](cookbooks/).

### Find Tools

> ```sh
> tool search filesystem
> ```
>
> Search the registry for tools. You'll see names, descriptions, and download counts.
>
> ```sh
> tool grep "file"
> ```
>
> Search across all installed tools - server names, tool names, descriptions, and schema fields.

##

### Preview Tools

> ```sh
> tool preview library/open-data
> ```
>
> Inspect a tool from the registry without installing it. See its available methods before you commit.
>
> ```sh
> tool preview library/bash -m exec
> ```
>
> Preview a specific method to see its input and output schemas.

##

### Install Tools

> ```sh
> tool install library/bash
> ```
>
> Installs a tool from the registry. You can also install from a local path.
>
> ```sh
> tool list
> ```
>
> See what you have installed.

##

### Run Tools

> ```sh
> tool run library/bash
> ```
>
> Starts the tool with its native transport. Connect your MCP client to it.
>
> You can also use `--expose` to bridge between transports.
>
> ```sh
> tool run <namespace/remote-mcp> --expose stdio # HTTP backend to stdio
> ```
>
> ```sh
> tool run <namespace/local-mcp> --expose http --port 3000 # stdio backend to HTTP
> ```

##

### Configure Tools

> ```sh
> tool config set library/terminal
> ```
>
> Some tools need configuration like API keys. This walks you through setting them up interactively. You can also pass values directly with `tool config set library/terminal KEY=VALUE`.
>
> ```sh
> tool config get library/terminal
> ```
>
> Check what config values are set.
>
> ```sh
> tool config list
> ```
>
> See all tools that have saved configuration.
>
> ```sh
> tool config unset library/terminal
> ```
>
> Remove config and credentials for a tool, or use `--all` for all tools.

##

### Use Tools

> ```sh
> tool info library/bash
> ```
>
> See what a tool exposes. Tools, prompts, resources.
>
> ```sh
> tool call library/bash -m exec -p command="echo hello"
> ```
>
> Call a method directly. Great for testing things out.

<br />

<div align='center'>• • •</div>

<br />

## Host Integration

Once you've installed some tools, you probably want to use them in your favorite AI app. Instead of manually editing JSON configs, just run:

> ```sh
> tool host add claude-desktop library/open-data
> ```
>
> This registers the tool with the host. Works with Claude Desktop, Cursor, VS Code, Claude Code, Codex, Windsurf, Zed, Gemini CLI, Kiro, Roo Code, and OpenCode.

<br />

<div align="center">
<table>
<tr>
<td align="center" width="180">
<img src="https://github.com/user-attachments/assets/33950c03-0925-437d-8cf2-edbc2adf731b" width="50" height="50" alt="Claude Code"/>
<br />
<code>claude-code</code>
</td>
<td align="center" width="180">
<img src="https://github.com/user-attachments/assets/9bd08b42-6cfc-4c32-80df-0e04d2ec5544" width="50" height="50" alt="OpenCode"/>
<br />
<code>opencode</code>
</td>
<td align="center" width="180">
<img src="https://avatars.githubusercontent.com/u/14957082?s=200&v=4" width="50" height="50" alt="Codex"/>
<br />
<code>codex</code>
</td>
<td align="center" width="180">
<img src="https://www.cursor.com/brand/icon.svg" width="50" height="50" alt="Cursor"/>
<br />
<code>cursor</code>
</td>
<td align="center" width="180">
<img src="https://upload.wikimedia.org/wikipedia/commons/9/9a/Visual_Studio_Code_1.35_icon.svg" width="50" height="50" alt="VS Code"/>
<br />
<code>vscode</code>
</td>
<td align="center" width="180">
<img src="https://github.com/user-attachments/assets/2a57726d-c1f0-4826-a4ff-adef71cd3842" width="50" height="50" alt="Claude Desktop"/>
<br />
<code>claude-desktop</code>
</td>
</tr>
<tr>
<td align="center" width="180">
<img src="https://exafunction.github.io/public/brand/windsurf-black-symbol.svg" width="50" height="50" alt="Windsurf"/>
<br />
<code>windsurf</code>
</td>
<td align="center" width="180">
<img src="https://avatars.githubusercontent.com/u/79345384?s=200&v=4" width="50" height="50" alt="Zed"/>
<br />
<code>zed</code>
</td>
<td align="center" width="180">
<img src="https://avatars.githubusercontent.com/u/161781182?s=200&v=4" width="50" height="50" alt="Gemini CLI"/>
<br />
<code>gemini-cli</code>
</td>
<td align="center" width="180">
<img src="https://avatars.githubusercontent.com/u/207925904?s=200&v=4" width="50" height="50" alt="Kiro"/>
<br />
<code>kiro</code>
</td>
<td align="center" width="180">
<img src="https://avatars.githubusercontent.com/u/211522643?s=200&v=4" width="50" height="50" alt="Roo Code"/>
<br />
<code>roo-code</code>
</td>
</tr>
</table>
</div>

<br />

> ```sh
> tool host list                             # see all supported hosts
> tool host add cursor library/open-data     # add a tool to Cursor
> tool host add vscode                       # add all installed tools
> tool host remove claude-desktop            # remove tools from a host
> tool host show cursor                      # preview the generated config
> ```
>
> You can specify individual tools or omit them to register all installed tools. The command creates backups before modifying anything, so your original config is safe.

<br />

<div align='center'>• • •</div>

<br />

## Multi-Platform Bundles

Most MCP tools are pure JavaScript or Python and work everywhere with a single bundle. But if your tool has **platform-specific dependencies**, you need multi-platform publishing:

- **Native binaries** (Rust, Go, C++)
- **Node.js with native addons** (better-sqlite3, sharp, etc.)
- **Python with compiled extensions** (numpy, pandas, etc.)

Multi-platform creates separate bundles for each OS/architecture. Users automatically get the right one. Learn more about [multi-platform packaging](https://tool.store/docs/advanced/multi-platform).

> Reference-mode `.mcpbx` bundles that point to remote servers or external commands like `npx` and `uvx` don't need multi-platform publishing since they don't bundle any code.

### Publishing

> ```sh
> tool publish --multi-platform \
>   --darwin-arm64 ./dist/my-tool-darwin-arm64.mcpb \
>   --darwin-x64 ./dist/my-tool-darwin-x64.mcpb \
>   --linux-arm64 ./dist/my-tool-linux-arm64.mcpb \
>   --linux-x64 ./dist/my-tool-linux-x64.mcpb \
>   --win32-arm64 ./dist/my-tool-win32-arm64.mcpb \
>   --win32-x64 ./dist/my-tool-win32-x64.mcpb
> ```
>
> Specify the bundle for each platform. Typically done in CI after building on each runner.
>
> #### GitHub Actions
>
> Use [zerocore-ai/tool-action](https://github.com/zerocore-ai/tool-action) to automate multi-platform builds:
>
> ```yaml
> - uses: zerocore-ai/tool-action/setup@v1
> - uses: zerocore-ai/tool-action/pack@v1
>   with:
>     target: ${{ matrix.target }}
> ```
>
> <details>
> <summary>Auto-detect from manifest</summary>
> <blockquote>
>
> If all platform binaries are available locally, this packs and uploads all variants defined in your manifest's `platform_overrides`:
>
> ```sh
> tool publish --multi-platform
> ```
>
> </blockquote>
> </details>

##

### Installing with Platform Selection

> ```sh
> tool install library/bash
> ```
>
> Automatically downloads the bundle matching your system (e.g., `darwin-arm64` on Apple Silicon).

<br />

<div align='center'>• • •</div>

<br />

## Commands

| Command     | What it does                                                |
| ----------- | ----------------------------------------------------------- |
| `init`      | Create a new tool or convert an existing MCP server to MCPB |
| `detect`    | Scan a project and show what tool-cli finds                 |
| `validate`  | Check your manifest for errors                              |
| `info`      | Show what a tool exposes                                    |
| `preview`   | Preview a registry tool without installing                  |
| `call`      | Call a tool method directly                                 |
| `run`       | Start a tool as a server                                    |
| `pack`      | Bundle into `.mcpb`/`.mcpbx` (supports multi-platform)      |
| `publish`   | Upload to the registry (supports multi-platform)            |
| `install`   | Install a tool (auto-detects platform)                      |
| `download`  | Download a bundle without installing                        |
| `uninstall` | Remove an installed tool                                    |
| `list`      | Show installed tools                                        |
| `search`    | Find tools in the registry                                  |
| `grep`      | Search tool schemas by pattern                              |
| `config`    | Manage tool configuration                                   |
| `host`      | Register tools with MCP hosts                               |
| `login`     | Log in to the registry                                      |
| `logout`    | Log out from the registry                                   |
| `whoami`    | Show current authentication status                          |
| `self`      | Manage tool-cli itself (update, uninstall)                  |

Check out the [CLI docs](https://tool.store/docs/cli) for the full details.

<br />

<div align='center'>• • •</div>

<br />

## The MCPB Extension

[MCPB](https://github.com/modelcontextprotocol/mcpb) is great for what it was designed for: bundled servers that run locally over stdio. But most MCP servers today run via `npx` or `uvx` (nothing to bundle), some are remote HTTP servers (no local code at all), and some need things like host-managed ports or OAuth flows that the spec doesn't cover.

We created [MCPBX](https://tool.store/docs/building-tools/mcpbx) (`.mcpbx`) to fill those gaps. It's a superset of MCPB that adds HTTP transport, reference mode (so you can point to `npx`/`uvx` or a remote URL instead of bundling code), system config for host-managed resources, OAuth config, and template functions for constructing auth headers.

The separate file extension exists so hosts know upfront whether they can handle the manifest. `tool-cli` picks the right format automatically based on what your manifest uses.

<br />

<div align='center'>• • •</div>

<br />

## Why This Exists

MCP is becoming the standard for AI tool integration. But standards only matter if people can actually use them.

Anthropic's MCPB format solved the installation problem. Users can install MCP tools with one click now. But developers still need to create those packages. They need to validate manifests, bundle dependencies, test locally, and publish somewhere discoverable.

tool-cli is that toolchain. And tool.store is that registry.

The goal is simple. Make building and sharing MCP tools as easy as publishing an npm package.

<div align="center">
    <a href="https://tool.store/blog/building-context-efficient-agents" target="_blank"><img src="https://octicons-col.vercel.app/dependabot/f8834b" height="16"/></a> <sup><a href="https://tool.store/blog/building-context-efficient-agents" target="_blank">BUILD <strong>CONTEXT-EFFICIENT</strong> AI AGENTS WITH TOOL-CLI →</a></sup>
</div>

<br />

<div align='center'>• • •</div>

<br />

## Licensing

`tool-cli` is licensed under the [Apache 2.0 License](LICENSE).
