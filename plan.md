
---

We are introducing --concise/-c mode to tool-cli to reduce output verbosity for ai agents and a `tool grep` command for grepping on a tool's schema. This mode is particularly useful when agents require succinct responses, minimizing unnecessary details (e.g. unecessary line breaks, extra spaces, or verbose explanations that is okay with normal user experience but not agent experience). We don't want to contribute to context bloat, but we must also do it in a way that doesn't sacrifice clarity when needed or introduce ambiguity.

## Context
Model Context Protocol (MCP) was introduced by Anthropic to standardize how AI models interact with external tools and environments. MCP provides a structured way for models to execute commands, access files, and manage their context effectively. This protocol is essential for building robust AI agents that can perform complex tasks by leveraging external resources.

## The Issue: MCP Context Bloat
Lately, there is a growing sentiment among AI developers and users about the verbosity of the MCP tools descriptions and schemas that gets pre-loaded into the context of AI agents. They eat up valuable context space, leaving less room for actual user-agent interactions. In addition to that, verbose outputs from tool calls can cause bloat as well. A lot of MCP servers out there treat tools like traditional Rest APIs, returning detailed responses that are great for non-AI systems but not ideal for AI agents that need just enough to work with. Some are not even properly paginated, leading to overwhelming amounts of data being fed back to the agent. Further more, sometimes an AI agent may not need the entire/exact output of a tool call or maybe it needs to apply algorithmic transformations to the output before using it, but because of the way MCP is currently designed, the agent has to deal with the full output, and if piping the ouput as input to other tool calls, deal with duplication further worsening context space management.

## The Solution: Code Mode
As mentioned earlier, context space management is crucial for AI agents. To address the verbosity issue MCP schemas and outputs, Cloudlfare, Anthropic and others came up with code-mode. With code-mode, instead of preloading the entire tool schema and description into the context, you instead provide make the agent generate the code for searching for the tools it needs and executing them. This way, the agent would only a tool to execute the code it generates.

Everyone's approach to code-mode is slightly different, but the general idea is the same. Cloudflare's approach is to generate typescript schemas for for the mcp servers that gets loaded into the context and then generating code snippets that use those schemas to call the tools. Anthropic's idea is similar except you export the typescript schemas to a filesystem representing the server/tool structure and then the agent generates code snippets to call the tools in typescript.

In our case, we believe all you need is a bash tool and tool-cli. Agents are not just great at writing typescript code, they are spectacular at bash too. And what do you call most with bash? command line tools. So with a bash tool and tool-cli, the agent can generate concise bash commands to call the tools it needs. Typescript schemas are still quite verbose, clis are flexible and generally more concise. More importantly, we are not limited to just one language, bash makes it trivial to call other languages, pipe outputs to other commands, do text processing with tools like grep, awk, sed, jq etc. and do file manipulations.

tool-cli is meant to be a package manager for MCP tools in general but turns out it is also perfectly suited for code-mode because it provides a way for agent to discover for mcp tools, install them locally, grep on them and execute them. So we have introduced the --concise/-c mode that reduces output verbosity and cleans it up for AI agents consumption and a `tool grep` command for grepping on a tool's schemas.

## REFERENCES
- https://modelcontextprotocol.io/docs/getting-started/intro
- https://www.anthropic.com/engineering/code-execution-with-mcp
- https://blog.cloudflare.com/code-mode/

Please read the references above for more context on MCP and code-mode.
If you can't access the links above, please let me know.


Now go through the entire printed outputs of tool-cli subcommands and come up with a proposal for concise variants as well as the `tool grep` command.

---

# Proposal: Concise Mode (`--concise/-c`) and `tool grep` Command

## Executive Summary

This proposal introduces two features to reduce output verbosity for AI agents:

1. **`--concise/-c` global flag**: Reduces output verbosity across all commands
2. **`tool grep` command**: Enables searching/filtering tool schemas by pattern

These features support "code-mode" workflows where AI agents use `tool-cli` via bash rather than preloading MCP schemas into context.

---

## Part 1: Concise Mode (`--concise/-c`)

### Design Principles

1. **No decorations** - No emojis (✓, ✗, →, ⚠), no colors, no tree-drawing characters (├, └, │)
2. **Minimal whitespace** - No extra blank lines, minimal indentation
3. **Header + TSV format** - Column names on first line (prefixed with `#`), then tab-separated values
4. **Quoted strings** - Fields that may contain spaces (descriptions, paths, text) are double-quoted
5. **Machine-parseable** - Easy to parse with `cut`, `awk`, or skip header line programmatically
6. **Errors on stderr** - Success data on stdout, errors on stderr

