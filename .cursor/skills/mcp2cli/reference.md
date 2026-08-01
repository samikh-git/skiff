# mcp2cli flag cheat-sheet

## Sources (mutually exclusive)

| Flag | Meaning |
|------|---------|
| `--mcp URL` | MCP over HTTP (streamable / SSE) |
| `--mcp-stdio CMD` | MCP child process |
| `--spec URL\|FILE` | OpenAPI |
| `--graphql URL` | GraphQL introspection |

## Global options (common)

| Flag | Notes |
|------|-------|
| `--list` / `--search P` | Discover tools |
| `--auth-header K:V` | Repeatable; `env:` / `file:` value prefixes |
| `--env K=V` | Stdio child env (repeatable) |
| `--transport auto\|sse\|streamable` | MCP HTTP |
| `--base-url URL` | OpenAPI override |
| `--fields "…"` | GraphQL selection set |
| `--pretty` / `--json` / `--raw` / `--head N` | Output |
| `--refresh` / `--cache-ttl N` | Cache |
| `--oauth*` | HTTP OAuth (not with `--mcp-stdio`) |

## Sessions

| Flag | Notes |
|------|-------|
| `--session-start NAME` | Needs `--mcp` or `--mcp-stdio` |
| `--session NAME` | Route list/call/resources/prompts via daemon |
| `--session-stop NAME` | SIGTERM then SIGKILL |
| `--session-list` | Text or `--json` |
| `--session-idle-secs N` | Default 1800; `0` disables |
| `--session-clean-env` | Scrub stdio env |

## Paths

| Env | Default |
|-----|---------|
| `MCP2CLI_CACHE_DIR` | `~/.cache/mcp2cli` |
| `MCP2CLI_CONFIG_DIR` | `~/.config/mcp2cli` |
| Sessions | `$MCP2CLI_CACHE_DIR/sessions/{name}.{sock,json,log}` |
| Baked | `$MCP2CLI_CONFIG_DIR/baked.json` |
