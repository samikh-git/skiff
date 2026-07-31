# mcp2cli (Rust)

Rust port of [knowsuchagency/mcp2cli](https://github.com/knowsuchagency/mcp2cli) — turn any MCP server or OpenAPI spec into a CLI at runtime, with zero codegen.

**Status:** Milestone 1 in progress (OpenAPI + MCP + bake). Tests from upstream are the contract.

```bash
cargo build --release
./target/release/mcp2cli --help
cargo test
```

## License

MIT — see [LICENSE](LICENSE). Original Python project © Stephan Fitzpatrick / knowsuchagency.
