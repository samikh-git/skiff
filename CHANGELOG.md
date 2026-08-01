# Changelog

All notable changes to this project are documented here.

## Unreleased

### Added

- `skiff doctor` / `skiff doctor --json` — binary PATH, cache/config dirs, spool/oauth/session/bake summary, install hints
- Canonical agent playbook in README + skill; local demo script [`examples/agent_workflow.sh`](examples/agent_workflow.sh)
- Mid-daemon OAuth refresh for HTTP `--session` (token rotate → reconnect before RPC; clearer auth/child-death errors)
- MCP `--list-resources` / `--read-resource` / `--list-prompts` / `--get-prompt` work with `--mcp` / `--mcp-stdio` (session no longer required)
- [ROADMAP.md](ROADMAP.md)

### Changed

- Agent skill prefers `skiff` on PATH (`brew` / `cargo install skiff-cli`) over local `cargo build`
- README documents warm discovery and progressive discovery

## 0.1.2 — 2026-07-31

- Initial crates.io / Homebrew publish as **skiff-cli** (binary `skiff`)
- MCP stdio/HTTP/SSE, OpenAPI, GraphQL, OAuth, Unix sessions, bake/`@name`
- Progressive discovery: `--agent`, `--detail`, `--describe`, spool, v4 tools index
- Agent skill via `npx skills add samikh-git/skiff`
