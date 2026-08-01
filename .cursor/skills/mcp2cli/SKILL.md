---
name: mcp2cli
description: >-
  Turn any MCP server, OpenAPI spec, or GraphQL endpoint into a CLI via the
  Rust mcp2cli binary. Use when calling MCP tools, listing OpenAPI/GraphQL
  operations, bake/@name configs, session daemons, or token-efficient agent
  discovery (--agent, --detail, --describe, spool). Triggers: mcp2cli, --mcp,
  --mcp-stdio, --spec, --graphql, bake, @name, session-start, --agent,
  "list tools from this server".
---

# mcp2cli (Rust)

Prefer this CLI over one-off HTTP clients when discovering or calling remote tools. No codegen.

## Binary

```bash
cargo build --release
export MCP2CLI=./target/release/mcp2cli   # or: cargo install --path .
```

## Workflow (token-efficient / agent)

Progressive discovery beats dumping full schemas (compete with Code Mode on tokens):

1. One source: `--mcp` | `--mcp-stdio` | `--spec` | `--graphql`
2. Discover lightly: `--list --json --detail names` or `--search PATTERN`
3. Describe one tool: `--describe TOOL` or `TOOL --help --json`
4. Run; prefer `--json` (content-only) or `--toon`; preview with `--head N`
5. If stdout has `"spooled": true`, **`rg` the `path`** — do not `cat` the whole file

```bash
$MCP2CLI --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" --agent --list
$MCP2CLI --mcp http://127.0.0.1:8000/mcp --list --json --detail names
$MCP2CLI --spec ./openapi.json --base-url https://api.example.com list-pets --limit 5
$MCP2CLI --graphql https://api.example.com/graphql --fields "id name" user --id 1
```

`--agent` / `MCP2CLI_AGENT=1`: JSON; `--search` ⇒ `--detail names` + `--top 20`; otherwise brief list; spill oversize to spool (64KiB).

| Flag | Role |
|------|------|
| `--detail names\|brief\|full` | List depth (`names`+`--top 20` on agent search; `brief` when browsing) |
| `--describe TOOL` | One-tool full schema JSON |
| `--envelope` | Full MCP `CallToolResult` (default `--json` is content-only) |
| `--toon` | Native TOON encode (falls back to JSON on failure) |
| `--max-bytes N` / `--inline` | Spill threshold / never spill |
| `--spool-clean` | Remove expired spool files |

Name lists may use **prefix compression**:
`{"groups":{"workers-scripts":["list","get"]},"names":["echo"]}` → tool ids `workers-scripts-list`, etc.

### How search stays fast (local index)

MCP `list_tools` returns **every** tool with full `inputSchema` (multi‑MB on fat APIs). mcp2cli:

1. Caches the full list under `$MCP2CLI_CACHE_DIR`
2. Writes a slim **v4** `*_tools_index.json` (sorted names + sparse tool-name overrides; postings rebuilt in RAM)
3. Warm `--search` / `--detail names` reads the **index**, not full schemas
4. With `--session`, the daemon keeps `CompactIndex` in RAM and serves `list_tools_light` (search in-process)

That cuts CPU and disk I/O; **agent tokens** still depend on stdout (`--top`, compression). True **server-side** search needs MCP protocol/server support we do not assume — the index is the portable stand-in.

## Sessions (Unix)

Warm one MCP connection; later calls are cheap IPC (`get_tool` for single-tool schema on call).

```bash
$MCP2CLI --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" --session-start myfs
$MCP2CLI --session myfs read-file --path /tmp/hello.txt
$MCP2CLI --session-list --json
$MCP2CLI --session-stop myfs
```

| Issue | Action |
|-------|--------|
| Socket missing | `--session-start` first |
| MCP child died | `--session-stop` then start again |
| Untrusted stdio server | add `--session-clean-env` |
| Idle leak | default 1800s; override `--session-idle-secs` / `MCP2CLI_SESSION_IDLE_SECS` |

## Bake

```bash
$MCP2CLI bake create myfs --mcp-stdio "npx -y …" --session myfs
$MCP2CLI @myfs --list    # needs session already started if bake has --session
$MCP2CLI bake show myfs  # secrets masked
```

## Auth

Always `env:` / `file:` — never literal secrets on argv.

```bash
$MCP2CLI --mcp https://mcp.example.com/mcp --auth-header "Authorization:env:API_TOKEN" --list
$MCP2CLI --mcp https://mcp.example.com/mcp --oauth --list
$MCP2CLI --mcp https://docs.mcp.cloudflare.com/mcp \
  --auth-header "Authorization:Bearer:env:CF_API_TOKEN" --agent --list --detail names
```

No mid-session OAuth refresh — restart the session if the token expires.

## Cloudflare token bench

```bash
export CF_API_TOKEN=…
export MCP2CLI_BENCH_CF=1
cargo test --test cloudflare_bench -- --ignored --nocapture
```

## Do / don't

- Do: `--agent`, `--detail names|brief`, `--describe`, `--json` / `--toon`, `--head`, `--refresh` when schemas stale
- Do: grep spooled paths; use `--envelope` only when you need the wire form
- Don't: dump full `--detail full` catalogs into agent context
- Don't: pass untrusted remote URLs (no SSRF sandbox)
- Don't: put `MCP2CLI_CACHE_DIR` on a world-writable share

## Wrap an API as a skill

1. List/probe with `$MCP2CLI … --agent --list` and real calls  
2. `bake create` with `env:` auth + filters  
3. `bake install NAME --dir <skill>/scripts/`  
4. Write `SKILL.md` with gotchas only — not a full `--help` dump  

Flag tables: [reference.md](reference.md).
