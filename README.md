# mcp2cli (Rust)

Turn any **MCP server**, **OpenAPI** spec, or **GraphQL** endpoint into a CLI at runtime — zero codegen.

Rust port of [knowsuchagency/mcp2cli](https://github.com/knowsuchagency/mcp2cli).

## Install

```bash
cargo build --release
cargo install --path .          # puts mcp2cli on PATH
cargo test                      # needs python3 + mcp for fixtures
```

Binary: `./target/release/mcp2cli` or `mcp2cli` after install.

## Quick start

One source flag is required (`--spec` | `--mcp` | `--mcp-stdio` | `--graphql`).

```bash
# OpenAPI
mcp2cli --spec ./tests/fixtures/petstore.json --base-url http://127.0.0.1:8080/api/v1 --list
mcp2cli --spec ./openapi.json --base-url https://api.example.com list-pets --limit 5

# MCP stdio
mcp2cli --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --list
mcp2cli --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" \
  read-file --path /tmp/hello.txt

# MCP HTTP (auto → streamable, then SSE)
mcp2cli --mcp http://127.0.0.1:8000/mcp --list
mcp2cli --mcp http://127.0.0.1:8000/sse --transport sse echo --message hi

# GraphQL
mcp2cli --graphql http://127.0.0.1:4000 --list
mcp2cli --graphql http://127.0.0.1:4000 --fields "id name" user --id 1
```

### Sessions (Unix only)

Keeps one long-lived MCP client behind a Unix-domain socket so agents avoid paying `npx`/initialize on every call.

```bash
mcp2cli --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --session-start myfs
mcp2cli --session myfs --list
mcp2cli --session myfs echo --message hi
mcp2cli --session myfs --list-resources
mcp2cli --session-list --json
mcp2cli --session-stop myfs
```

| Detail | Behavior |
|--------|----------|
| Layout | `$MCP2CLI_CACHE_DIR/sessions/{name}.{sock,json,log}` |
| Security | Socket `0o600`, same-UID peer check; start config is a `0o600` file (not argv), unlinked after read |
| Idle | Default 30m (`--session-idle-secs` / `MCP2CLI_SESSION_IDLE_SECS`; `0` = never) |
| Bake | `bake create … --session NAME` so `@name` reuses the warm daemon |
| Failure | If the MCP child dies, IPC errors; `--session-stop` then `--session-start` again |

Not available on Windows.

### OAuth

HTTP sources only (not `--mcp-stdio`). Prefer streamable HTTP; SSE gets a Bearer at connect only (no mid-stream refresh).

```bash
mcp2cli --mcp https://mcp.example.com/mcp --oauth --list
mcp2cli --mcp https://mcp.example.com/mcp \
  --oauth-client-id env:OAUTH_CLIENT_ID \
  --oauth-client-secret env:OAUTH_CLIENT_SECRET \
  --oauth-flow client_credentials --list
mcp2cli --mcp https://mcp.example.com/mcp --oauth-clear
```

Tokens: `$MCP2CLI_CACHE_DIR/oauth/` (default `~/.cache/mcp2cli/oauth/`).

### Bake / `@name`

```bash
mcp2cli bake create petstore --spec ./tests/fixtures/petstore.json --methods GET,POST
mcp2cli bake create mytools \
  --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --exclude deploy --session myfs

mcp2cli @petstore --list
mcp2cli @mytools echo --message hi   # after --session-start myfs

mcp2cli bake list
mcp2cli bake show petstore           # secrets masked
mcp2cli bake update petstore --cache-ttl 7200
mcp2cli bake remove petstore
mcp2cli bake install mytools --dir ./scripts/
```

Configs: `$MCP2CLI_CONFIG_DIR/baked.json` (default `~/.config/mcp2cli/baked.json`).

## Security

- **Secrets:** use `env:` / `file:` prefixes on `--auth-header` and OAuth flags — never literal tokens on argv.
- **Trust:** treat remote MCP/API responses as untrusted input.
- **URLs:** remote `--spec` / `--mcp` / `--graphql` fetches are **not** SSRF-sandboxed; only pass URLs you trust.
- **Cache dir:** do not point `MCP2CLI_CACHE_DIR` at a shared world-writable path (sessions and OAuth store secrets there).
- **Sessions:** same-UID local IPC only; `--session-clean-env` for untrusted stdio servers.

## Agent skill

Cursor skill for agents: [`.cursor/skills/mcp2cli/`](.cursor/skills/mcp2cli/).

Token-efficient agent path: `--agent` (or `MCP2CLI_AGENT=1`) → progressive `--detail names|brief` → `--describe` / `TOOL --help --json` → `--json` or `--toon`. Oversized results spill to `$MCP2CLI_CACHE_DIR/spool/` with a small stdout pointer for `rg`.

Cloudflare MCP byte bench (optional):

```bash
CF_API_TOKEN=… MCP2CLI_BENCH_CF=1 cargo test --test cloudflare_bench -- --ignored --nocapture
```

Rust vs Python multi-run comparison (dataframe + optional CSV; needs `pandas` and `uvx mcp2cli`):

```bash
cargo build --release
python3 scripts/bench_vs_python.py --runs 10
# MCP2CLI_RUST_BIN=… MCP2CLI_PYTHON_BIN="uvx mcp2cli" python3 scripts/bench_vs_python.py --csv /tmp/bench.csv
```

## Status / limits

Shipped: OpenAPI, MCP stdio/HTTP (streamable + SSE), OAuth, GraphQL, sessions (Unix), bake/`@name`, list/search/output flags, native `--toon`, `--envelope`, spool overflow, `--agent` defaults.

Still thin: no Windows sessions; no mid-daemon OAuth refresh (restart the session if the token TTL is shorter than idle).

## License

MIT — see [LICENSE](LICENSE). Original Python project © Stephan Fitzpatrick / knowsuchagency.
