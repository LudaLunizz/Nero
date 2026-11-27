#!/usr/bin/env bash
# Run the pure-logic tests against build/ (run tilingshell-patch/apply.sh first).
# They go through the JS wrappers into the Rust core (WebAssembly), so they check
# the whole path GNOME Shell uses. The Rust side alone: cargo test in core/.
set -euo pipefail
cd "$(dirname "$0")"
for t in test-freeSpace test-frames test-fillfree test-fluid test-linegroups test-adjacent test-linked-resize; do echo "== $t"; gjs -m "$t.mjs"; done
echo "== test-loader-version"; gjs -m test-loader-version.mjs
echo "== test-registry"; gjs -m test-registry.mjs
echo "== test-prompt-choices"; gjs -m test-prompt-choices.mjs
echo "== test-build-check"; ./test-build-check.sh
echo "== test-sysmon"; ./test-sysmon.sh
echo "all tests passed"
