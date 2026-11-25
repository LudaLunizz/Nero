# Nero

A window manager for GNOME: window placement tweaks for GNOME Shell (Wayland, GNOME 46–50), made for a
multi-monitor desktop with portrait and small screens. Two parts:

1. **keep-onscreen**: a small standalone extension. New windows are shrunk and
   moved so they fit the monitor they open on (no more windows half off the edge).
2. **Tiling Shell patch**: a patch set for [Tiling Shell](https://github.com/domferr/tilingshell)
   **18.0** by Domenico Ferraro, which changes how tiling, dragging and resizing behave.

The placement logic itself is **Rust** (`core/`), built to WebAssembly and run inside
GNOME Shell by a small JS loader (`core/js/nero.js`). GNOME Shell only lets JavaScript
move windows, so the extensions stay JS for the signals and window calls, and ask the
Rust core where things go. The call is synchronous and in-process (no daemon, no D-Bus);
a Rust panic becomes a JS exception and cannot take the shell down.

## 0. Build the Rust core

Needs Rust (edition 2024) with the WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
./core/build.sh       # builds nero_core.wasm and copies it + nero.js into both extensions
cd core && cargo test # the Rust tests
```

`apply.sh` (below) runs `core/build.sh` itself.

## 1. keep-onscreen

```sh
./core/build.sh
cp -r keep-onscreen/keep-onscreen@LudaLunizz ~/.local/share/gnome-shell/extensions/
# log out and back in (Wayland), then:
gnome-extensions enable keep-onscreen@LudaLunizz
```

Only acts once per window, on its first frame. Maximized and full-screen windows are left alone.

## 2. Tiling Shell 18.0 patch

```sh
./tilingshell-patch/apply.sh            # download 18.0 (sha256-checked), patch into build/
./tests/run.sh                          # logic tests through the wasm (needs gjs)
./tilingshell-patch/apply.sh --install  # replace your installed Tiling Shell (old copy kept as .bak-<date>)
```

Log out and back in afterwards. **An update of Tiling Shell from extensions.gnome.org
overwrites the patch**; run `apply.sh --install` again (the patch is made for 18.0 only).

### What the patch changes

| Feature | How it works |
|---|---|
| Exact placement | A window dropped on a tile gets exactly that tile. Windows already there stay put underneath (a new layer); nothing gets pushed around or refilled automatically. |
| No window suggestions | Tiling Shell normally offers your other windows for the frames still empty after a drop: they fade out of their place and fly into those frames. Switched off, so windows below and on other screens stay where they are. |
| Fluid drag preview | While dragging, the preview is a rounded shape that morphs to the largest open space around the pointer (other windows are obstacles); over a window it falls back to the plain tile. |
| Fill free space | Super + right-click on a window, or "Fill free space" in the window menu: the window grows left/right, then up/down, until it meets other windows or the screen edge. (Super + right-click needs `resize-with-right-button` off, the GNOME default.) |
| Ctrl-linked resize | Resizing normally moves one window. Hold Ctrl when you grab an edge and every touching window on that monitor resizes with it, including neighbours that overlap slightly and windows whose edge lies on the same line. |
| Keep in work area | A window that was moved into a frame smaller than its minimum size is slid back fully onto its own monitor instead of sticking out. |
| Layout editor per screen | In the layout editor, Tab / Shift+Tab switch to the next/previous monitor. Esc reliably closes dialog, then editor. |
| Line link (L) | In the layout editor, L joins the line nearest the pointer across columns (one slider moves all pieces) or splits it per column again. |

New files: `utils/freeSpace.js`, `components/layout/lineGroups.js` (thin wrappers around
the Rust core; the logic is in `core/src/free_space.rs`, `line_groups.rs` and `resize.rs`),
plus `utils/nero.js` and `utils/nero_core.wasm` from `core/build.sh`. Some older code paths
(automatic refill / push-aside) are still present in the patch but switched off by an early `return`.

### Testing without logging out

You can try it in a nested shell: `dbus-run-session -- gnome-shell --nested --wayland`
(GNOME 49+: `--devkit`). Use a separate dconf profile / `XDG_CONFIG_HOME` so your real settings are not touched.

## License

GPL-3.0, same as Tiling Shell. The patch is a derivative of Tiling Shell © Domenico Ferraro.
