#! /usr/bin/env python3
"""OAuth2 AS + Bearer-gated MCP streamable endpoint for client-credentials tests.

Prints:
  PORT=<n>
  URL=http://127.0.0.1:<n>/mcp
  CLIENT_ID=test-client
  CLIENT_SECRET=test-secret
"""

from __future__ import annotations

import asyncio
import base64
import socket
import sys
from urllib.parse import parse_qs

from starlette.middleware.base import BaseHTTPMiddleware
from starlette.requests import Request
from starlette.responses import JSONResponse, Response
from starlette.routing import Route

CLIENT_ID = "test-client"
CLIENT_SECRET = "test-secret"
ACCESS_TOKEN = "test-access-token"


def find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def main() -> None:
    from mcp.server.fastmcp import FastMCP
    import uvicorn

    port = find_free_port()
    base = f"http://127.0.0.1:{port}"
    mcp_url = f"{base}/mcp"

    mcp = FastMCP("oauth-test-server", host="127.0.0.1", port=port)

    @mcp.tool()
    def echo(message: str) -> str:
        """Echo back the input"""
        return message

    async def as_metadata(_request: Request) -> JSONResponse:
        return JSONResponse(
            {
                "issuer": base,
                "authorization_endpoint": f"{base}/authorize",
                "token_endpoint": f"{base}/token",
                "registration_endpoint": f"{base}/register",
                "response_types_supported": ["code"],
                "grant_types_supported": [
                    "authorization_code",
                    "client_credentials",
                    "refresh_token",
                ],
                "code_challenge_methods_supported": ["S256"],
                "token_endpoint_auth_methods_supported": [
                    "client_secret_post",
                    "client_secret_basic",
                    "none",
                ],
            }
        )

    async def protected_resource(_request: Request) -> JSONResponse:
        return JSONResponse(
            {
                "resource": mcp_url,
                "authorization_servers": [base],
                "scopes_supported": ["mcp"],
            }
        )

    async def token(request: Request) -> Response:
        body = (await request.body()).decode()
        params = parse_qs(body)
        grant = (params.get("grant_type") or [""])[0]
        cid = (params.get("client_id") or [""])[0]
        csec = (params.get("client_secret") or [""])[0]
        auth = request.headers.get("authorization", "")
        if auth.startswith("Basic "):
            try:
                raw = base64.b64decode(auth.split(" ", 1)[1]).decode()
                cid, csec = raw.split(":", 1)
            except Exception:
                pass
        if grant != "client_credentials":
            return JSONResponse({"error": "unsupported_grant_type"}, status_code=400)
        if cid != CLIENT_ID or csec != CLIENT_SECRET:
            return JSONResponse({"error": "invalid_client"}, status_code=401)
        return JSONResponse(
            {
                "access_token": ACCESS_TOKEN,
                "token_type": "bearer",
                "expires_in": 3600,
                "scope": "mcp",
            }
        )

    class RequireBearer(BaseHTTPMiddleware):
        async def dispatch(self, request, call_next):
            path = request.url.path
            if path.startswith("/mcp"):
                auth = request.headers.get("authorization", "")
                if auth != f"Bearer {ACCESS_TOKEN}":
                    return JSONResponse(
                        {"error": "unauthorized"},
                        status_code=401,
                        headers={
                            "WWW-Authenticate": (
                                f'Bearer realm="mcp", '
                                f'resource_metadata="{base}/.well-known/oauth-protected-resource"'
                            )
                        },
                    )
            return await call_next(request)

    streamable = mcp.streamable_http_app()
    oauth_routes = [
        Route("/.well-known/oauth-authorization-server", as_metadata),
        Route("/.well-known/oauth-authorization-server/mcp", as_metadata),
        Route("/.well-known/openid-configuration", as_metadata),
        Route("/.well-known/openid-configuration/mcp", as_metadata),
        Route("/.well-known/oauth-protected-resource", protected_resource),
        Route("/.well-known/oauth-protected-resource/mcp", protected_resource),
        Route("/token", token, methods=["POST"]),
    ]
    # Prepend OAuth routes; keep FastMCP lifespan.
    streamable.router.routes = oauth_routes + list(streamable.router.routes)
    streamable.add_middleware(RequireBearer)

    print(f"PORT={port}", flush=True)
    print(f"URL={mcp_url}", flush=True)
    print(f"CLIENT_ID={CLIENT_ID}", flush=True)
    print(f"CLIENT_SECRET={CLIENT_SECRET}", flush=True)

    config = uvicorn.Config(streamable, host="127.0.0.1", port=port, log_level="error")
    server = uvicorn.Server(config)
    try:
        asyncio.run(server.serve())
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
