"""
MCP server that provides bash execution.
"""

import logging
import subprocess

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("Bash")

logging.disable(logging.INFO)


@mcp.tool()
def exec(command: str, timeout: int = 120) -> str:
    """
    Execute a bash command and return its output.

    Args:
        command: The shell command to execute
        timeout: Timeout in seconds (default 120)

    Returns:
        Command output (stdout + stderr)
    """
    try:
        result = subprocess.run(
            command,
            shell=True,
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
