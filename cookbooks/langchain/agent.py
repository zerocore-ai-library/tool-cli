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
        self.turn_count = 0

    def add_turn(self, input_tokens: int, output_tokens: int):
        self.total_input += input_tokens
        self.total_output += output_tokens
        self.turn_count += 1

    def reset(self):
        self.total_input = 0
        self.total_output = 0
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
    table.add_row("Total tokens", f"{tracker.total:,}")

    if tracker.turn_count > 0:
        avg = tracker.total // tracker.turn_count
        table.add_row("Avg per turn", f"{avg:,}")

    console.print(table)


async def main():
    model, model_name, provider, model_id = get_model()
    bash_server_path = Path(__file__).parent / "bash_server.py"
    open_data_server_path = Path(__file__).parent / "open_data_server.py"

    log.info("Starting MCP servers...")

    client = MultiServerMCPClient(
        {
            "bash": {
                "command": "uv",
                "args": ["run", str(bash_server_path)],
                "transport": "stdio",
            },
            "open_data": {
                "command": "uv",
                "args": ["run", str(open_data_server_path)],
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

        with console.status("[cyan]Thinking...[/]", spinner="dots"):
            response = await agent.ainvoke({"messages": messages})
            # Extract usage from AIMessage objects in response
            for msg in response.get("messages", []):
                if isinstance(msg, AIMessage) and msg.usage_metadata:
                    turn_input += msg.usage_metadata.get("input_tokens", 0)
                    turn_output += msg.usage_metadata.get("output_tokens", 0)

        tracker.add_turn(turn_input, turn_output)

        # Build assistant response with tool calls
        from rich.console import Group
        parts = []
        current_tool = None

        for msg in response.get("messages", []):
            if isinstance(msg, AIMessage):
                if msg.tool_calls:
                    for tc in msg.tool_calls:
                        cmd = tc["args"].get("command", str(tc["args"]))
                        current_tool = {"cmd": cmd, "output": None}
                elif msg.content:
                    parts.append(Markdown(msg.content))
            elif isinstance(msg, ToolMessage) and current_tool:
                # Extract text from content
                if isinstance(msg.content, list):
                    output = "\n".join(
                        item.get("text", "") for item in msg.content if isinstance(item, dict)
                    )
                else:
                    output = msg.content
                output = output[:500] + "..." if len(output) > 500 else output

                # Create nested panel for tool call
                tool_panel = Panel(
                    f"[dim]{output}[/]",
                    title=f"[cyan]$ {current_tool['cmd']}[/]",
                    title_align="left",
                    border_style="dim",
                    padding=(0, 1),
                )
                parts.append(tool_panel)
                current_tool = None

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
        console.print(
            f"[dim]Tokens: {turn_input:,} in / {turn_output:,} out | "
            f"Total: {tracker.total:,}[/]"
        )
        console.print()


if __name__ == "__main__":
    asyncio.run(main())