### Global Flags

```
tool [--concise|-c] [--no-header|-H] <COMMAND> [OPTIONS]
```

| Flag | Description |
|------|-------------|
| `-c, --concise` | Enable concise TSV output for AI agents |
| `-H, --no-header` | Suppress the `#` header line (requires `-c`) |

**Note:** The `--config` flag in `tool info` and `tool call` commands uses `-C` as its short form to avoid conflicts.

---

### Command-by-Command Comparison

#### 1. `tool --help`

**Current (96 chars, 20 lines):**
```
Manage MCP tools and packages

Usage: tool <COMMAND>

Commands:
  detect    Detect an existing MCP server project (dry-run preview)
  init      Initialize a new tool package
  validate  Validate a tool manifest
  pack      Pack a tool into an .mcpb bundle
  run       Run a script defined in manifest.json
  info      Inspect a tool's capabilities
  call      Call a tool method
  list      List installed tools
  download  Download a tool from the registry
  add       Add a tool from the registry
  remove    Remove an installed tool
  search    Search for tools in the registry
  publish   Publish a tool to the registry
  login     Login to the registry
  logout    Logout from the registry
  whoami    Show authentication status
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
```

**Concise (14 lines):**
```
detect init validate pack run info call list download add remove search publish login logout whoami
```

Just a space-separated list of available commands. If an agent needs details, it can run `tool <cmd> -c --help`.

---

#### 2. `tool <cmd> --help` (e.g., `tool call --help`)

**Current:**
```
Call a tool method

Usage: tool call [OPTIONS] --method <METHOD> [TOOL]

Arguments:
  [TOOL]  Tool reference or path (default: current directory) [default: .]

Options:
  -m, --method <METHOD>            Method name to call
  -p, --param <PARAM>              Method parameters (KEY=VALUE or KEY=JSON)
  -c, --config <CONFIG>            Configuration values (KEY=VALUE)
      --config-file <CONFIG_FILE>  Path to config file (JSON)
  -v, --verbose                    Show verbose output
  -h, --help                       Print help
```

**Concise:**
```
tool call [TOOL] -m METHOD [-p KEY=VALUE]... [-c KEY=VALUE]... [--config-file PATH] [-v]
```

Single-line synopsis with essential structure.

---

#### 3. `tool --tree`

**Current (91 lines for full tree):**
```
tool
├── detect                               Detect an existing MCP server project (dry-run preview)
│   ├── [PATH]                           Path to project directory (defaults to current directory)
│   ├── -e, --entry <ENTRY>              Override detected entry point
│   └── ...
├── init                                 Initialize a new tool package
│   ├── [PATH]                           Directory path to initialize (defaults to current directory)
│   └── ...
...
```

**Concise:**
```
detect [PATH] [-e ENTRY] [--transport TRANSPORT] [-n NAME]
init [PATH] [-n NAME] [-t TYPE] [-d DESC] [-a AUTHOR] [-l LICENSE] [--http] [--reference] [-y] [--pm PM] [-e ENTRY] [--transport TRANSPORT] [-f]
validate [PATH] [--strict] [--json] [-q]
pack [PATH] [-o OUTPUT] [--no-validate] [--include-dotfiles] [-v]
run [SCRIPT] [PATH] [-l] [ARGS...]
info [TOOL] [--tools] [--prompts] [--resources] [-a] [--json] [-c CONFIG...] [--config-file PATH] [-v]
call [TOOL] -m METHOD [-p PARAM...] [-c CONFIG...] [--config-file PATH] [-v]
list [FILTER] [--json]
download NAME [-o OUTPUT]
add NAME
remove NAME
search QUERY
publish [PATH] [--dry-run]
login [--token TOKEN]
logout
whoami
```

One command per line with abbreviated options.

---

#### 4. `tool validate`

**Current (7 lines):**
```
  Validating manifest.json

  error[E000]: → /Users/steveakinyemi/Desktop/Personal/FOCUS/radical-projects/tool-cli
      ├─ manifest.json does not exist
      └─ help: run `tool init` to create one

  ✗ 1 error
```

**Concise (Header + TSV on stderr):**
```
#code	message	location	help
E000	"manifest not found"	"/path/to/dir"	"run `tool init` to create one"
```

Header + TSV format. Quoted fields for strings that may contain spaces.

**On success (0 lines):**
```
(empty - exit code 0 indicates success)
```

