"""
Basic AI agent using MCP with LangChain.

Usage:
    export OPENAI_API_KEY=<your_key>
    # or
    export ANTHROPIC_API_KEY=<your_key>

    uv run agent.py
    uv run agent.py --code  # Only bash tool, use `tool call` for MCP tools
"""

import argparse
import asyncio
import os
import platform
import shutil
from datetime import datetime
from pathlib import Path

from langchain_mcp_adapters.client import MultiServerMCPClient
from langchain.agents import create_agent
from langchain_core.messages import AIMessage, ToolMessage
from rich.console import Console
from rich.panel import Panel
from rich.prompt import Prompt
from rich.table import Table
from rich.markdown import Markdown
from rich.logging import RichHandler
import logging

# Set up our logger only
log = logging.getLogger("agent")
log.setLevel(logging.INFO)
log.addHandler(RichHandler(rich_tracebacks=True, show_path=False, omit_repeated_times=False, markup=True))

# Suppress noisy loggers
logging.getLogger("httpx").setLevel(logging.WARNING)
logging.getLogger("httpcore").setLevel(logging.WARNING)
logging.getLogger("openai").setLevel(logging.WARNING)
logging.getLogger("anthropic").setLevel(logging.WARNING)
logging.getLogger("mcp").setLevel(logging.WARNING)

console = Console()

CODE_MODE_TEMPLATE = """\
You are a CODE MODE agent with ONLY bash access. Use the `tool` CLI to discover and call MCP tools.

## Environment
- OS: {os_info}
- CPU: {cpu_info}
- Time: {current_time}
- Available commands: {available_commands}

## Quick Reference

```bash
tool grep <pattern> -c              # Find tools/methods matching pattern (includes descriptions!)
tool info <tool> -m <method> -c     # Get ONE method's signature
tool info <tool> --tools -c         # List ALL method signatures for a tool
tool call <tool> -m <method> -p key=value -c
```

## Output Formats

### tool grep output
```
#path    value
['open-data-mcp'].tools.search_movies    search_movies
['open-data-mcp'].tools.search_movies.description    "Search for movies by title..."
['open-data-mcp'].tools.get_movie    get_movie
['open-data-mcp'].tools.get_movie.description    "Get detailed movie information. Args: imdb_id..."
```
Descriptions tell you what parameters are needed. Often NO `tool info` call required.

### tool info -m output
```
open-data-mcp:search_movies(query*: string) -> {{query*: string, count*: integer, results*: {{imdb_id*: string, title*: string, year*: string}}[]}}
```
Shows exact input params and output fields. Use this to understand what to extract with jq.

### tool call output
Returns JSON. Use jq to extract fields for chaining:
```bash
tool call open-data-mcp -m search_movies -p query="Inception" -c | jq -r '.results[0].imdb_id'
```

## Workflow

### 1. DISCOVER with grep (often sufficient)
```bash
tool grep <keyword> -c
```
Descriptions usually tell you parameter names. If clear, skip to EXECUTE.

### 2. CLARIFY with info (only if needed)
```bash
tool info <tool> -m <method> -c    # Get ONE method signature - NOT the whole tool
```
Only use this if grep descriptions are unclear about parameters or output structure.

### 3. EXECUTE (REQUIRED: chain when possible)

When calls can be chained (output of one feeds into another), you MUST combine them in ONE script:

**BAD** (2 tool calls - wasteful):
```bash
# Call 1
tool call open-data-mcp -m search_movies -p query="Inception" -c | jq -r '.results[0].imdb_id'
# Returns: tt1375666

# Call 2
tool call open-data-mcp -m get_movie -p imdb_id="tt1375666" -c
```

**GOOD** (1 tool call - efficient):
```bash
ID=$(tool call open-data-mcp -m search_movies -p query="Inception" -c | jq -r '.results[0].imdb_id')
tool call open-data-mcp -m get_movie -p imdb_id="$ID" -c
```

Only use separate calls when you genuinely cannot predict what the next step will be.

## Rules

1. `tool grep` descriptions often have enough info - don't over-discover
2. NEVER run `tool info <tool> -c` without `-m <method>` - it dumps everything
3. Use jq to extract fields: `.field`, `.results[0].id`, `.items[] | .name`

## Anti-patterns (AVOID)

- `tool list -c` then `tool grep` then `tool info <tool> -c` then `tool call` (over-discovery)
- `tool info open-data-mcp -c` (dumps 40+ method signatures)
"""


