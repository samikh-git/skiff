---
name: skiff
description: >-
  Turn any MCP server, OpenAPI spec, or GraphQL endpoint into a CLI via the
  Rust skiff binary. Use when calling MCP tools, listing OpenAPI/GraphQL
  operations, bake/@name configs, session daemons, or token-efficient agent
  discovery (--agent, --detail, --describe, spool). Triggers: skiff, --mcp,
  --mcp-stdio, --spec, --graphql, bake, bake import, @name, session-start,
  --agent, "list tools from this server", "import mcp.json".
---

# skiff

Prefer this CLI over one-off HTTP clients when discovering or calling remote tools. No codegen.

## Binary

Prefer `skiff` on PATH (do **not** require a local cargo build):

```bash
# Homebrew
brew tap samikh-git/tools && brew install skiff

# crates.io (package skiff-cli; binary is skiff)
cargo install skiff-cli

# verify
skiff doctor
skiff --version
```

In this repo only, fall back to `./target/release/skiff` after `cargo build --release`.

Optional: `export SKIFF=skiff` if a script expects `$SKIFF`; otherwise call `skiff` directly.

### First run

```bash
npx skills add samikh-git/skiff   # this skill
brew install skiff                # or: cargo install skiff-cli
skiff doctor
skiff --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" --agent --list
```

## Workflow

Use progressive discovery to keep output focused:

1. One source: `--mcp` | `--mcp-stdio` | `--spec` | `--graphql`
2. Discover lightly: `--list --json --detail names` or `--search PATTERN`
3. Describe one tool: `--describe TOOL` or `TOOL --help --json`
4. Run; prefer `--json` (content-only) or `--toon`; preview with `--head N`
5. If stdout has `"spooled": true`, **`rg` the `path`** — do not `cat` the whole file

```bash
skiff --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" --agent --list
skiff --mcp http://127.0.0.1:8000/mcp --list --json --detail names
skiff --spec ./openapi.json --base-url https://api.example.com list-pets --limit 5
skiff --graphql https://api.example.com/graphql --fields "id name" user --id 1
```

`--agent` / `SKIFF_AGENT=1`: JSON; `--search` ⇒ `--detail names` + `--top 20`; otherwise brief list; spill oversize to spool (64KiB).

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

### Local discovery cache

Warm `--search` and `--detail names` use the local catalog index. With `--session`,
name-only discovery uses the daemon's in-memory catalog.

## Canonical agent playbook (Cloudflare Docs MCP)

Warm session + progressive discovery against a fat catalog:

```bash
export CF_API_TOKEN=…   # Cloudflare API token with MCP access

skiff bake create cfdocs \
  --mcp https://docs.mcp.cloudflare.com/mcp \
  --auth-header "Authorization:Bearer:env:CF_API_TOKEN" \
  --session cfdocs --force

skiff --mcp https://docs.mcp.cloudflare.com/mcp \
  --auth-header "Authorization:Bearer:env:CF_API_TOKEN" \
  --session-start cfdocs

skiff @cfdocs --agent --search workers
skiff @cfdocs --describe search_cloudflare_documentation
skiff @cfdocs --agent search_cloudflare_documentation --query "workers kv"
# if stdout has "spooled": true → rg the path; do not cat the whole file

skiff --session-stop cfdocs
```

Local no-token demo: `examples/agent_workflow.sh` (stdio fixture server).

## Sessions (Unix)

Warm one MCP connection; later calls are cheap IPC (`get_tool` for single-tool schema on call).

```bash
skiff --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" --session-start myfs
skiff --session myfs read-file --path /tmp/hello.txt
skiff --session-list --json
skiff --session-stop myfs
```

| Issue | Action |
|-------|--------|
| Socket missing | `--session-start` first |
| MCP child died / auth failed | `--session-stop` then `--session-start` again |
| Untrusted stdio server | add `--session-clean-env` |
| Idle leak | default 1800s; override `--session-idle-secs` / `SKIFF_SESSION_IDLE_SECS` |

HTTP sessions with `--oauth` refresh the Bearer token in-daemon before RPC when the cached token rotates.

## Resources / prompts

Works with `--mcp` / `--mcp-stdio` **or** `--session` (no session required):

