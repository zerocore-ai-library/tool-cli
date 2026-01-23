# LangChain MCP Agent

Interactive ReAct agent with rich terminal UI that connects to MCP servers.

## Quick Start

```bash
uv sync
export ANTHROPIC_API_KEY=<your_key>  # or OPENAI_API_KEY
uv run agent.py
```

## Example Queries

```
You: What's the weather in Tokyo?
You: How much is Bitcoin worth right now?
You: Tell me about the movie Inception
You: What are the top stories on Hacker News?
You: Give me a random cocktail recipe
You: What earthquakes happened this week?
You: Define the word "ephemeral"
You: Show me Pikachu's stats
```

## Commands

| Command | Description |
|---------|-------------|
| `clear` | Clear conversation history |
| `tokens` | Show token usage summary |
| `quit` / `exit` / `q` | Exit the agent |

## Configuration

The agent automatically detects which LLM to use based on environment variables:

| Variable | Model |
|----------|-------|
| `ANTHROPIC_API_KEY` | Claude Sonnet (preferred) |
| `OPENAI_API_KEY` | GPT-4o |

## MCP Servers

The agent connects to sibling MCP server projects:

- `../bash_mcp/` - Shell command execution
- `../open_data_mcp/` - 42 public API tools