def get_available_commands() -> list[str]:
    """Check which useful commands are available on the system."""
    commands_to_check = [
        ("jq", "JSON processor"),
        ("rg", "ripgrep (fast search)"),
        ("grep", "text search"),
        ("awk", "text processing"),
        ("sed", "stream editor"),
        ("cut", "column extraction"),
        ("sort", "sorting"),
        ("uniq", "deduplication"),
        ("xargs", "argument builder"),
        ("curl", "HTTP client"),
        ("head", "first lines"),
        ("tail", "last lines"),
        ("wc", "word/line count"),
    ]
    available = []
    for cmd, desc in commands_to_check:
        if shutil.which(cmd):
            available.append(f"{cmd} ({desc})")
    return available


def build_code_mode_prompt() -> str:
    """Build the code mode system prompt with environment info."""
    os_info = f"{platform.system()} {platform.release()}"
    cpu_info = platform.processor() or platform.machine()
    current_time = datetime.now().strftime("%Y-%m-%d %H:%M %Z")
    available_commands = ", ".join(get_available_commands())

    return CODE_MODE_TEMPLATE.format(
        os_info=os_info,
        cpu_info=cpu_info,
        current_time=current_time,
        available_commands=available_commands,
    )


class TokenTracker:
    """Track token usage across conversation turns."""

    def __init__(self, system_tokens: int = 0):
        self.system_tokens = system_tokens
        self.total_input = 0
        self.total_output = 0
        self.total_cached = 0
        self.turn_count = 0

    def add_turn(self, input_tokens: int, output_tokens: int, cached_tokens: int = 0):
        self.total_input += input_tokens
        self.total_output += output_tokens
        self.total_cached += cached_tokens
        self.turn_count += 1

    def reset(self):
        self.total_input = 0
        self.total_output = 0
        self.total_cached = 0
        self.turn_count = 0

    @property
    def total(self) -> int:
        return self.total_input + self.total_output


def get_model():
    """Get the LLM model based on available API keys."""
    if os.getenv("ANTHROPIC_API_KEY"):
        from langchain_anthropic import ChatAnthropic
        model_id = "claude-sonnet-4-5-20250929"
        return ChatAnthropic(model=model_id), "Claude Sonnet", "claude", model_id

    if os.getenv("OPENAI_API_KEY"):
        from langchain_openai import ChatOpenAI
        model_id = "gpt-4o"
        return ChatOpenAI(model=model_id), "GPT-4o", "openai", model_id

    raise ValueError("Set OPENAI_API_KEY or ANTHROPIC_API_KEY environment variable")


async def count_tools_tokens(agent, tools, provider: str, model_id: str) -> int:
    """Count tokens used by system prompt + tool definitions."""

    if provider == "claude":
        # Use Anthropic's official count_tokens API for exact count
        from anthropic import Anthropic
        tools_data = []
        for t in tools:
            params = {"type": "object", "properties": {}, "required": []}
            if hasattr(t, 'args_schema') and t.args_schema:
                if hasattr(t.args_schema, 'schema'):
                    params = t.args_schema.schema()
                elif isinstance(t.args_schema, dict):
                    params = t.args_schema
            tools_data.append({
                "name": t.name,
                "description": t.description or "",
                "input_schema": params
            })
        client = Anthropic()
        result = client.messages.count_tokens(
            model=model_id,
            messages=[{"role": "user", "content": "."}],
            tools=tools_data,
        )
        # Subtract 1 token for "."
        return result.input_tokens - 1

    # OpenAI: Make a minimal request to get exact token count from API
    # The input_tokens in the response includes system prompt + tools + user message
    response = await agent.ainvoke(
        {"messages": [{"role": "user", "content": "."}]}
    )
    # Extract input tokens from response
    for msg in response.get("messages", []):
        if isinstance(msg, AIMessage) and msg.usage_metadata:
            # Subtract ~2 tokens for "." + message framing overhead
            return msg.usage_metadata.get("input_tokens", 0) - 2
    return 0