```bash
skiff --mcp-stdio "…" --list-resources --json
skiff --mcp-stdio "…" --read-resource "file:///tmp/hello.txt" --json
skiff --mcp-stdio "…" --list-prompts --json
skiff --mcp-stdio "…" --get-prompt echo --prompt-arg message=hi --json
```

## Bake

```bash
skiff bake create myfs --mcp-stdio "npx -y …" --session myfs
skiff bake import --dry-run          # Cursor / Claude / Codex MCP configs
skiff bake import --from cursor
skiff bake import --path ./mcp.json --name server-key
skiff @myfs --list    # needs session already started if bake has --session
skiff bake show myfs  # secrets masked
```

## Auth

Always `env:` / `file:` — never literal secrets on argv.

```bash
skiff --mcp https://mcp.example.com/mcp --auth-header "Authorization:env:API_TOKEN" --list
skiff --mcp https://mcp.example.com/mcp --oauth --list
skiff --mcp https://docs.mcp.cloudflare.com/mcp \
  --auth-header "Authorization:Bearer:env:CF_API_TOKEN" --agent --list --detail names
```

## Cloudflare token bench

```bash
export CF_API_TOKEN=…
export SKIFF_BENCH_CF=1
cargo test --test cloudflare_bench -- --ignored --nocapture
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Runtime failure, including MCP `isError: true` |
| 2 | Usage / bad args (unknown tool, invalid flags) |

## Do / don't

- Do: `--agent`, `--detail names|brief`, `--describe`, `--json` / `--toon`, `--head`, `--refresh` when schemas stale
- Do: `skiff doctor` when install/path/cache looks wrong; rebuild/reinstall if PATH binary lags the skill
- Do: grep spooled paths; use `--envelope` only when you need the wire form
- Don't: dump full `--detail full` catalogs into agent context
- Don't: pass untrusted remote URLs (no SSRF sandbox)
- Don't: put `SKIFF_CACHE_DIR` on a world-writable share
- Don't: put literal secrets on argv — use `env:` / `file:` (including `Authorization:Bearer:env:VAR`)

## Generating a skill from an API

When the user asks to create a skill from an MCP server, OpenAPI spec, or GraphQL endpoint:

1. **Discover lightly** (keep tokens low):
   ```bash
   skiff --mcp https://target.example.com/mcp --agent --list
   skiff --mcp https://target.example.com/mcp --agent --search PATTERN
   ```
2. **Describe** only the tools you need:
   ```bash
   skiff --mcp https://target.example.com/mcp --describe TOOL
   # or: skiff … TOOL --help --json
   ```
3. **Probe** with real calls. Prefer `--agent` / `--json`; use `--head N` for large arrays. If stdout has `"spooled": true`, **`rg` the `path`** — do not `cat` the whole file.
   Test for: oversized fields, date formats, pagination, error messages, binary vs text, scope surprises.
4. **Bake** connection settings (secrets via `env:` / `file:` only):
   ```bash
   skiff bake create myapi \
     --mcp https://target.example.com/mcp \
     --auth-header "Authorization:Bearer:env:MYAPI_TOKEN" \
     --exclude "delete-*" --session myapi --force
   ```
5. **Warm session** (stdio / fat catalogs):
   ```bash
   skiff --mcp https://target.example.com/mcp \
     --auth-header "Authorization:Bearer:env:MYAPI_TOKEN" \
     --session-start myapi
   skiff @myapi --agent --search PATTERN
   ```
6. **Install** a wrapper into the skill scripts dir:
   ```bash
   skiff bake install myapi --dir .cursor/skills/myapi/scripts/
   ```
7. **Write `SKILL.md`** that teaches another agent how to use this API. Focus on knowledge **not** in `--help`:
   - Frontmatter `name` / `description` / allowed tools for Bash
   - Core workflow using the baked wrapper (`--agent`, `--describe`, spool/`rg`)
   - Before-query checklist (pagination, `--head`, date formats)
   - Anti-patterns & gotchas found while probing
   - Output processing (`--json` content-only vs `--envelope`, `--toon`, spool)

**Knowledge delta:** do not duplicate parameter listings from `--help`. Document what actually matters for common tasks, surprising defaults, and rate/size limits.

Flag tables: [reference.md](reference.md). Competitive backlog: repo [ROADMAP.md](../../ROADMAP.md).

