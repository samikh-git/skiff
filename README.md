# skiff

Turn any **MCP server**, **OpenAPI** spec, or **GraphQL** endpoint into a CLI at runtime — zero codegen. Built for agent workflows: progressive discovery, warm catalog index, sessions, and spool for huge payloads.

Inspired by [knowsuchagency/mcp2cli](https://github.com/knowsuchagency/mcp2cli) (Python prior art); skiff is a separate Rust project with its own focus on warm-path latency and token-efficient defaults.

## Install

```bash
# Homebrew (samikh-git tap)
brew tap samikh-git/tools
brew install skiff

# crates.io (package name skiff-cli; binary is skiff)
cargo install skiff-cli

# from source
cargo install --git https://github.com/samikh-git/skiff --locked
# or: cargo build --release && cargo install --path .
```

Binary: `skiff` (or `./target/release/skiff`).

## Quick start

One source flag is required (`--spec` | `--mcp` | `--mcp-stdio` | `--graphql`).

```bash
# OpenAPI
skiff --spec ./tests/fixtures/petstore.json --base-url http://127.0.0.1:8080/api/v1 --list
skiff --spec ./openapi.json --base-url https://api.example.com list-pets --limit 5

# MCP stdio
skiff --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --list
skiff --mcp-stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" \
  read-file --path /tmp/hello.txt

# MCP HTTP (auto → streamable, then SSE)
skiff --mcp http://127.0.0.1:8000/mcp --list
skiff --mcp http://127.0.0.1:8000/sse --transport sse echo --message hi

# GraphQL
skiff --graphql http://127.0.0.1:4000 --list
skiff --graphql http://127.0.0.1:4000 --fields "id name" user --id 1
```

### Sessions (Unix only)

Keeps one long-lived MCP client behind a Unix-domain socket so agents avoid paying `npx`/initialize on every call.

```bash
skiff --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --session-start myfs
skiff --session myfs --list
skiff --session myfs echo --message hi
skiff --session myfs --list-resources
skiff --session-list --json
skiff --session-stop myfs
```

| Detail | Behavior |
|--------|----------|
| Layout | `$SKIFF_CACHE_DIR/sessions/{name}.{sock,json,log}` |
| Security | Socket `0o600`, same-UID peer check; start config is a `0o600` file (not argv), unlinked after read |
| Idle | Default 30m (`--session-idle-secs` / `SKIFF_SESSION_IDLE_SECS`; `0` = never) |
| Bake | `bake create … --session NAME` so `@name` reuses the warm daemon |
| Failure | If the MCP child dies, IPC errors; `--session-stop` then `--session-start` again |

Not available on Windows.

### OAuth

HTTP sources only (not `--mcp-stdio`). Prefer streamable HTTP; SSE gets a Bearer at connect only (no mid-stream refresh).

```bash
skiff --mcp https://mcp.example.com/mcp --oauth --list
skiff --mcp https://mcp.example.com/mcp \
  --oauth-client-id env:OAUTH_CLIENT_ID \
  --oauth-client-secret env:OAUTH_CLIENT_SECRET \
  --oauth-flow client_credentials --list
skiff --mcp https://mcp.example.com/mcp --oauth-clear
```

Tokens: `$SKIFF_CACHE_DIR/oauth/` (default `~/.cache/skiff/oauth/`).

### Bake / `@name`

```bash
skiff bake create petstore --spec ./tests/fixtures/petstore.json --methods GET,POST
skiff bake create mytools \
  --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --exclude deploy --session myfs

skiff @petstore --list
skiff @mytools echo --message hi   # after --session-start myfs

skiff bake list
skiff bake show petstore           # secrets masked
skiff bake update petstore --cache-ttl 7200
skiff bake remove petstore
skiff bake install mytools --dir ./scripts/
```

Configs: `$SKIFF_CONFIG_DIR/baked.json` (default `~/.config/skiff/baked.json`).

## Security

- **Secrets:** use `env:` / `file:` prefixes on `--auth-header` and OAuth flags — never literal tokens on argv.
- **Trust:** treat remote MCP/API responses as untrusted input.
- **URLs:** remote `--spec` / `--mcp` / `--graphql` fetches are **not** SSRF-sandboxed; only pass URLs you trust.
- **Cache dir:** do not point `SKIFF_CACHE_DIR` at a shared world-writable path (sessions and OAuth store secrets there).
- **Sessions:** same-UID local IPC only; `--session-clean-env` for untrusted stdio servers.

## Agent skill

Cursor skill for agents: [`.cursor/skills/skiff/`](.cursor/skills/skiff/).

Token-efficient agent path: `--agent` (or `SKIFF_AGENT=1`) → progressive `--detail names|brief` → `--describe` / `TOOL --help --json` → `--json` or `--toon`. Oversized results spill to `$SKIFF_CACHE_DIR/spool/` with a small stdout pointer for `rg`.

Cloudflare MCP byte bench (optional):

```bash
CF_API_TOKEN=… SKIFF_BENCH_CF=1 cargo test --test cloudflare_bench -- --ignored --nocapture
```

Rust vs Python multi-run comparison (dataframe + optional CSV; needs `pandas` and `uvx`):

```bash
cargo build --release
python3 scripts/bench_vs_python.py --runs 10
# SKIFF_BIN=… SKIFF_PYTHON_BIN="uvx --with mcp==1.12.0 mcp2cli" \
#   python3 scripts/bench_vs_python.py --csv /tmp/bench.csv
```

### Benchmark results (Cloudflare MCP, 10 warm runs)

Measured 2026-07-31 against upstream Python [`mcp2cli`](https://github.com/knowsuchagency/mcp2cli) via `uvx --with mcp==1.12.0 mcp2cli` (plain `uvx mcp2cli` currently fails streamable HTTP on newer `mcp` SDKs). Isolated `SKIFF_CACHE_DIR` per implementation. Wall clock includes process spawn.

| Scenario | Rust warm median | Python warm median | Approx. speedup |
|----------|-----------------:|-------------------:|----------------:|
| Docs MCP `--list --json --compact` | 8.2 ms | 575 ms | ~70× |
| Docs MCP `--list --json` | 12.8 ms | 555 ms | ~43× |
| Fat catalog `--search workers --json --compact --top 20` | **9.9 ms** | 3224 ms | **~325×** |
| Fat catalog `--list --json --compact` | 12.8 ms | 1846 ms | ~144× |
| Fat catalog session `--agent --search workers` (Rust-only) | 12.8 ms | — | — |

Cold first fetch of the fat Cloudflare catalog is ~1–2 s for Rust and ~2–3.5 s for Python (network + full `list_tools`). After that, Rust stays near **10 ms** because warm discovery reads a slim **v4 tools index** (~143 KiB names + overrides; postings rebuilt in memory) or, with `--session`, searches an in-daemon RAM index over Unix IPC. Python’s warm path still pays a large per-invocation cost on this catalog (often seconds), so spawn + cache alone do not explain the gap.

**Index / session notes**

- Non-session disk index omits descriptions and postings by default; `--detail brief` may fall through to the full tools cache.
- Session daemons hold `CompactIndex` in RAM and answer `list_tools_light` without shipping `inputSchema`s — preferred for fat servers.
- Full `tools.json` remains ~2.4 MB for schema-heavy calls (`--detail full`, `--describe`, tool `--help`).

**Limitations of this comparison**

- **Not identical stdout:** on fat `--search … --top 20`, Python returned fewer bytes (~719 vs ~1100). Formats and hit sets can differ; do not treat byte counts as a pure efficiency win either way.
- **Heuristic tokens:** `ceil(bytes/4)`, not tiktoken.
- **Auth / transport:** both use `--transport streamable` and `Authorization:Bearer …`. Results depend on Cloudflare MCP availability and token scope.
- **Python pin:** SDK pin is required for a fair streamable run; future upstream fixes may change absolute Python numbers.
- **Machine noise:** medians over 10 warm runs on one laptop; expect variance across OS/load.
- **Feature asymmetry:** `--agent`, sessions, spool, native `--toon` are Rust-only in this harness.

**Analysis:** the headline win is **latency on warm discovery**, especially search over thousands of tools — the design goal of the compact index + session RAM path. Token/byte competition remains scenario-dependent: progressive `--detail` / `--top` / `--agent` still matter more for context size than raw CLI speed.

## Status / limits

Shipped: OpenAPI, MCP stdio/HTTP (streamable + SSE), OAuth, GraphQL, sessions (Unix), bake/`@name`, list/search/output flags, native `--toon`, `--envelope`, spool overflow, `--agent` defaults.

Still thin: no Windows sessions; no mid-daemon OAuth refresh (restart the session if the token TTL is shorter than idle).

## License

MIT — see [LICENSE](LICENSE). Python mcp2cli © Stephan Fitzpatrick / knowsuchagency.
