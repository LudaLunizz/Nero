#!/usr/bin/env bash
# core/build.sh --check passes on a directory built by core/build.sh and fails on
# one that differs or is missing a file. Needs the extension directories built.
set -euo pipefail
cd "$(dirname "$0")/.."
SRC=nero-workspaces/nero-workspaces@LudaLunizz
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
cp "$SRC/nero_core.wasm" "$SRC/nero.js" "$SRC/helpers.js" "$T/"
core/build.sh --check "$T" >/dev/null || { echo "FAIL: a fresh copy was rejected"; exit 1; }
echo "// edited" >> "$T/nero.js"
if core/build.sh --check "$T" >/dev/null; then echo "FAIL: an edited nero.js passed"; exit 1; fi
cp "$SRC/nero.js" "$T/nero.js"; rm "$T/helpers.js"
if core/build.sh --check "$T" >/dev/null; then echo "FAIL: a missing helpers.js passed"; exit 1; fi
if core/build.sh --check "$T/nope" >/dev/null; then echo "FAIL: a missing directory passed"; exit 1; fi
echo "build --check ok"
