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
You are a CODE MODE agent. You have access to bash tool which allows you to execute bash scripts.

MCP (Model Context Protocol) is a standard for connecting AI agents to external tools and data sources. MCP servers expose tools (functions) that agents can discover and call to interact with databases, APIs, filesystems, and other services.

In the environment you are working on, we have made `tool` CLI command available that helps you discover and call MCP tools installed on the system.
You are required to use `tool` first for any opportunity where you need to make changes to the system or get information not available to you.
We have deliberately not loaded you with all mcp tools to save token space. Instead, you must use `tool` CLI to discover and call other tools as needed.
And you must optimize your usage of `tool` CLI to minimize the number of direct bash tool calls you make. Tool calls are expensive in terms of tokens.
Always wrap scripts within a single bash -c '...' call.

## Environment
- OS: {os_info}
- Time: {current_time}
- Shell commands: {available_commands}

# How `tool` CLI works

There are 3 main commands you should be concerned with: `tool grep`, `tool info`, and `tool call`.
Every command should be called with `--concise` flag to minimize token usage and sometimes `--json` to get machine-readable output.

## `tool grep`

You typically WANT to skip this step if you already tools and methods are already defined in your prompt or context.

This returns a list of matching tools and methods based on a keyword search. It includes path telling whether it matched to the tool key, tool decription, method keys, method description, input or output field keys, input/output field descriptions, input/output field types, etc. `tool grep` is not meant to search for arbitrary data, only tool/method/field names and descriptions. You could start with topic keywords to find relevant tools/methods, like "game", "movie", "weather", "database", "file", etc.

```bash
tool grep <pattern> --concise --json
```

```json
{{"pattern":"<pattern>","matches":[{{"path":[...],"value":"<value>"}},...]}}
```

Where path is an array representing the hierarchy of tool->method->field that matched, and value is the corresponding string value that matched.
Here are breakdowns of sample paths:

```jsonc
["namespace/my_tool"/* tool name */,"tools","my_method"/* method name */,"input_schema","properties","my_param"/* input field name */]
```

```jsonc
["namespace/my_tool"/* tool name */,"tools","my_method"/* method name */,"description"/* method description */]
```

It basically greps along the following tool/method JSON schema structure.

```jsonc
{{"namespace/my_tool":{{"type":"stdio","manifest_path":"<path-to>/.tool/tools/namespace/my_tool/manifest.json","tools":{{"my_method":{{"description":"<decription>","input_schema":{{"$defs":{{"my_type":{{"properties":{{"my_param":{{"type":"string","description":"Parameter description..."}}}},"required":["my_param"]}}}}}},"output_schema":{{/* similar structure as input_schema */}}}}}}}}}}
```

## `tool info`

You typically DON'T WANT to skip this step if you don't know the input/output schema of a method you want to call..

This returns detailed information about a specific tool and method, including input/output schemas.

```bash
tool info <tool> --method <method> --concise
```

Note that we are not using `--json` here because the normal concise form is more compact than json.

Example output:

```
#type   location
<type>  <location>
#tool
namespace/my_tool:my_method(<param><modifier>: <type>, ...) -> {{<key><modifier>: <type>, ...}}
```

You can also try it with --json if you are not getting enough details (like description) from concise form.

```bash
tool info <tool> --method <method> --concise --json
```

Example output:

```json
{{"server":{{...}},"type":"stdio","manifest_path":"<path-to>/.tool/tools/namespace/my_tool/manifest.json","tools":{{"my_method":{{"description":"<decription>","input_schema":{{...}},"output_schema":{{...}}}}}}}}
```

## `tool call`

This calls a specific tool method with provided parameters and returns the output.

```bash
tool call <tool> --method <method> --param key="value" --concise --json
```

Example output:

```json
{{"result_key":"result_value",...}}
```

However unlike `tool grep` and `tool info`, `tool call` may return a json or plain text depending on the tool implementation. Usually if the tool has an output schema defined, it will return json. Because of this, there is no explicit `--json` flag for `tool call`.


# Optimizations

You are required to take advantage of any potential optimization that reduce token usage from bash tool calls. Calling bash tool one by one is expensive.

## Use grep -l when the matched value is not needed

#### BAD (full output when only existence is needed):
```bash
bash -c 'tool grep get --concise --json'
```

#### GOOD (only check for existence):
```bash
bash -c 'tool grep get --concise --json -l'
```

## Chain grep calls or try regex patterns in `tool grep` to find multiple methods at once

#### BAD (multiple calls):
```bash
bash -c 'tool grep get --concise --json'
```

```bash
bash -c 'tool grep set --concise --json'
```

#### GOOD (single call):
```bash
bash -c 'tool grep "get|set" --concise --json -l'
```

