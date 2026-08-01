# mcp2cli flags

## Sources (exactly one)

| Flag | Role |
|------|------|
| `--mcp URL` | MCP HTTP (`auto` → streamable, else SSE) |
| `--mcp-stdio CMD` | MCP child process |
| `--spec URL\|FILE` | OpenAPI |
| `--graphql URL` | GraphQL introspection |

## Common globals

| Flag | Notes |
|------|-------|
| `--list` / `--search P` | Discover (search implies list) |
| `--auth-header K:V` | Repeatable; `env:` / `file:` on values |
| `--env K=V` | Stdio child env |
| `--transport auto\|sse\|streamable` | MCP HTTP |
| `--base-url` | OpenAPI server override |
| `--fields "…"` | GraphQL selection set |
| `--pretty` / `--json` / `--raw` / `--head N` | Output |
| `--refresh` / `--cache-ttl N` | Cache (default TTL 3600) |
| `--oauth*` | HTTP only — not with `--mcp-stdio` |

## Sessions (Unix)

| Flag | Notes |
|------|-------|
| `--session-start NAME` | Needs `--mcp` or `--mcp-stdio` |
| `--session NAME` | Tools / resources / prompts via daemon |
| `--session-stop NAME` | SIGTERM → SIGKILL |
| `--session-list` | Add `--json` for agents |
| `--session-idle-secs N` | Default 1800; `0` = never |
| `--session-clean-env` | Minimal stdio env |

## Paths

| Env / path | Default |
|------------|---------|
| `MCP2CLI_CACHE_DIR` | `~/.cache/mcp2cli` |
| `MCP2CLI_CONFIG_DIR` | `~/.config/mcp2cli` |
| Sessions | `$MCP2CLI_CACHE_DIR/sessions/{name}.{sock,json,log}` |
| OAuth | `$MCP2CLI_CACHE_DIR/oauth/<hash>/` |
| Baked | `$MCP2CLI_CONFIG_DIR/baked.json` |
