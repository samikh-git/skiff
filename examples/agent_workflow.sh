#!/usr/bin/env bash
# Canonical agent workflow against the local MCP fixture (no API token).
# Usage: from repo root after `cargo build --release`
#   ./examples/agent_workflow.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SKIFF="${SKIFF:-$ROOT/target/release/skiff}"
if [[ ! -x "$SKIFF" ]]; then
  if command -v skiff >/dev/null 2>&1; then
    SKIFF="$(command -v skiff)"
  else
    echo "skiff binary not found. Run: cargo build --release" >&2
    echo "Or: brew install skiff / cargo install skiff-cli" >&2
    exit 1
  fi
fi

SERVER="${PYTHON:-python3} $ROOT/tests/fixtures/mcp_test_server.py"
NAME="demo-workflow"
export SKIFF_CACHE_DIR="${SKIFF_CACHE_DIR:-$(mktemp -d -t skiff-demo.XXXXXX)}"
export SKIFF_CONFIG_DIR="${SKIFF_CONFIG_DIR:-$SKIFF_CACHE_DIR/config}"
mkdir -p "$SKIFF_CONFIG_DIR"

cleanup() {
  "$SKIFF" --session-stop "$NAME" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> doctor"
"$SKIFF" doctor

echo "==> bake create @$NAME"
"$SKIFF" bake create "$NAME" --mcp-stdio "$SERVER" --session "$NAME" --force

echo "==> session-start"
"$SKIFF" --mcp-stdio "$SERVER" --session-start "$NAME" --session-idle-secs 600

echo "==> agent search"
"$SKIFF" @"$NAME" --agent --search echo

echo "==> describe"
"$SKIFF" @"$NAME" --describe echo --json

echo "==> call"
"$SKIFF" @"$NAME" --agent echo --message "hello from agent workflow"

echo "==> resources (no session flag required on one-shot; here via session)"
"$SKIFF" --session "$NAME" --list-resources --json

echo "==> done (session stopped on exit)"
