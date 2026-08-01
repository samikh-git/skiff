# Roadmap

Near-term focus: make agent MCP workflows reliable end-to-end (install → warm session → search/describe/call → stay authenticated), and keep beating peers on **agent tokens + warm discovery latency**.

## Now (0 → 1)

- [x] Skill + README lead with brew/crates binary on PATH; `skiff doctor`
- [x] Canonical bake/session/search/describe/call playbook + `examples/agent_workflow.sh`
- [x] Mid-daemon OAuth refresh + actionable session/auth IPC errors
- [x] Resources/prompts without requiring `--session`
- [x] CHANGELOG and roadmap
- [x] Competitive gap analysis vs Python mcp2cli / mcpx / mcpli / mcpc / mcporter (2026-08-01)
- [x] `bake import`, shell completion, doctor stale-PATH detection (0.1.3)

## Next (evidence-ranked)

1. **Dogfood real MCPs**; fix whatever early users hit first
2. **OpenAPI realism** when a real bake target fails (path-level params, form-urlencoded, external `$ref`)
3. Broader release artifacts (musl / arm Linux) as demand appears
4. Publish **0.1.3** to crates.io / Homebrew when ready

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

Positioning fence ([AGENTS.md](AGENTS.md)): runtime CLI for MCP / OpenAPI / GraphQL — compete on **context tokens** and **warm discovery latency**, not Code Mode clones.

### Experiment log (local + live)

Against **Python mcp2cli 3.3.1** (`uvx --with mcp==1.12.0 mcp2cli`) and **skiff** (fresh build):

| Scenario | Finding |
|----------|---------|
| CLI surface | Python has sessions, resources/prompts, bake, OAuth, `--toon`, `--compact`/`--top`/`--sort`. **Skiff-only:** `--agent`, `--detail`, `--describe`, `--envelope`, spool, `doctor`, sessions extras, `bake import`, `completion` |
| `--json` call | Python always full MCP envelope; skiff content-only unless `--envelope` |
| Literal secrets | Python allows literals; skiff rejects with exit 1 |
| `isError` tool | Python exit **0**; skiff exit **1**. Unknown tool: both exit **2** |
| Sessions | Both ship; skiff warm list ~11 ms vs Python ~152 ms on tiny fixture |
| Spool | Skiff spills oversized agent output; Python only `--head` |
| Stale PATH binary | Older `~/.cargo/bin/skiff` lacked newer features — **`skiff doctor` flags this** (0.1.3) |

### Landed since analysis

Auth `Bearer:env:`, skill-from-API playbook, exit-code docs, `bake import`, shell completion, doctor stale detection.

**Next:** dogfood / OpenAPI realism; crates.io + Homebrew 0.1.3.
