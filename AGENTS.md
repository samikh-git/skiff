# AGENTS.md — working on skiff

Guidance for coding agents and humans extending this repo. Prefer this file plus [README.md](README.md) and [`.cursor/skills/skiff/`](.cursor/skills/skiff/) over inventing new CLI surfaces.

## Product goals

- **Runtime CLI** for MCP / OpenAPI / GraphQL — no codegen.
- Compete on **agent context tokens** and **warm discovery latency**, not by cloning Cloudflare Code Mode sandboxes.
- Progressive discovery: `--detail names|brief|full`, `--describe`, `--search`, `--agent`, spool for huge payloads.
- Brand: **skiff** (not mcp2cli). Credit Python mcp2cli as prior art only.

## Layout (high-signal)

| Path | Role |
|------|------|
| `src/cli/` | Args, dispatch, list/search output |
| `src/tools_index.rs` | Compact catalog index (v4 disk; postings in RAM) |
| `src/session/` | Unix session daemon + NDJSON IPC |
| `src/mcp/` | HTTP / SSE / stdio transports |
| `scripts/bench_vs_python.py` | Multi-run Rust vs upstream Python dataframe bench |
| `tests/cloudflare_bench.rs` | Env-gated CF smoke bench |
| `tests/token_efficiency.rs` | Local fixture token/size checks |

## Index + sessions (do not regress)

1. **Disk index (non-session):** `*_tools_index.json` is **names + sparse `tool_overrides` only**. Rebuild `postings` on load. Do not reintroduce full parallel `tool_names` or default truncated `descs` without a strong reason.
2. **`--detail brief`:** if the index has no descs, **fall through** to tools cache / live fetch — do not fake empty descriptions as warm-brief success.
3. **Session daemon:** keep `CompactIndex` beside `tools_cache`. Discovery with `--detail names` (including `--agent --search`) must use **`list_tools_light`** (search in-process). Full `list_tools` only when schemas are needed.
4. **No embedded KV** (redb/sled) unless catalogs grow far beyond ~3k tools or we need a shared index without a daemon. In-memory postings map is the intended KV shape.
5. Index lifetime should stay **temporary with the session** for fat catalogs; disk sidecar is a thin one-shot accelerator.

## Agent / CLI conventions

- `--agent` / `SKIFF_AGENT=1`: JSON; search ⇒ names + `--top 20`; else brief; spool default 64KiB.
- `--json` = MCP **content-only**; `--envelope` / `--full` for wire `CallToolResult`.
- Auth header form: `Authorization:Bearer <token>` (space after `Bearer`). Prefer `env:` / `file:` secret prefixes in docs/examples.
- Cache/config: `SKIFF_CACHE_DIR` / `SKIFF_CONFIG_DIR` → `~/.cache/skiff`, `~/.config/skiff`.
- Do not commit `.env`, tokens, or cache dumps.

## How to verify

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
# sessions (Unix):
cargo test --test session_integration
# optional live CF:
CF_API_TOKEN=… SKIFF_BENCH_CF=1 cargo test --test cloudflare_bench -- --ignored --nocapture
# Rust vs Python (needs pandas + uv):
cargo build --release
python3 scripts/bench_vs_python.py --runs 10
```

When changing discovery/index/session paths, re-run at least `tools_index` unit tests + `session_integration`, and prefer a short `bench_vs_python` warm fat-search check if network/token available.

## Docs to keep in sync

- User-facing: `README.md` (including bench table / limitations when numbers change materially).
- Agent skill: `.cursor/skills/skiff/` (index description should match v4 + session RAM behavior).
- This file: architecture decisions agents must not casually reverse.

## Git / PR hygiene

- Do **not** add `Co-authored-by: Cursor` (or similar bot trailers) to commits.
- Do not force-push `main`/`master`. Feature branches may be rewritten only when the user asks.
- Prefer small, focused commits; do not commit secrets or unrelated drive-bys unless requested.