def print_token_summary(tracker: TokenTracker):
    """Display full token usage summary."""
    table = Table(title="Token Usage", title_style="bold cyan", box=None)
    table.add_column("Metric", style="white")
    table.add_column("Value", justify="right", style="yellow")

    table.add_row("System (tools)", f"{tracker.system_tokens:,}")
    table.add_row("Turns", f"{tracker.turn_count}")
    table.add_row("Input tokens", f"{tracker.total_input:,}")
    table.add_row("Output tokens", f"{tracker.total_output:,}")
    table.add_row("Cached tokens", f"{tracker.total_cached:,}")
    table.add_row("Total tokens", f"{tracker.total:,}")

    if tracker.turn_count > 0:
        avg = tracker.total // tracker.turn_count
        table.add_row("Avg per turn", f"{avg:,}")

    console.print(table)


async def run(code_mode: bool = False):
    model, model_name, provider, model_id = get_model()

    # MCP server paths - relative to this file's parent directory (agent/)
    # The sibling directories are bash_mcp/ and open_data_mcp/
    base_dir = Path(__file__).parent.parent
    bash_server_path = base_dir / "bash_mcp" / "server.py"
    open_data_server_path = base_dir / "open_data_mcp" / "server.py"

    log.info("Starting MCP servers...")

    # Configure MCP servers based on mode
    servers = {
        "bash": {
            "command": "uv",
            "args": ["run", "--directory", str(bash_server_path.parent), str(bash_server_path)],
            "transport": "stdio",
        },
    }

    # In code mode, only bash tool is loaded - agent uses `tool call` CLI for other tools
    if not code_mode:
        servers["open_data"] = {
            "command": "uv",
            "args": ["run", "--directory", str(open_data_server_path.parent), str(open_data_server_path)],
            "transport": "stdio",
        }

    client = MultiServerMCPClient(servers)

    tools = await client.get_tools()
    tool_names = ", ".join(t.name for t in tools)
    agent = create_agent(model, tools)

    tools_tokens = await count_tools_tokens(agent, tools, provider, model_id)
    mode_label = " | [bold yellow]Code Mode[/]" if code_mode else ""
    log.info(f"Model: {model_name} | Tools: {len(tools)} | System + schemas: {tools_tokens:,} tokens{mode_label}")
    log.info(f"Tools: {tool_names}")

    # Initialize messages with system prompt for code mode
    if code_mode:
        system_prompt = build_code_mode_prompt()
        messages = [{"role": "system", "content": system_prompt}]
    else:
        messages = []

    tracker = TokenTracker(system_tokens=tools_tokens)

    while True:
        try:
            user_input = Prompt.ask("[bold blue]You[/]")
        except (EOFError, KeyboardInterrupt):
            console.print("\n[dim]Goodbye![/]")
            break

        if not user_input.strip():
            continue

        cmd = user_input.strip().lower()
        if cmd in ("quit", "exit", "q"):
            console.print("[dim]Goodbye![/]")
            break

        if cmd == "clear":
            # Preserve system message in code mode
            if code_mode:
                messages = [{"role": "system", "content": build_code_mode_prompt()}]
            else:
                messages = []
            tracker.reset()
            console.print("[green]Cleared[/]\n")
            continue

        if cmd == "tokens":
            print_token_summary(tracker)
            continue

        messages.append({"role": "user", "content": user_input})

        turn_input = 0
        turn_output = 0
        turn_cached = 0
        num_messages_before = len(messages)

        with console.status("[cyan]Thinking...[/]", spinner="dots"):
            response = await agent.ainvoke({"messages": messages})
            # Extract usage from AIMessage objects in response
            for msg in response.get("messages", []):
                if isinstance(msg, AIMessage) and msg.usage_metadata:
                    turn_input += msg.usage_metadata.get("input_tokens", 0)
                    turn_output += msg.usage_metadata.get("output_tokens", 0)
                    # Check for cached tokens (OpenAI and Anthropic have different formats)
                    input_details = msg.usage_metadata.get("input_token_details", {})
                    if input_details:
                        # OpenAI format
                        turn_cached += input_details.get("cache_read", 0)
                    # Anthropic format
                    turn_cached += msg.usage_metadata.get("cache_read_input_tokens", 0)

        tracker.add_turn(turn_input, turn_output, turn_cached)

        # Build assistant response with tool calls (only new messages from this turn)
        from rich.console import Group
        parts = []
        pending_tools = {}  # tool_call_id -> {name, args, batch}
        new_messages = response.get("messages", [])[num_messages_before:]
        batch_num = 0
        current_batch_panels = []  # Panels for current batch of parallel calls
        current_batch_ids = set()  # Tool IDs in current batch

        for msg in new_messages:
            if isinstance(msg, AIMessage):
                if msg.tool_calls:
                    # New batch of tool calls (could be 1 or many in parallel)
                    batch_num += 1
                    is_parallel = len(msg.tool_calls) > 1
                    current_batch_ids = set()
                    for tc in msg.tool_calls:
                        tool_id = tc.get("id", "")
                        tool_name = tc.get("name", "tool")
                        tool_args = tc.get("args", {})
                        pending_tools[tool_id] = {
                            "name": tool_name,
                            "args": tool_args,
                            "batch": batch_num,
                            "parallel": is_parallel,
                        }
                        current_batch_ids.add(tool_id)
                elif msg.content:
                    parts.append(Markdown(msg.content))
            elif isinstance(msg, ToolMessage):
                # Match tool result with its call by ID
                tool_id = msg.tool_call_id
                tool_info = pending_tools.get(tool_id, {})
                tool_name = tool_info.get("name", "tool")
                tool_args = tool_info.get("args", {})
                is_parallel = tool_info.get("parallel", False)
                batch = tool_info.get("batch", 0)

                # Extract text from content
                if isinstance(msg.content, list):
                    output = "\n".join(
                        item.get("text", "") for item in msg.content if isinstance(item, dict)
                    )
                else:
                    output = msg.content
                output = output[:500] + "..." if len(output) > 500 else output

                # Format args for display
                args_str = ", ".join(f"{k}={v!r}" for k, v in tool_args.items())

                # Create panel for tool call
                if is_parallel:
                    # Parallel call - use magenta style and collect in batch
                    tool_panel = Panel(
                        f"[dim]{output}[/]",
                        title=f"[magenta]{tool_name}({args_str})[/]",
                        title_align="left",
                        border_style="magenta dim",
                        padding=(0, 1),
                    )
                    current_batch_panels.append(tool_panel)
                    current_batch_ids.discard(tool_id)

                    # When all parallel calls in batch are done, wrap in group panel
                    if not current_batch_ids:
                        batch_group = Panel(
                            Group(*current_batch_panels),
                            title=f"[bold magenta]⚡ Parallel batch {batch}[/]",
                            title_align="left",
                            border_style="magenta",
                            padding=(0, 1),
                        )
                        parts.append(batch_group)
                        current_batch_panels = []
                else:
                    # Sequential call - use cyan style
                    tool_panel = Panel(
                        f"[dim]{output}[/]",
                        title=f"[cyan]{tool_name}({args_str})[/]",
                        title_align="left",
                        border_style="dim",
                        padding=(0, 1),
                    )
                    parts.append(tool_panel)

        assistant_message = response["messages"][-1]
        messages.append({"role": "assistant", "content": assistant_message.content})

        console.print()
        console.print(
            Panel(
                Group(*parts) if parts else "",
                title="[bold green]Assistant[/]",
                title_align="left",
                border_style="green",
                padding=(1, 2),
            )
        )
        cache_str = f" [green]({turn_cached:,} cached)[/]" if turn_cached > 0 else ""
        console.print(
            f"[dim]Tokens: {turn_input:,} in{cache_str} / {turn_output:,} out | "
            f"Total: {tracker.total:,}[/]"
        )
        console.print()


def main():
    parser = argparse.ArgumentParser(description="LangChain ReAct agent with MCP tools")
    parser.add_argument(
        "--code",
        action="store_true",
        help="Code mode: only load bash tool, use `tool call` CLI for MCP tools",
    )
    args = parser.parse_args()

    asyncio.run(run(code_mode=args.code))


if __name__ == "__main__":
    main()
