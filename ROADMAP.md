# Roadmap

Near-term focus: make agent MCP workflows reliable end-to-end (install → warm session → search/describe/call → stay authenticated).

## Now (0 → 1)

- [x] Skill + README lead with brew/crates binary on PATH; `skiff doctor`
- [x] Canonical bake/session/search/describe/call playbook + `examples/agent_workflow.sh`
- [x] Mid-daemon OAuth refresh + actionable session/auth IPC errors
- [x] Resources/prompts without requiring `--session`
- [x] CHANGELOG and roadmap

## Next

- Dogfood more real MCPs; fix whatever early users hit first
- OpenAPI realism when a real bake target fails (path-level params, form-urlencoded, external `$ref`)
- Broader release artifacts (musl / arm Linux) as demand appears

## Later

- Windows sessions (named pipes) and Windows release binaries
- GraphQL subscriptions / deeper auto selection