---

#### 5. `tool validate --json` (unchanged)

JSON output is already agent-friendly. In concise mode, we could minify it:

**Current (pretty-printed, 16 lines):**
```json
{
  "errors": [
    {
      "code": "E000",
      "details": "manifest.json does not exist",
      "help": "run `tool init` to create one",
      "location": "/path/to/dir",
      "message": "manifest not found"
    }
  ],
  "format": "manifest.json",
  "strict_valid": false,
  "valid": false,
  "warnings": []
}
```

**Concise (1 line):**
```json
{"errors":[{"code":"E000","details":"manifest.json does not exist","help":"run `tool init` to create one","location":"/path/to/dir","message":"manifest not found"}],"format":"manifest.json","strict_valid":false,"valid":false,"warnings":[]}
```

---

#### 6. `tool list`

**Current (5 lines):**
```
  ✓ Found 1 tool

    appcypher/filesystem  File system operations for AI agents (read, write, edit, glob, grep)
    └── stdio  /Users/steveakinyemi/.tool/tools/appcypher/filesystem@0.1.2
```

**Concise (Header + TSV):**
```
#name	type	path
appcypher/filesystem	stdio	"/Users/steveakinyemi/.tool/tools/appcypher/filesystem@0.1.2"
```

Tab-separated with header. Paths are quoted since they may contain spaces.

---

#### 7. `tool list --json` (minified in concise mode)

**Concise:**
```json
[{"name":"appcypher/filesystem","type":"stdio","location":"/path"}]
```

---

#### 8. `tool info <tool>`

**Current (large tree structure, 40+ lines):**
```
  ✓ Connected to rmcp v0.12.0

    Type       manifest.json (stdio)
    Location   /Users/steveakinyemi/.tool/tools/appcypher/filesystem@0.1.2/manifest.json

    Tools:
      filesystem__read  Read a file from the local filesystem. Returns content with line numbers.
      ├── Input
      │   ├── file_path*           string     Absolute path to the file to read.
      │   ├── limit                integer    Number of lines to read. Defaults to 2000.
      │   └── offset               integer    Starting line number (1-indexed). Defaults to 1.
      └── Output
          ├── content*             string     The file content with line numbers in cat -n format.
          ...
```

**Concise (function signature format):**
```
#type	location
stdio	"/path/to/manifest.json"
#tool
toolset:filesystem__read(file_path*: string, limit?: integer, offset?: integer) -> {content: string, end_line: integer, start_line: integer, total_lines: integer, truncated: boolean}
toolset:filesystem__write(file_path*: string, content*: string) -> {bytes_written: integer}
toolset:filesystem__edit(file_path*: string, old_string*: string, new_string*: string, replace_all?: boolean) -> {replacements: integer}
toolset:filesystem__glob(pattern*: string, path?: string) -> {files: array}
toolset:filesystem__grep(pattern*: string, path?: string, ...) -> {matches: array, total: integer, truncated: boolean}
```

Format: `TOOLSET:TOOL_NAME(params) -> {outputs}`
- `*` = required, `?` = optional
- Function signature style for intuitive reading

---

#### 9. `tool info --json` (minified in concise mode)

Same as regular JSON but minified (single line).

---

#### 10. `tool info --tools` (Concise: tool names only)

**Concise:**
```
filesystem__read filesystem__write filesystem__edit filesystem__glob filesystem__grep
```

Space-separated list of tool names.

---

#### 11. `tool whoami`

**Current (4 lines):**
```
  ✓ Authenticated
    User: @appcypher
    Registry: http://localhost:4444
    Token: reg_pkU2kxLxFg6...RXBH
```

**Concise (Header + TSV):**
```
#user	registry	status
@appcypher	http://localhost:4444	authenticated
```

**If not authenticated:**
```
#user	registry	status
-	http://localhost:4444	unauthenticated
```

---

#### 12. `tool detect`

**Current (12 lines):**
```
✓ Detected Rust MCP server

    Type         Rust
    Transport    stdio
    Entry        target/release/filesystem
    Confidence   95%
    Build        cargo build --release

  Files to create:
    manifest.json
    .mcpbignore

  Run tool init /path/to/project to generate files.
```

**Concise (Header + TSV):**
```
#type	transport	entry	confidence	build
rust	stdio	target/release/filesystem	95%	"cargo build --release"
```

Build command is quoted since it may contain spaces.

---

#### 13. `tool call`