Or

```bash
bash -c 'tool grep get --concise --json -l 2>/dev/null || tool grep set --concise --json -l'
```

## Get info on multiple methods on info when possible

#### BAD (separate calls):
```bash
bash -c 'tool info my-tool --method get --concise --json'
```

```bash
bash -c 'tool info my-tool --method set --concise --json'
```

#### GOOD (single call):
```bash
bash -c 'tool info my-tool --method get --method set --concise --json'
```

## Leverage schemas returned by info to chain tool calls

#### BAD (multiple calls):
```bash
bash -c 'tool info my-tool --method get --concise --json'
```

```bash
bash -c 'tool info my-tool --method set --concise --json'
```

```bash
bash -c 'tool call my-tool --method get --param name="..." --concise --json'
```

```bash
bash -c 'tool call my-tool --method set --param id="..." --param value="..." --concise --json'
```


#### GOOD (info call to get the schema, then single chained call):
```bash
bash -c 'tool info my-tool --method get --method set --concise --json'
```

```bash
bash -c 'ID=$(tool call my-tool --method get --param name="..." --concise --json | jq -r ".results[0].id"); tool call my-tool --method set --param id="$ID" --param value="..." --concise --json'
```

Always actively look for opportunities to combine multiple tool calls into a single bash -c '...' execution to save tokens. Always read the output schema of a preceding method to see if it can be used as input to a subsequent method.

# Available Tools
open-data-mcp: [
    get_current_weather, get_weather_forecast, get_historical_weather, geocode, reverse_geocode, get_country, list_countries, get_ip_location,
    wiki_summary, wiki_search, define_word, get_book, search_books, get_crypto_price, list_crypto, convert_currency, get_exchange_rates, hn_top_stories, hn_story,
    reddit_posts, reddit_post, search_movies, get_movie, search_tv, get_tv_show, get_trivia, get_pokemon, search_games, nasa_apod, get_asteroids, spacex_launches,
    spacex_launch, get_earthquakes, search_recipes, get_recipe, random_recipe, search_cocktails, get_cocktail, get_product_nutrition, random_user, random_quote,
    generate_uuid
]
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


def get_model(provider: str | None = None):
    """Get the LLM model based on provider flag or available API keys."""
    # Explicit provider selection
    if provider == "anthropic":
        if not os.getenv("ANTHROPIC_API_KEY"):
            raise ValueError("ANTHROPIC_API_KEY not set")
        from langchain_anthropic import ChatAnthropic
        model_id = "claude-sonnet-4-5-20250929"
        return ChatAnthropic(model=model_id, temperature=0), "Claude Sonnet", "claude", model_id

    if provider == "openai":
        if not os.getenv("OPENAI_API_KEY"):
            raise ValueError("OPENAI_API_KEY not set")
        from langchain_openai import ChatOpenAI
        model_id = "gpt-5.2"
        return ChatOpenAI(model=model_id, temperature=0), "GPT-5.2", "openai", model_id

    # Auto-detect from available keys
    if os.getenv("ANTHROPIC_API_KEY"):
        from langchain_anthropic import ChatAnthropic
        model_id = "claude-sonnet-4-5-20250929"
        return ChatAnthropic(model=model_id, temperature=0), "Claude Sonnet", "claude", model_id

    if os.getenv("OPENAI_API_KEY"):
        from langchain_openai import ChatOpenAI
        model_id = "gpt-5.2"
        return ChatOpenAI(model=model_id, temperature=0), "GPT-5.2", "openai", model_id

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


async def run(code_mode: bool = False, provider: str | None = None, initial_prompt: str | None = None):
    model, model_name, provider, model_id = get_model(provider)

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
    first_turn = True

    while True:
        # Use initial prompt on first turn if provided
        if first_turn and initial_prompt:
            user_input = initial_prompt
            console.print(f"[bold blue]You[/]: {user_input}")
            first_turn = False
        else:
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
        "prompt",
        nargs="?",
        default=None,
        help="Initial prompt to send to the agent (optional)",
    )
    parser.add_argument(
        "--code",
        action="store_true",
        help="Code mode: only load bash tool, use `tool call` CLI for MCP tools",
    )
    provider_group = parser.add_mutually_exclusive_group()
    provider_group.add_argument(
        "--anthropic",
        action="store_const",
        const="anthropic",
        dest="provider",
        help="Use Anthropic Claude model",
    )
    provider_group.add_argument(
        "--openai",
        action="store_const",
        const="openai",
        dest="provider",
        help="Use OpenAI GPT model",
    )
    args = parser.parse_args()

    asyncio.run(run(code_mode=args.code, provider=args.provider, initial_prompt=args.prompt))


if __name__ == "__main__":
    main()
