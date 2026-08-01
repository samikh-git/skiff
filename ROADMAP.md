# Roadmap

Near-term focus: make agent MCP workflows reliable end-to-end (install → warm session → search/describe/call → stay authenticated), and keep beating peers on **agent tokens + warm discovery latency**.

## Now (0 → 1)

- [x] Skill + README lead with brew/crates binary on PATH; `skiff doctor`
- [x] Canonical bake/session/search/describe/call playbook + `examples/agent_workflow.sh`
- [x] Mid-daemon OAuth refresh + actionable session/auth IPC errors
- [x] Resources/prompts without requiring `--session`
- [x] CHANGELOG and roadmap
- [x] Competitive gap analysis vs Python mcp2cli / mcpx / mcpli / mcpc / mcporter (2026-08-01)

## Next (evidence-ranked)

1. ~~**Skill-from-API playbook**~~ — done in [`skills/skiff/SKILL.md`](skills/skiff/SKILL.md) (2026-08-01)
2. ~~**Document exit-code contract**~~ — done in README + skill
3. ~~**Import editor MCP configs**~~ — `skiff bake import` (Cursor/Claude JSON, Codex TOML)
4. ~~**Shell completion**~~ — `skiff completion bash|zsh|fish` + `__complete` helpers
5. **Dogfood real MCPs**; fix whatever early users hit first
6. **OpenAPI realism** when a real bake target fails (path-level params, form-urlencoded, external `$ref`)
7. Broader release artifacts (musl / arm Linux) as demand appears
8. `skiff doctor` warn when PATH binary is older than the skill/docs expect (stale `~/.cargo/bin/skiff` confused resources testing)

## Later

- Windows sessions (named pipes) and Windows release binaries
- GraphQL subscriptions / deeper auto selection
- MCP tasks / resource subscribe only if dogfood shows need (do not chase Apify mcpc’s full surface)
- Response result-cache TTL (mcpx `--cache=`) — secondary to catalog index
- OS keychain for OAuth (file `env:`/`file:` is enough for now)

## Explicitly defer

- Codegen CLI minting (mcporter `generate-cli` / makabakaxy AI CLI+skill)
- Interactive REPL / Code Mode sandboxes
- Cloning mcpc’s protocol-complete inspector UX

---

## Competitive gap analysis (2026-08-01)

Positioning fence ([AGENTS.md](AGENTS.md)): runtime CLI for MCP / OpenAPI / GraphQL — compete on **context tokens** and **warm latency**, not Code Mode clones.

### Experiment log (local + live)

Against **Python mcp2cli 3.3.1** (`uvx --with mcp==1.12.0 mcp2cli`) and **skiff 0.1.2** (fresh `./target/release/skiff`):

| Scenario | Finding |
|----------|---------|
| CLI surface | Python has sessions, resources/prompts, bake, OAuth, `--toon`, `--compact`/`--top`/`--sort`. **Skiff-only:** `--agent`, `--detail`, `--describe`, `--envelope`, spool/`--max-bytes`, `doctor`, `--session-idle-secs` / `--session-clean-env`, `--oauth-clear` |
| `--json` call | Python always full MCP envelope; skiff content-only unless `--envelope` |
| Literal secrets | Python allows literals (connects, then fails messily); skiff rejects with exit 1 |
| `isError` tool | Python exit **0** with `isError: true`; skiff exit **1**. Unknown tool: both exit **2** |
| Sessions | **Both** have `--session-*`. Tiny fixture warm `--list --json --compact`: skiff **~11 ms** median vs Python **~152 ms** (~14×) |
| Spool | Skiff spills oversized agent output to pointer JSON; Python only `--head` (still dumps huge text into JSON envelope) |
| `--toon` | Both work; Python shells out to `@toon-format/cli`, skiff encodes natively |
| OpenAPI / GraphQL fixtures | Parity on list + call for petstore + local GraphQL fixture |
| Auth docs bug | Documented `Authorization:Bearer:env:VAR` was rejected until `resolve_secret` accepted `Bearer env:` / `Bearer:env:` (fixed in tree). Bench harness previously passed literal Bearer tokens and broke against strict skiff |
| Stale PATH binary | `~/.cargo/bin/skiff` (older install) lacked working non-session resources; current source works — prefer `skiff doctor` + rebuild/reinstall |

### skiff vs Python mcp2cli

| Area | Winner | Action |
|------|--------|--------|
| Warm discovery / fat catalog | **skiff** (v4 index + session RAM) | Keep; re-bench after auth harness fix |
| Progressive agent UX (`--agent`/`--detail`/`--describe`/spool) | **skiff** | Keep; teach in skill-from-API playbook |
| Content-only `--json` vs envelope | **skiff** | Keep |
| Sessions exist | Tie (both ship) | Fix README “sessions Rust-only” claim; still win on session warm latency |
| Skill-from-API generation recipe | **Python** | **Parity** — add playbook to skiff skill |
| `uvx` zero-install | **Python** | Accept for now; brew/crates + doctor |
| Literal-secret rejection | **skiff** | Keep |
| `isError` → non-zero exit | **skiff** | Document |

### Near peers (same job)

| Tool | They have / we lack | Action |
|------|---------------------|--------|
| [mcpx](https://github.com/lydakis/mcpx) | Editor MCP config import; shell completion; exit-code marketing; response `--cache=`; server-specific skills | **Parity** import + completion + document exits; skip Codex Apps |
| [mcpli](https://github.com/juanibiapina/mcpli) | Nested `server tool` UX; tab completion; auto OAuth on 401 during add | Completion yes; nested UX optional; MCP-only so not a full substitute |
| [makabakaxy/mcp2cli](https://github.com/makabakaxy/mcp2cli) | AI-generated CLI + skill sync | **Skip** (codegen / LLM-in-loop) |

### Adjacent (learn, don’t clone)

| Tool | Useful ideas | Skip |
|------|--------------|------|
| [Apify mcpc](https://github.com/apify/mcpc) | Tasks, resource subscribe, multi-session grep, OS keychain | REPL, x402, full protocol surface |
| [mcporter](https://github.com/steipete/mcporter) | Editor import; keep-alive daemon; health `--exit-code` | `generate-cli` / `emit-ts` codegen |

### First implementation PR (recommended)

Landed:

1. Auth/`Bearer:env:` secret resolution + bench harness `env:` auth
2. Expanded **skill-from-API** playbook + exit codes in skill/README
3. Honest competitive notes (Python has sessions; skiff still faster)
4. **`skiff bake import`** from Cursor / Claude / Codex MCP configs
5. **`skiff completion`** (bash/zsh/fish) with dynamic bake + session names

**Next PR:** dogfood real MCPs / OpenAPI realism, or `doctor` stale-binary warning.
