#! /usr/bin/env python3
"""Minimal MCP legacy-SSE test server (upstream-compatible).

Prints:
  PORT=<n>
  URL=http://127.0.0.1:<n>/sse
"""

from __future__ import annotations

import asyncio
import socket
import sys

from mcp.server import Server
from mcp.types import TextContent, Tool

app = Server("test-http-sse-server")


@app.list_tools()
async def list_tools():
    return [
        Tool(
            name="echo",
            description="Echo back the input",
            inputSchema={
                "type": "object",
                "properties": {
                    "message": {"type": "string", "description": "Message to echo"},
                },
                "required": ["message"],
            },
        ),
        Tool(
            name="add_numbers",
            description="Add two numbers",
            inputSchema={
                "type": "object",
                "properties": {
                    "a": {"type": "integer", "description": "First number"},
                    "b": {"type": "integer", "description": "Second number"},
                },
                "required": ["a", "b"],
            },
        ),
    ]


@app.call_tool()
async def call_tool(name: str, arguments: dict):
    if name == "echo":
        return [TextContent(type="text", text=arguments.get("message", ""))]
    if name == "add_numbers":
        result = arguments.get("a", 0) + arguments.get("b", 0)
        return [TextContent(type="text", text=str(result))]
    return [TextContent(type="text", text=f"Unknown tool: {name}")]


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


async def main() -> None:
    from mcp.server.sse import SseServerTransport
    from starlette.applications import Starlette
    from starlette.routing import Mount, Route
    import uvicorn

    sse = SseServerTransport("/messages/")

    async def handle_sse(request):
        async with sse.connect_sse(
            request.scope, request.receive, request._send
        ) as (read, write):
            await app.run(read, write, app.create_initialization_options())

    starlette_app = Starlette(
        routes=[
            Route("/sse", endpoint=handle_sse),
            Mount("/messages/", app=sse.handle_post_message),
        ],
    )

    port = find_free_port()
    print(f"PORT={port}", flush=True)
    print(f"URL=http://127.0.0.1:{port}/sse", flush=True)

    config = uvicorn.Config(starlette_app, host="127.0.0.1", port=port, log_level="error")
    server = uvicorn.Server(config)
    await server.serve()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        sys.exit(0)
