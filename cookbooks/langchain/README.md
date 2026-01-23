# LangChain MCP Agent Cookbook

A LangChain ReAct agent with 43 MCP tools for accessing public data APIs.

## Project Structure

```
cookbooks/langchain/
├── agent/           # LangChain ReAct agent
├── bash_mcp/        # MCP server for bash execution (1 tool)
└── open_data_mcp/   # MCP server with 42 public API tools
```

## Quick Start

```bash
# Install dependencies for all projects
cd cookbooks/langchain/agent && uv sync
cd ../bash_mcp && uv sync
cd ../open_data_mcp && uv sync

# Run the agent
cd ../agent
export ANTHROPIC_API_KEY=<your_key>  # or OPENAI_API_KEY
uv run agent.py
```

## Architecture

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   agent/    │────▶│ MultiServerMCP   │────▶│ open_data_mcp/  │
│  (LangChain │     │    Client        │     │   (42 tools)    │
│   ReAct)    │     │                  │────▶│ bash_mcp/       │
└─────────────┘     └──────────────────┘     │   (1 tool)      │
                                             └─────────────────┘
```

1. Agent receives user query
2. LangChain ReAct loop reasons about which tools to use
3. MCP client routes tool calls to appropriate server
4. Servers fetch data from public APIs
5. Agent synthesizes response from tool outputs
