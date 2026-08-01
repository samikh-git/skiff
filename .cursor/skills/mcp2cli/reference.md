# mcp2cli flags

## Sources (exactly one)

| Flag | Role |
|------|------|
| `--mcp URL` | MCP HTTP (`auto` → streamable, else SSE) |
| `--mcp-stdio CMD` | MCP child process |
| `--spec URL\|FILE` | OpenAPI |
| `--graphql URL` | GraphQL introspection |

## Discovery / agent

| Flag | Notes |
|------|-------|
| `--list` / `--search P` | Discover (search implies list) |
| `--detail names\|brief\|full` | JSON list depth; `compact` ⇒ names; default `full` for `--json`, `brief` for `--agent` |
| `--describe TOOL` | One-tool full schema |
| `TOOL --help --json` | Same as describe for that tool |
| `--agent` / `MCP2CLI_AGENT=1` | JSON; search ⇒ names + `--top 20`; else brief; spool |
| `--json` | Structured JSON; MCP **content-only** |
| `--envelope` / `--full` | Full MCP CallToolResult |
| `--toon` | Native TOON (JSON fallback on encode fail) |
| `--head N` | Truncate top-level JSON arrays |
| `--max-bytes N` | Spill to `$MCP2CLI_CACHE_DIR/spool/` when over N (0 = never) |
| `--inline` | Never spill |
| `--spool-clean` | Delete expired spool files |

## Common globals

| Flag | Notes |
|------|-------|
| `--auth-header K:V` | Repeatable; `env:` / `file:` on values |
| `--env K=V` | Stdio child env |
| `--transport auto\|sse\|streamable` | MCP HTTP |
| `--base-url` | OpenAPI server override |
| `--fields "…"` | GraphQL selection set |
| `--pretty` / `--raw` | Output |
| `--refresh` / `--cache-ttl N` | Cache (default TTL 3600) |
| `--oauth*` | HTTP only — not with `--mcp-stdio` |

## Sessions (Unix)

| Flag | Notes |
|------|-------|
| `--session-start NAME` | Needs `--mcp` or `--mcp-stdio` |
| `--session NAME` | Tools / resources / prompts via daemon; calls use `get_tool` |
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
| Spool | `$MCP2CLI_CACHE_DIR/spool/` |
| Tool index | `$MCP2CLI_CACHE_DIR/<key>_tools_index.json` (names/descriptions) |
| OAuth | `$MCP2CLI_CACHE_DIR/oauth/<hash>/` |
| Baked | `$MCP2CLI_CONFIG_DIR/baked.json` |

## Cloudflare bench

```bash
CF_API_TOKEN=… MCP2CLI_BENCH_CF=1 cargo test --test cloudflare_bench -- --ignored --nocapture
```
