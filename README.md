# mcp2cli (Rust)

Rust port of [knowsuchagency/mcp2cli](https://github.com/knowsuchagency/mcp2cli) — turn any MCP server or OpenAPI spec into a CLI at runtime, with zero codegen.

```bash
cargo build --release
cargo test

# OpenAPI
./target/release/mcp2cli --spec ./tests/fixtures/petstore.json --base-url http://localhost:8080/api/v1 --list
./target/release/mcp2cli --spec ./openapi.json --base-url https://api.example.com list-pets --limit 5

# MCP stdio
./target/release/mcp2cli --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --list
./target/release/mcp2cli --mcp-stdio "npx @modelcontextprotocol/server-filesystem /tmp" read-file --path /tmp/hello.txt
```

**M1 status:** OpenAPI + MCP stdio + list/search/output flags. Still deferred: MCP HTTP, OAuth, GraphQL, bake/`@name`, sessions.

## License

MIT — see [LICENSE](LICENSE). Original Python project © Stephan Fitzpatrick / knowsuchagency.
