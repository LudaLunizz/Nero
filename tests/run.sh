#!/usr/bin/env bash
# Run the pure-logic tests against build/ (run tilingshell-patch/apply.sh first).
set -euo pipefail
cd "$(dirname "$0")"
for t in test-freeSpace test-frames test-fillfree test-fluid; do echo "== $t (gjs)"; gjs -m "$t.mjs"; done
for t in test-linegroups test-adjacent; do echo "== $t (node)"; node "$t.mjs"; done
echo "all tests passed"
