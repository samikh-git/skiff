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

# MCP HTTP (streamable or legacy SSE; --transport auto|sse|streamable)
./target/release/mcp2cli --mcp http://127.0.0.1:8000/mcp --list
./target/release/mcp2cli --mcp http://127.0.0.1:8000/sse --transport sse echo --message hi

# GraphQL
./target/release/mcp2cli --graphql http://127.0.0.1:4000 --list
./target/release/mcp2cli --graphql http://127.0.0.1:4000 user --id 1
./target/release/mcp2cli --graphql http://127.0.0.1:4000 --fields "id name" user --id 1

# OAuth (MCP HTTP / OpenAPI URL fetch / GraphQL; not with --mcp-stdio)
./target/release/mcp2cli --mcp https://mcp.example.com/mcp --oauth --list
./target/release/mcp2cli --mcp https://mcp.example.com/mcp \
  --oauth-client-id env:OAUTH_CLIENT_ID \
  --oauth-client-secret env:OAUTH_CLIENT_SECRET \
  --oauth-flow client_credentials --list
./target/release/mcp2cli --mcp https://mcp.example.com/mcp --oauth-clear
```

Tokens cache under `~/.cache/mcp2cli/oauth/` (override with `MCP2CLI_CACHE_DIR`). Prefer streamable HTTP for OAuth; legacy SSE injects a Bearer at connect time only (no mid-stream refresh). GraphQL introspection caches under `~/.cache/mcp2cli/` like OpenAPI.

**M1 status:** OpenAPI + MCP stdio/HTTP (streamable + legacy SSE) + OAuth + GraphQL + list/search/output flags + bake/`@name`. Still deferred: sessions.

### Bake mode

```bash
# Save connection settings
./target/release/mcp2cli bake create petstore \
  --spec ./tests/fixtures/petstore.json --methods GET,POST

./target/release/mcp2cli bake create mytools \
  --mcp-stdio "python3 ./tests/fixtures/mcp_test_server.py" --exclude deploy

# Use without repeating connection flags
./target/release/mcp2cli @petstore --list
./target/release/mcp2cli @mytools echo --message hi

# Manage
./target/release/mcp2cli bake list
./target/release/mcp2cli bake show petstore
./target/release/mcp2cli bake update petstore --cache-ttl 7200
./target/release/mcp2cli bake remove petstore
./target/release/mcp2cli bake install mytools --dir ./scripts/
```

Configs are stored in `~/.config/mcp2cli/baked.json` (override with `MCP2CLI_CONFIG_DIR`).

## License

MIT — see [LICENSE](LICENSE). Original Python project © Stephan Fitzpatrick / knowsuchagency.
