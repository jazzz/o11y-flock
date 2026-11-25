#!/bin/bash
# Runs one read-only SELECT through roost's MCP server and prints the rows as a JSON array.
#
#   export FLOCK_MCP_URL=https://flock-mcp.<tailnet>.ts.net/mcp
#   ./sql.sh "SELECT id FROM groups WHERE id LIKE '34bd798b%'"
#
# Results stop at 500 rows: it warns on stderr when a query hits that, so page by time.
set -euo pipefail
here=$(dirname "$0")
out=$("$here/mcp.sh" query "$(jq -nc --arg s "${1:?usage: sql.sh \"<SELECT ...>\"}" '{sql: $s}')")
if jq -e '.truncated == true' >/dev/null 2>&1 <<<"$out"; then
  echo "sql.sh: 500 rows, the cap: page by time" >&2
fi
jq '.rows // .' <<<"$out"
