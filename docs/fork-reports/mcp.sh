#!/bin/bash
# Calls one tool on roost's read-only MCP server, without adding the server to a client.
#
#   export FLOCK_MCP_URL=https://flock-mcp.<tailnet>.ts.net/mcp
#   ./mcp.sh docs '{"name":"glossary"}'
#   ./mcp.sh group '{"id":"34bd798b1544"}'
#
# Prints the tool's text result: JSON for every tool but docs. Needs curl and jq.
set -euo pipefail
url=${FLOCK_MCP_URL:?set FLOCK_MCP_URL to the MCP server address}
tool=${1:?usage: mcp.sh <tool> '<json arguments>'}
args=${2:-"{}"}

post() {
  curl -sS -X POST "$url" -H 'Content-Type: application/json' \
    -H 'Accept: application/json, text/event-stream' "$@"
}

sid=$(post -D - -o /dev/null -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"forkreport","version":"1"}}}' |
  awk 'tolower($1) == "mcp-session-id:" { print $2 }' | tr -d '\r')
post -o /dev/null -H "Mcp-Session-Id: $sid" -d '{"jsonrpc":"2.0","method":"notifications/initialized"}'

call=$(jq -nc --arg name "$tool" --argjson a "$args" \
  '{jsonrpc: "2.0", id: 2, method: "tools/call", params: {name: $name, arguments: $a}}')
post -H "Mcp-Session-Id: $sid" -d "$call" |
  sed -n 's/^data: //; /^{/p' |
  jq -r '.result.content[]?.text // .error // .'