**Current:**
```
  ✓ Called filesystem__glob on filesystem@0.1.2

    {
      "files": []
    }
```

**Concise:**
```
{"files":[]}
```

Just the raw JSON result, no wrapper.

---

#### 14. `tool search`

**Current (assumed format based on registry):**
```
  → Searching registry for tools matching: filesystem

    appcypher/filesystem@0.1.2  File system operations for AI agents
    └── 1000 downloads
```

**Concise (Header + TSV):**
```
#ref	description	downloads
appcypher/filesystem@0.1.2	"File system operations for AI agents"	1000
```

Header + TSV format. Description is quoted since it may contain spaces.

---

#### 15. `tool pack`

**Current:**
```
  → Packing tool...
  ✓ Packed to myproject-1.0.0.mcpb (145.2 KB)
```

**Concise (Header + TSV):**
```
#file	size
myproject-1.0.0.mcpb	148684
```

Header + TSV format.

---

#### 16. `tool init`

**Current:**
```
  → Initializing new tool package...
  ✓ Created manifest.json
  ✓ Created .mcpbignore

  Next steps:
    1. Edit manifest.json to configure your package
    2. Run `tool validate` to check your manifest
    3. Run `tool pack` to create a distributable bundle
```

**Concise (Header + TSV):**
```
#file
manifest.json
.mcpbignore
```

Header + TSV format. One file per line.

---

#### 17. `tool add`

**Current:**
```
  → Downloading appcypher/filesystem@0.1.2...
  ✓ Installed to /path/to/tools/appcypher/filesystem@0.1.2
```

**Concise (Header + TSV):**
```
#path
"/path/to/tools/appcypher/filesystem@0.1.2"
```

Header + TSV format. Path is quoted since it may contain spaces.

---

#### 18. `tool remove`

**Current:**
```
  ✓ Removed appcypher/filesystem
```

**Concise:**
```
(empty - exit code 0 indicates success)
```

---

#### 19. `tool download`

**Current:**
```
  → Downloading appcypher/filesystem@0.1.2...
  ✓ Downloaded to ./appcypher-filesystem-0.1.2.mcpb
```

**Concise (Header + TSV):**
```
#path
./appcypher-filesystem-0.1.2.mcpb
```

Header + TSV format.

---

#### 20. `tool publish`

**Current:**
```
  → Publishing to registry...
  ✓ Published appcypher/filesystem@0.1.2
    URL: https://tool.store/tools/appcypher/filesystem
```

**Concise (Header + TSV):**
```
#ref	url
appcypher/filesystem@0.1.2	https://tool.store/tools/appcypher/filesystem
```

Header + TSV format.

---

#### 21. `tool run --list`

**Current:**
```
  Available scripts:
    build    cargo build --release
    test     cargo test
```

**Concise (Header + TSV):**
```
#script	command
build	"cargo build --release"
test	"cargo test"
```

Header + TSV format. Commands are quoted since they may contain spaces.

---

#### 22. Error messages (all commands)

**Current:**
```
error: Search failed: error sending request for url (https://tool.store/api/v1/search)
```

**Concise (Header + TSV on stderr):**
```
#type	code	message
error	search_failed	"error sending request for url"
```

Header + TSV format. Messages are quoted since they may contain spaces.

---

## Part 2: `tool grep` Command

### Purpose

Search tool schemas by pattern. Enables agents to discover relevant tools without loading full schemas into context.

### Synopsis

```
tool grep [OPTIONS] <PATTERN> [TOOL]

Arguments:
  <PATTERN>  Regex pattern to match against tool names, descriptions, or parameter names
  [TOOL]     Tool reference or path (default: search all installed tools)

Options:
  -n, --name         Search tool names only
  -d, --description  Search descriptions only
  -p, --params       Search parameter names only
  -i, --ignore-case  Case-insensitive search
  -l, --list         List matching tool names only (no details)
      --json         JSON output
  -h, --help         Print help
```

Note: Use the global `-c/--concise` flag for concise output (e.g., `tool -c grep file`).

### Examples

#### Search all tools for "file" pattern (Normal mode):
```
$ tool grep file
```

**Output:**
```
18 matches in appcypher/filesystem:

  filesystem__write
    ◆ "Write content to a file. Overwrites existing content."
    → file_path: "Absolute path to the file to write"
    → content: "Content to write to the file"

  filesystem__read
    ◆ "Read a file from the local filesystem."
    → file_path: "Absolute path to the file to read"

  filesystem__glob
    ◆ "Find files matching a glob pattern."
    ← files: "List of matching file paths"

  filesystem__grep
    ◆ "Search file contents using regex patterns."

◆ desc  → input  ← output
```

