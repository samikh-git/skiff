#! /usr/bin/env python3
"""Minimal MCP streamable-HTTP test server.

Prints:
  PORT=<n>
  URL=http://127.0.0.1:<n>/mcp
"""

from __future__ import annotations

import socket
import sys

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("test-http-server", host="127.0.0.1", port=0)


@mcp.tool()
def echo(message: str) -> str:
    """Echo back the input"""
    return message


@mcp.tool()
def add_numbers(a: int, b: int) -> str:
    """Add two numbers"""
    return str(a + b)


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def main() -> None:
    import asyncio

    import uvicorn

    port = find_free_port()
    # Mutate settings so streamable_http_app / uvicorn use this port.
    mcp.settings.port = port
    mcp.settings.host = "127.0.0.1"

    app = mcp.streamable_http_app()
    print(f"PORT={port}", flush=True)
    print(f"URL=http://127.0.0.1:{port}/mcp", flush=True)

    config = uvicorn.Config(app, host="127.0.0.1", port=port, log_level="error")
    server = uvicorn.Server(config)

    async def _serve() -> None:
        await server.serve()

    try:
        asyncio.run(_serve())
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
