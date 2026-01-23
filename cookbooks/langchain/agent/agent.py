"""
Basic AI agent using MCP with LangChain.

Usage:
    export OPENAI_API_KEY=<your_key>
    # or
    export ANTHROPIC_API_KEY=<your_key>

    uv run agent.py
"""

import asyncio
import os
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
log.addHandler(RichHandler(rich_tracebacks=True, show_path=False, omit_repeated_times=False))

# Suppress noisy loggers
logging.getLogger("httpx").setLevel(logging.WARNING)
logging.getLogger("httpcore").setLevel(logging.WARNING)
logging.getLogger("openai").setLevel(logging.WARNING)
logging.getLogger("anthropic").setLevel(logging.WARNING)
logging.getLogger("mcp").setLevel(logging.WARNING)

console = Console()


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


async def run():
    model, model_name, provider, model_id = get_model()

    # MCP server paths - relative to this file's parent directory (agent/)
    # The sibling directories are bash_mcp/ and open_data_mcp/
    base_dir = Path(__file__).parent.parent
    bash_server_path = base_dir / "bash_mcp" / "server.py"
    open_data_server_path = base_dir / "open_data_mcp" / "server.py"

    log.info("Starting MCP servers...")

    client = MultiServerMCPClient(
        {
            "bash": {
                "command": "uv",
                "args": ["run", "--directory", str(bash_server_path.parent), str(bash_server_path)],
                "transport": "stdio",
            },
            "open_data": {
                "command": "uv",
                "args": ["run", "--directory", str(open_data_server_path.parent), str(open_data_server_path)],
                "transport": "stdio",
            },
        }
    )

    tools = await client.get_tools()
    tool_names = ", ".join(t.name for t in tools)
    agent = create_agent(model, tools)

    tools_tokens = await count_tools_tokens(agent, tools, provider, model_id)
    log.info(f"Model: {model_name} | Tools: {len(tools)} ({tools_tokens:,} tokens)")
    log.info(f"Tools: {tool_names}")
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
    asyncio.run(run())


if __name__ == "__main__":
    main()
