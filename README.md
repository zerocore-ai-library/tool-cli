<div align="center">

  <h3>[ tool-cli ]</h3>
  <h4>———&nbsp;&nbsp;&nbsp;The Missing Package Manager for MCP Tools&nbsp;&nbsp;&nbsp;———</h4>

</div>

<br />

<div align="center">
  <video autoplay src="https://github.com/user-attachments/assets/23618f92-5897-44d1-bfa6-1058f30c09ef" width="800" controls></video>
</div>


<br />

<div align='center'>
  <a href="https://discord.gg/ck5xz7vR" target="_blank">
    <img src="https://img.shields.io/badge/join discord-%2300acee.svg?color=mediumslateblue&style=for-the-badge&logo=discord&logoColor=white" alt=discord style="margin-bottom: 5px;"/>
  </a>
  <a href="https://tool.store/docs" target="_blank">
    <img src="https://img.shields.io/badge/read the docs-%2300acee.svg?color=ff4500&style=for-the-badge&logo=gitbook&logoColor=white" alt=documentation style="margin-bottom: 5px;"/>
  </a>
</div>

<br />

> [MCP](https://github.com/modelcontextprotocol) solved how AI agents integrate with other systems. [MCPB](https://github.com/modelcontextprotocol/mcpb) solved how users install them. But if you're building MCP tools, you're still copying JSON configs, wrestling with dependencies, and manually testing against clients.
>
> `tool-cli` is the missing piece. It handles the entire lifecycle, from scaffolding to publishing, so you can focus on developing your tool.

<br />

<div align="center">
    <a href="https://asciinema.org/a/itQE92vIJiyq1PAPnaGURzDpv" target="_blank"><img src="https://octicons-col.vercel.app/dependabot/f8834b" height="16"/></a> <sup><a href="https://asciinema.org/a/itQE92vIJiyq1PAPnaGURzDpv" target="_blank">BUILD <strong>CONTEXT-EFFICIENT</strong> AI AGENTS WITH TOOL-CLI →</a></sup>
</div>

<br />

<div align='center'>• • •</div>

<br />

## Install

> ```sh
> curl -fsSL https://cli.tool.store | sh
> ```

<br />

## Quick Start

Get your first MCP tool published in three steps.

<h4>1&nbsp;&nbsp;⏵&nbsp;&nbsp;Create</h4>

> ```sh
> tool init my_tool
> ```
>
> This gives you a working MCP server with a valid `manifest.json`. Just follow the prompts to pick your language and transport.
>
> <details>
> <summary>Already have an MCP server?</summary>
> <blockquote>
> Run `tool detect` in your project to see what tool-cli finds. Then `tool init` will generate a manifest from your existing code.
>
> ```sh
> cd my_existing_mcp_project
> tool detect        # shows detected type, transport, entry point
> tool init          # generates manifest.json
> ```
> </blockquote>
> </details>

##

<h4>2&nbsp;&nbsp;⏵&nbsp;&nbsp;Test</h4>

> ```sh
> tool info
> ```
>
> Shows you what your server exposes. Tools, prompts, resources. This is what clients will see when they connect.
>
> ```sh
> tool call my_tool -m get_weather location="San Francisco"
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
> tool call bash -m .exec command="ls -la"     # expands to bash__exec
> tool call files -m .fs.read path="/tmp"      # expands to files__fs__read
> ```
> </blockquote>
> </details>

##

<h4>3&nbsp;&nbsp;⏵&nbsp;&nbsp;Share</h4>

> ```sh
> tool login
> tool publish
> ```
>
> Log in once, then publish. Now anyone can install your tool.
>
> <details>
> <summary>Just want to bundle it?</summary>
> <blockquote>
>
> ```sh
> tool pack
> ```
>
> Creates a `.mcpb` file you can distribute yourself.
> </blockquote>
> </details>

<br />

<div align='center'>• • •</div>

<br />

## Using Tools

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
> Search through tool schemas by pattern. Useful when you're looking for tools with specific capabilities.

##

### Install Tools

> ```sh
> tool install appcypher/bash
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
> tool run appcypher/bash
> ```
>
> Starts the tool with its native transport. Connect your MCP client to it.
>
> You can also use `--expose` to bridge between transports.
>
> ```sh
> tool run --expose stdio              # HTTP backend to stdio
> tool run --expose http --port 3000   # stdio backend to HTTP
> ```

##

### Configure Tools

> ```sh
> tool config set appcypher/bash
> ```
>
> Some tools need configuration like API keys. This walks you through setting them up interactively.
>
> ```sh
> tool config get appcypher/bash
> ```
>
> Check what config values are set.

##

### Use Tools

> ```sh
> tool info appcypher/bash
> ```
>
> See what a tool exposes. Tools, prompts, resources.
>
> ```sh
> tool call appcypher/bash -m .exec command="echo hello"
> ```
>
> Call a method directly. Great for testing things out.

<br />

<div align='center'>• • •</div>

<br />

## Host Integration

Once you've installed some tools, you probably want to use them in your favorite AI app. Instead of manually editing JSON configs, just run:

> ```sh
> tool host add claude-desktop acme/weather
> ```
>
> This registers the tool with the host. Works with Claude Desktop, Cursor, VS Code, and Claude Code.

<br />

<div align="center">
<table>
<tr>
<td align="center" width="150">
<img src="https://avatars.githubusercontent.com/u/76263028?s=200&v=4" width="50" height="50" alt="Anthropic"/>
<br />
<strong>Claude Desktop</strong>
<br />
<code>claude-desktop</code>
</td>
<td align="center" width="150">
<img src="https://www.cursor.com/brand/icon.svg" width="50" height="50" alt="Cursor"/>
<br />
<strong>Cursor</strong>
<br />
<code>cursor</code>
</td>
<td align="center" width="150">
<img src="https://upload.wikimedia.org/wikipedia/commons/9/9a/Visual_Studio_Code_1.35_icon.svg" width="50" height="50" alt="VS Code"/>
<br />
<strong>VS Code</strong>
<br />
<code>vscode</code>
</td>
<td align="center" width="150">
<img src="https://avatars.githubusercontent.com/u/76263028?s=200&v=4" width="50" height="50" alt="Claude Code"/>
<br />
<strong>Claude Code</strong>
<br />
<code>claude-code</code>
</td>
</tr>
</table>
</div>

<br />

> ```sh
> tool host list                         # see all supported hosts
> tool host add cursor acme/weather      # add a tool to Cursor
> tool host add vscode                   # add all installed tools
> tool host remove claude-desktop        # remove tools from a host
> tool host show cursor                  # preview the generated config
> ```
>
> You can specify individual tools or omit them to register all installed tools. The command creates backups before modifying anything, so your original config is safe.

<br />

<div align='center'>• • •</div>

<br />

## Commands

| Command | What it does |
|---------|--------------|
| `init` | Create a new tool or convert an existing MCP server |
| `detect` | Scan a project and show what tool-cli finds |
| `validate` | Check your manifest for errors |
| `info` | Show what a tool exposes |
| `call` | Call a tool method directly |
| `run` | Start a tool as a server |
| `pack` | Bundle into a `.mcpb` file |
| `publish` | Upload to the registry |
| `install` | Install a tool from the registry |
| `uninstall` | Remove an installed tool |
| `list` | Show installed tools |
| `search` | Find tools in the registry |
| `grep` | Search tool schemas by pattern |
| `config` | Manage tool configuration |
| `host` | Register tools with MCP hosts |
| `login` | Log in to the registry |

Check out the [CLI docs](https://tool.store/docs/cli) for the full details.

<br />

<div align='center'>• • •</div>

<br />

## Why This Exists

MCP is becoming the standard for AI tool integration. But standards only matter if people can actually use them.

Anthropic's MCPB format solved the installation problem. Users can install MCP tools with one click now. But developers still need to create those packages. They need to validate manifests, bundle dependencies, test locally, and publish somewhere discoverable.

tool-cli is that toolchain. And tool.store is that registry.

The goal is simple. Make building and sharing MCP tools as easy as publishing an npm package.


<br />

<div align="center">
    <a href="https://asciinema.org/a/itQE92vIJiyq1PAPnaGURzDpv" target="_blank"><img src="https://octicons-col.vercel.app/dependabot/f8834b" height="16"/></a> <sup><a href="https://asciinema.org/a/itQE92vIJiyq1PAPnaGURzDpv" target="_blank">BUILD <strong>CONTEXT-EFFICIENT</strong> AI AGENTS WITH TOOL-CLI →</a></sup>
</div>

<br />


## Links

- [tool.store](https://tool.store) is the MCP tool registry
- [MCPB Specification](https://github.com/modelcontextprotocol/mcpb) is the bundle format
- [MCP Protocol](https://modelcontextprotocol.io) has the Model Context Protocol docs
