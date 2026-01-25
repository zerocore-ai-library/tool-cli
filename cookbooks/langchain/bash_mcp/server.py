"""
MCP server that provides bash execution.
"""

import logging
import subprocess

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("Bash")

logging.disable(logging.INFO)


@mcp.tool()
def exec(script: str, timeout: int = 120) -> str:
    """
    Execute a bash script. Supports multiple commands, pipes, variable assignments,
    and control flow. Combine related operations into a single script to minimize calls.

    Example:
        ID=$(tool call api -m search -p q="test" -c | jq -r '.id')
        tool call api -m get -p id="$ID" -c

    Args:
        script: The shell script to execute (can be multi-line)
        timeout: Timeout in seconds (default 120)

    Returns:
        Script output (stdout + stderr)
    """
    try:
        result = subprocess.run(
            ["bash", "-c", script],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        output = result.stdout + result.stderr
        return output.strip() if output.strip() else "(no output)"
    except subprocess.TimeoutExpired:
        return f"Command timed out after {timeout} seconds"
    except Exception as e:
        return f"Error: {e}"


def main():
    mcp.run(transport="stdio")


if __name__ == "__main__":
    main()
