#!/usr/bin/env bash
# The tests that need GJS, against build/ (run tilingshell-patch/apply.sh first).
# The logic itself is tested in Rust (cargo test in core/); these check what only
# GJS can: the wrappers of the Tiling Shell patch, the loader, the Registry.
set -euo pipefail
cd "$(dirname "$0")"
echo "== test-wrappers"; gjs -m test-wrappers.mjs
echo "== test-loader-version"; gjs -m test-loader-version.mjs
echo "== test-registry"; gjs -m test-registry.mjs
echo "== test-build-check"; ./test-build-check.sh
echo "all tests passed"
