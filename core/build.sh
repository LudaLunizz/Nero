#!/usr/bin/env bash
# Build the Rust core as WebAssembly and put it (with its JS loader) where the
# extensions load it from:
#   keep-onscreen/nero-keep-onscreen@LudaLunizz/
#   nero-layout/nero-layout@LudaLunizz/
#   nero-workspaces/nero-workspaces@LudaLunizz/
#   build/tilingshell@ferrarodomenico.com/utils/   (when apply.sh has made it)
# The loader (nero.js) is stamped with the core's version; it refuses to run next
# to a wasm of another version. The three extension directories also get
# helpers.js (shared GNOME Shell helpers).
#
#   core/build.sh                 build and install the copies
#   core/build.sh --check [DIR..] build, install nothing, and fail if a copy
#                                 (or an installed extension directory given as
#                                 DIR) differs from the fresh build
#
# Needs Rust with the wasm32-unknown-unknown target:
#   rustup target add wasm32-unknown-unknown
set -euo pipefail
cd "$(dirname "$0")"
MODE=install
if [ "${1:-}" = --check ]; then MODE=check; shift; fi
[ $# -eq 0 ] || [ "$MODE" = check ] || { echo "usage: core/build.sh [--check [DIR...]]" >&2; exit 2; }

cargo build --release --locked --target wasm32-unknown-unknown -j "${NERO_JOBS:-10}"
META="$(cargo metadata --format-version 1 --no-deps)"
WASM="$(python3 -c 'import sys, json; print(json.load(sys.stdin)["target_directory"])' <<<"$META")/wasm32-unknown-unknown/release/nero_core.wasm"
VERSION="$(python3 -c 'import sys, json; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "nero-core"))' <<<"$META")"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
sed "s/@NERO_CORE_VERSION@/$VERSION/" js/nero.js > "$STAGE/nero.js"
grep -q "\"$VERSION\"" "$STAGE/nero.js" || { echo "could not stamp version $VERSION into nero.js" >&2; exit 1; }
cp "$WASM" "$STAGE/nero_core.wasm"
cp js/helpers.js "$STAGE/helpers.js"

EXT_DIRS=(../keep-onscreen/nero-keep-onscreen@LudaLunizz ../nero-layout/nero-layout@LudaLunizz ../nero-workspaces/nero-workspaces@LudaLunizz)
TS_DIR=../build/tilingshell@ferrarodomenico.com/utils

if [ "$MODE" = install ]; then
  for dest in "${EXT_DIRS[@]}" "$TS_DIR"; do
    [ -d "$dest" ] || continue
    files=(nero_core.wasm nero.js)
    case "$dest" in "$TS_DIR") ;; *) files+=(helpers.js) ;; esac
    for f in "${files[@]}"; do install -m 644 "$STAGE/$f" "$dest/"; done
    echo "core $VERSION -> $dest"
  done
  exit 0
fi

# --check: compare every copy with the fresh build. The repo's own directories
# are skipped when absent (build/ exists only after apply.sh); a directory named
# on the command line must exist. A directory without helpers.js (the patched
# Tiling Shell) is only compared on the two core files.
bad=0
check_dir() {
  local dest=$1 files=(nero_core.wasm nero.js helpers.js) f
  case "$dest" in "$TS_DIR"|*/tilingshell@*) files=(nero_core.wasm nero.js) ;; esac
  for f in "${files[@]}"; do
    if [ ! -f "$dest/$f" ]; then echo "MISSING  $dest/$f"; bad=1
    elif ! cmp -s "$STAGE/$f" "$dest/$f"; then echo "DIFFERS  $dest/$f (not from this build, version $VERSION)"; bad=1
    fi
  done
}
if [ $# -gt 0 ]; then
  for dest in "$@"; do
    [ -d "$dest" ] || { echo "MISSING  $dest (not a directory)"; bad=1; continue; }
    check_dir "$dest"
  done
else
  for dest in "${EXT_DIRS[@]}" "$TS_DIR"; do
    [ -d "$dest" ] && check_dir "$dest"
  done
fi
if [ "$bad" = 0 ]; then echo "all copies match the build of core $VERSION"; else exit 1; fi
