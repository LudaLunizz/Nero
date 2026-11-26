#!/usr/bin/env bash
# Build the Rust core as WebAssembly and put it (with its JS loader) where the
# extensions load it from:
#   keep-onscreen/nero-keep-onscreen@LudaLunizz/
#   nero-layout/nero-layout@LudaLunizz/
#   nero-workspaces/nero-workspaces@LudaLunizz/
#   build/tilingshell@ferrarodomenico.com/utils/   (when apply.sh has made it)
# The three extension directories also get helpers.js (shared GNOME Shell helpers).
# Needs Rust with the wasm32-unknown-unknown target:
#   rustup target add wasm32-unknown-unknown
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --locked --target wasm32-unknown-unknown -j "${NERO_JOBS:-10}"
WASM="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])')/wasm32-unknown-unknown/release/nero_core.wasm"
for dest in ../keep-onscreen/nero-keep-onscreen@LudaLunizz ../nero-layout/nero-layout@LudaLunizz ../nero-workspaces/nero-workspaces@LudaLunizz ../build/tilingshell@ferrarodomenico.com/utils; do
  [ -d "$dest" ] || continue
  install -m 644 "$WASM" js/nero.js "$dest/"
  # helpers.js (shared GNOME Shell helpers) goes to the extensions, not into Tiling Shell's utils
  case "$dest" in *tilingshell@*) ;; *) install -m 644 js/helpers.js "$dest/" ;; esac
  echo "core -> $dest"
done
