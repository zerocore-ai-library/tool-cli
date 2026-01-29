# Cookbooks: Building Context-Efficient Agents

We wrote a blog post about [building context-efficient agents](https://tool.store/blog/building-context-efficient-agents). This is the code that goes with it.

The gist: instead of loading all your MCP tool schemas upfront (which eats a ton of tokens), let the agent discover and call tools on-demand using bash. We call it code execution mode.

| Normal Agent | Code Mode Agent |
|:----------------------------------:|:--------------------------:|
| ![Normal agent context](../assets/normal-agent-context.png) | ![Code mode agent context](../assets/code-mode-agent-context.png) |
| 50% context (88k tokens) | 26% context (42k tokens) |

Same task. 46k fewer tokens. Not bad.

---

## Claude Code

```sh
cd cookbooks/claude-code
```

First, install open-data-mcp:

```sh
tool install ../mcps/open_data_mcp
```

### Normal agent

Add open-data-mcp to Claude so the agent can use it directly:

```sh
tool host add cc open-data-mcp -y && claude --agent normal-agent --dangerously-skip-permissions "Find the movie \"The Vast of Night\" and tell me about its director"
```

Type `/context` to see how many tokens you used.

### Code mode agent

Remove open-data-mcp from Claude. The agent only has bash now—it'll use `tool` CLI to discover and call open-data-mcp:

```sh
tool host remove cc open-data-mcp -y && claude --agent code-mode-agent --dangerously-skip-permissions "Find the movie \"The Vast of Night\" and tell me about its director"
```

Type `/context` again. Should be way lower.

---

## LangChain

```sh
cd cookbooks/langchain
```

Install open-data-mcp:

```sh
tool install ../mcps/open_data_mcp
```

Set your API key:

```sh
export ANTHROPIC_API_KEY=sk-ant...
```

### Normal agent

Connects to both bash and open-data MCP servers directly:

```sh
uv run --directory agent/ agent.py "Find the movie \"The Vast of Night\" and tell me about its director"
```

Check the token count in the output.

### Code mode agent

Only connects to the bash MCP server. Uses `tool` CLI to call open-data-mcp:

```sh
uv run --directory agent/ agent.py "Find the movie \"The Vast of Night\" and tell me about its director" --code
```

Compare the numbers. You'll see the difference.