Only matched fields are shown. Legend at the bottom explains symbols.

#### Concise output (Header + TSV):
```
$ tool -c grep file
```

**Output:**
```
#toolset	appcypher/filesystem
#tool	type	field	text
filesystem__write	desc	-	"Write content to a file..."
filesystem__write	in	file_path	"Absolute path to the file to write"
filesystem__write	in	content	"Content to write to the file"
filesystem__read	desc	-	"Read a file from the local filesystem..."
filesystem__read	in	file_path	"Absolute path to the file to read"
filesystem__glob	desc	-	"Find files matching a glob pattern."
filesystem__glob	out	files	"List of matching file paths"
```

- `#toolset` header starts each toolset group
- `type`: `desc` (description), `in` (input param), `out` (output field)
- `field`: field name or `-` for description
- `text`: the matched content (quoted with backslash escaping)

#### List mode - show signatures (`-l`):
```
$ tool grep -l file
```

**Output (Normal):**
```
appcypher/filesystem:filesystem__write(file_path*, content*) -> {bytes_written}
appcypher/filesystem:filesystem__read(file_path*, limit?, offset?) -> {content, end_line, ...}
appcypher/filesystem:filesystem__glob(pattern*, path?) -> {files}
appcypher/filesystem:filesystem__grep(pattern*, path?, ...) -> {matches, total, truncated}
```

**Output (Concise):**
```
#tool
appcypher/filesystem:filesystem__write(file_path*, content*) -> {bytes_written}
appcypher/filesystem:filesystem__read(file_path*, limit?, offset?) -> {content, end_line, ...}
appcypher/filesystem:filesystem__glob(pattern*, path?) -> {files}
appcypher/filesystem:filesystem__grep(pattern*, path?, ...) -> {matches, total, truncated}
```

Function signatures for matching tools. Deduplicated.

#### Search specific tool:
```
$ tool grep pattern appcypher/filesystem
```

**Output:**
```
2 matches in appcypher/filesystem:

  filesystem__glob
    → pattern: "The glob pattern to match files against"

  filesystem__grep
    → pattern: "The regular expression pattern to search for"

◆ desc  → input  ← output
```

#### JSON output:
```
$ tool grep --json file appcypher/filesystem
```

**Output:**
```json
[
  {
    "toolset": "appcypher/filesystem",
    "tool": "filesystem__write",
    "type": "desc",
    "field": null,
    "text": "Write content to a file..."
  },
  {
    "toolset": "appcypher/filesystem",
    "tool": "filesystem__write",
    "type": "in",
    "field": "file_path",
    "text": "Absolute path to the file to write"
  }
]
```

#### Search parameter names only:
```
$ tool grep -p path
```

**Output:**
```
5 matches in appcypher/filesystem:

  filesystem__read
    → file_path: "Absolute path to the file to read"

  filesystem__write
    → file_path: "Absolute path to the file to write"

  filesystem__edit
    → file_path: "Absolute path to the file to edit"

  filesystem__glob
    → path: "Directory to search in"

  filesystem__grep
    → path: "File or directory to search in"

◆ desc  → input  ← output
```

---

## Part 3: Implementation Notes

### Adding Global Flags

In `lib/commands.rs`, add to the `Cli` struct:

```rust
#[derive(Parser)]
#[command(name = "tool")]
pub struct Cli {
    /// Concise output for AI agents (minimal formatting, machine-parseable).
    #[arg(short, long, global = true)]
    pub concise: bool,

    /// Suppress header line in concise mode (requires -c).
    #[arg(short = 'H', long, global = true)]
    pub no_header: bool,

    #[command(subcommand)]
    pub command: Command,
}
```

**Important:** The `--config` flag in `Info` and `Call` commands must use `-C` (uppercase) as its short form to avoid conflict with the global `-c` for `--concise`:

```rust
/// Configuration values (KEY=VALUE).
#[arg(short = 'C', long)]
config: Vec<String>,
```

### Concise Output Helper

Create a new module or extend `styles.rs`:

```rust
pub struct Output {
    pub concise: bool,
}

impl Output {
    pub fn line(&self, normal: &str, concise: &str) {
        if self.concise {
            println!("{}", concise);
        } else {
            println!("{}", normal);
        }
    }

    pub fn success(&self, normal: &str) {
        if !self.concise {
            println!("  {} {}", "✓".bright_green(), normal);
        }
    }

    pub fn json<T: Serialize>(&self, value: &T) {
        if self.concise {
            println!("{}", serde_json::to_string(value).unwrap());
        } else {
            println!("{}", serde_json::to_string_pretty(value).unwrap());
        }
    }
}
```

