---
name: mcp2cli
description: >-
  Turn any MCP server, OpenAPI spec, or GraphQL endpoint into a CLI via the
  Rust mcp2cli binary. Use when calling MCP tools, listing OpenAPI/GraphQL
  operations, bake/@name configs, or session daemons. Triggers: mcp2cli,
  --mcp, --mcp-stdio, --spec, --graphql, bake, @name, session-start,
  "list tools from this server".
---

# mcp2cli (Rust)

Prefer this CLI over one-off HTTP clients when discovering or calling remote tools. No codegen.

## Binary

```bash
cargo build --release
export MCP2CLI=./target/release/mcp2cli   # or: cargo install --path .
```

## Workflow

1. One source: `--mcp` | `--mcp-stdio` | `--spec` | `--graphql`
2. Discover: `--list` / `--search PATTERN`
3. Inspect: `<cmd> --help`
4. Run with flags; agents: prefer `--json`, preview with `--head N`

```bash
$MCP2CLI --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" --list
$MCP2CLI --mcp http://127.0.0.1:8000/mcp --list
$MCP2CLI --spec ./openapi.json --base-url https://api.example.com list-pets --limit 5
$MCP2CLI --graphql https://api.example.com/graphql --fields "id name" user --id 1
```

## Sessions (Unix)

Warm one MCP connection; later calls are cheap IPC.

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
$MCP2CLI --mcp https://mcp.example.com/mcp \
  --oauth-client-id env:CID --oauth-client-secret env:CSEC \
  --oauth-flow client_credentials --list
```

No mid-session OAuth refresh — restart the session if the token expires.

## Do / don't

- Do: `--json`, `--head`, `--refresh` when schemas stale
- Don't: trust `--toon` yet (warns → JSON)
- Don't: pass untrusted remote URLs (no SSRF sandbox)
- Don't: put `MCP2CLI_CACHE_DIR` on a world-writable share

## Wrap an API as a skill

1. List/probe with `$MCP2CLI … --list` and real calls  
2. `bake create` with `env:` auth + filters  
3. `bake install NAME --dir <skill>/scripts/`  
4. Write `SKILL.md` with gotchas only — not a full `--help` dump  

Flag tables: [reference.md](reference.md).
