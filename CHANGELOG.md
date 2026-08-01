# Changelog

All notable changes to this project are documented here.

## 0.1.3 — 2026-08-01

### Added

- `skiff bake import` — import MCP servers from Cursor / Claude / Codex into bake
- `skiff completion bash|zsh|fish` — shell completion (`__complete` helpers for bake/session names)
- `skiff doctor` stale PATH detection — version/mtime/feature probes (`completion`, `bake_import`, …); `ok: false` when PATH lags

### Changed

- Competitive gap analysis documented in ROADMAP; skill-from-API playbook; exit codes; `Bearer:env:` auth resolution
- README / skill: install, completion, bake import, doctor refresh guidance
- Tool lookup accepts MCP `toolName` and snake_case aliases (not only kebab CLI names)
- `TOOL --help --json` / `--toon` honor trailing format flags (not only globals before the tool)
- Name-list prefix compression dedupes duplicate kebab ids (fat catalogs)

## 0.1.2 — 2026-07-31

- Initial crates.io / Homebrew publish as **skiff-cli** (binary `skiff`)
- MCP stdio/HTTP/SSE, OpenAPI, GraphQL, OAuth, Unix sessions, bake/`@name`
- Progressive discovery: `--agent`, `--detail`, `--describe`, spool, v4 tools index
- Agent skill via `npx skills add samikh-git/skiff`
