# skiff flags

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
| `--agent` / `SKIFF_AGENT=1` | JSON; search ⇒ names + `--top 20`; else brief; spool |
| `--json` | Structured JSON; MCP **content-only** |
| `--envelope` / `--full` | Full MCP CallToolResult |
| `--toon` | Native TOON (JSON fallback on encode fail) |
| `--head N` | Truncate top-level JSON arrays |
| `--max-bytes N` | Spill to `$SKIFF_CACHE_DIR/spool/` when over N (0 = never) |
| `--inline` | Never spill |
| `--spool-clean` | Delete expired spool files |

## Common globals

| Flag | Notes |
|------|-------|
| `--auth-header K:V` | Repeatable; values must use `env:` / `file:` (also `Bearer env:VAR` / `Bearer:env:VAR`) |
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

## Resources / prompts (MCP)

Work with `--mcp` / `--mcp-stdio` or `--session` (session not required):

| Flag | Notes |
|------|-------|
| `--list-resources` | List MCP resources |
| `--list-resource-templates` | List templates |
| `--read-resource URI` | Read one resource |
| `--list-prompts` | List prompts |
| `--get-prompt NAME` | Get prompt; add `--prompt-arg KEY=VALUE` |

## Doctor

```bash
skiff doctor
skiff doctor --json
```

Prints binary version/PATH, cache/config dirs, spool/oauth/session/bake summary, and install hints.

## Bake import

```bash
skiff bake import --dry-run
skiff bake import --from cursor|claude|codex|auto
skiff bake import --path ./mcp.json --name my-server --force
```

Maps Cursor/Claude `mcpServers` (JSON) and Codex `[mcp_servers]` (TOML) into bake. HTTP `url`/`serverUrl` → `--mcp`; `command`+`args` → `--mcp-stdio`. Header values like `Bearer ${TOKEN}` become `Bearer:env:TOKEN`.

## Shell completion

```bash
skiff completion bash|zsh|fish
# helpers used by scripts:
skiff __complete bake-names
skiff __complete bake-names-at
skiff __complete session-names
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime / MCP `isError` |
| 2 | Usage / bad args |

## Paths

| Env / path | Default |
|------------|---------|
| `SKIFF_CACHE_DIR` | `~/.cache/skiff` |
| `SKIFF_CONFIG_DIR` | `~/.config/skiff` |
| Sessions | `$SKIFF_CACHE_DIR/sessions/{name}.{sock,json,log}` |
| Spool | `$SKIFF_CACHE_DIR/spool/` |
| Tool index | `$SKIFF_CACHE_DIR/<key>_tools_index.json` (names and sparse overrides) |
| OAuth | `$SKIFF_CACHE_DIR/oauth/<hash>/` |
| Baked | `$SKIFF_CONFIG_DIR/baked.json` |

## Cloudflare bench

```bash
CF_API_TOKEN=… SKIFF_BENCH_CF=1 cargo test --test cloudflare_bench -- --ignored --nocapture
```