### grep Command Implementation

Add to `Command` enum:

```rust
/// Search tool schemas by pattern
Grep {
    /// Pattern to search for
    pattern: String,

    /// Tool reference (default: all installed)
    tool: Option<String>,

    /// Search tool names only
    #[arg(short = 'n', long)]
    name: bool,

    /// Search descriptions only
    #[arg(short = 'd', long)]
    description: bool,

    /// Search parameter names only
    #[arg(short = 'p', long)]
    params: bool,

    /// Case-insensitive search
    #[arg(short = 'i', long = "ignore-case")]
    ignore_case: bool,

    /// List matching tool names only
    #[arg(short = 'l', long)]
    list: bool,

    /// JSON output
    #[arg(long)]
    json: bool,
}
```

---

## Part 4: Summary of Changes

### New Global Options
| Option | Description |
|--------|-------------|
| `-c, --concise` | Enable concise output mode for AI agents |
| `-H, --no-header` | Suppress the `#` header line (requires `-c`) |

### New Command
| Command | Description |
|---------|-------------|
| `tool grep` | Search tool schemas by pattern |

### Output Format Changes (in concise mode)

All concise outputs use **Header + TSV format** with `#` prefixed header line, tab separators, and quoted strings for fields that may contain spaces.

| Command | Current | Concise (Header + TSV) |
|---------|---------|---------|
| `--help` | Multi-line with descriptions | Space-separated command list |
| `--tree` | Tree diagram | One line per command |
| `validate` | Error tree with emojis | `#code\tmsg\tloc\thelp` + TSV rows |
| `list` | Tree with descriptions | `#name\ttype\tpath` + TSV rows |
| `info` | Full tree with I/O schemas | `#tool` + function signatures `name(params) -> {outputs}` |
| `whoami` | Multi-line status | `#user\tregistry\tstatus` + TSV row |
| `detect` | Multi-line with hints | `#type\ttransport\tentry\tconfidence\tbuild` + TSV row |
| `call` | Wrapped JSON | Raw minified JSON only |
| `search` | Tree with details | `#ref\tdescription\tdownloads` + TSV rows |
| `pack` | Status messages | `#file\tsize` + TSV row |
| `init` | Status + next steps | `#file` + file names |
| `add` | Progress + status | `#path` + install path |
| `remove` | Confirmation | Empty (exit code only) |
| `download` | Progress + status | `#path` + file path |
| `publish` | Status + URL | `#ref\turl` + TSV row |
| `run --list` | Tree format | `#script\tcommand` + TSV rows |
| `grep` | Grouped by tool with `◆→←` symbols + legend | `#tool\ttype\tfield\ttext` TSV rows |
| `grep -l` | Function signatures | `#tool` + signatures |
| All JSON flags | Pretty-printed | Minified single line |

---

## Part 5: Agent Workflow Examples

### Example 1: Discover available tools
```bash
$ tool -c list
#name	type	path
appcypher/filesystem	stdio	"/path/to/filesystem@0.1.2"
```

### Example 2: Find tools that work with files
```bash
$ tool -c grep -l file
#tool
appcypher/filesystem:filesystem__read
appcypher/filesystem:filesystem__write
appcypher/filesystem:filesystem__edit
```

### Example 3: Get tool signature (skip header, filter with grep)
```bash
$ tool -c -H info appcypher/filesystem | grep filesystem__read
appcypher/filesystem:filesystem__read(file_path*: string, limit?: integer, offset?: integer) -> {content: string, end_line: integer, ...}
```

### Example 4: Call a tool
```bash
$ tool -c call appcypher/filesystem -m filesystem__read -p file_path=/etc/hosts
{"content":"...","end_line":12,"start_line":1,"total_lines":12,"truncated":false}
```

### Example 5: Install a new tool
```bash
$ tool -c add some-namespace/some-tool
#path
"/path/to/tools/some-namespace/some-tool@1.0.0"
```

This workflow allows agents to:
1. Discover installed tools with minimal context usage
2. Search for relevant tools by capability
3. Get concise function signatures (use `-H` to skip headers when piping)
4. Execute tools and parse JSON results
5. Install new tools as needed

All without loading full MCP schemas into context upfront.
