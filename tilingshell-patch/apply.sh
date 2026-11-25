#!/usr/bin/env bash
# Build a patched Tiling Shell 18.0 in ./build, and optionally install it.
#   ./tilingshell-patch/apply.sh            build only (build/tilingshell@ferrarodomenico.com)
#   ./tilingshell-patch/apply.sh --install  build, then copy over the installed extension
#                                           (the old copy is kept as <dir>.bak-<date>)
set -euo pipefail
cd "$(dirname "$0")/.."
URL=https://github.com/domferr/tilingshell/releases/download/18.0/tilingshell@ferrarodomenico.com.zip
SHA=eb0540c0ca2f4b2d724b60db8a9d35a7712610c8fee6f2d38fe5b30810f93493
UUID=tilingshell@ferrarodomenico.com
mkdir -p build
[ -f build/upstream-18.0.zip ] || curl -fL -o build/upstream-18.0.zip "$URL"
echo "$SHA  build/upstream-18.0.zip" | sha256sum -c -
rm -rf "build/$UUID"; mkdir "build/$UUID"
unzip -q build/upstream-18.0.zip -d "build/$UUID"
patch -d "build/$UUID" -p1 --no-backup-if-mismatch < tilingshell-patch/tilingshell-18.0.patch
glib-compile-schemas "build/$UUID/schemas"
core/build.sh   # the Rust core (WebAssembly) + its loader into build/$UUID/utils
echo "built build/$UUID"
if [ "${1:-}" = --install ]; then
  DEST="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$UUID"
  [ -d "$DEST" ] && mv "$DEST" "$DEST.bak-$(date +%Y%m%d-%H%M%S)"
  cp -r "build/$UUID" "$DEST"
  echo "installed to $DEST: log out and back in (Wayland) to load it"
fi
