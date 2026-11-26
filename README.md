# Nero

A window manager for GNOME: window placement tweaks for GNOME Shell (Wayland, GNOME 46–50), made for a
multi-monitor desktop with portrait and small screens. Three parts:

1. **keep-onscreen**: a small standalone extension. New windows are shrunk and
   moved so they fit the monitor they open on (no more windows half off the edge).
2. **Tiling Shell patch**: a patch set for [Tiling Shell](https://github.com/domferr/tilingshell)
   **18.0** by Domenico Ferraro, which changes how tiling, dragging and resizing behave.
3. **nero-layout**: a standalone extension that remembers where every app window is and
   puts the windows back after the next login (after a shutdown, a crash or a log out).

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

The copy step above also needs the settings schema compiled once:
`glib-compile-schemas ~/.local/share/gnome-shell/extensions/keep-onscreen@LudaLunizz/schemas`

**Password prompts on one monitor (optional).** GNOME Shell opens its password prompts
(polkit, keyring unlock, Wi-Fi/VPN secrets, encrypted disk unlock) on the monitor under the
pointer. To always get them on one monitor, give its connector name (for example `HDMI-2`;
`ls /sys/class/drm` lists them as `card0-HDMI-A-2`, GNOME names that `HDMI-2`):

```sh
gsettings --schemadir ~/.local/share/gnome-shell/extensions/keep-onscreen@LudaLunizz/schemas \
  set org.gnome.shell.extensions.keep-onscreen password-prompt-monitor HDMI-2
```

Takes effect for the next prompt; set it to `''` to switch it off. If that monitor is not
connected, prompts open as usual. This only covers GNOME Shell's own dialogs: another polkit
agent (for example `hyprpolkitagent`, installed with Hyprland and started in every Wayland
session) can take the polkit slot at login, and then the prompt is a normal window. Keep it
out of GNOME with a systemd drop-in, `~/.config/systemd/user/hyprpolkitagent.service.d/not-in-gnome.conf`:

```ini
[Service]
ExecCondition=/bin/sh -c 'case ":$XDG_CURRENT_DESKTOP:" in *:GNOME:*) exit 1;; esac'
```

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

## 3. nero-layout

```sh
./core/build.sh
cp -r nero-layout/nero-layout@LudaLunizz ~/.local/share/gnome-shell/extensions/
glib-compile-schemas ~/.local/share/gnome-shell/extensions/nero-layout@LudaLunizz/schemas
# log out and back in (Wayland), then:
gnome-extensions enable nero-layout@LudaLunizz
```

**Saving.** Every minute (when something changed) it writes `~/.local/state/nero/layout.json`:
per window the app (desktop file id), the title, the monitor (connector name, for example `DP-1`),
position and size as a share of that monitor's work area (so a panel or resolution change does not
matter), the workspace, and whether it is maximized, full screen, minimized or always on top. A
layout with *fewer* windows is only written when it is still the same a minute later, so the apps
closing during a log out or shutdown do not overwrite the saved layout. Dialogs, helper windows and
apps without a desktop file are not saved.

**Restoring.** At the next login, each new window of a saved app takes the saved window of that app
with the closest title and goes to its monitor, spot, workspace and state. After 15 seconds, saved apps
that still have no window and are not running are started once (apps in your autostart are left to
the session, so nothing runs twice). For two minutes new windows are matched; saving waits until then.
The layout of the session before is kept as `layout.prev.json`. A monitor that is not connected
any more: its windows stay where they open. What is *inside* a window (tabs, open files) is up to the
app; browsers and most editors reopen that themselves.

Settings (`gsettings --schemadir ~/.local/share/gnome-shell/extensions/nero-layout@LudaLunizz/schemas
set org.gnome.shell.extensions.nero-layout KEY VALUE`):

| Key | Default | |
|---|---|---|
| `skip-apps` | `[]` | Desktop file ids to leave alone, e.g. `"['org.gnome.Ptyxis.desktop']"` when another tool places them |
| `launch-apps` | `true` | Start saved apps that are missing after a login |
| `launch-delay` | `15` | Seconds before starting them |
| `restore-seconds` | `120` | How long after a login new windows are matched |
| `save-interval` | `60` | Seconds between saves |
| `save-now` | `false` | Set to `true`: save at once |
| `restore-now` | `false` | Set to `true`: put the open windows back on their saved spots and start missing apps, without logging out |

## License

GPL-3.0, same as Tiling Shell. The patch is a derivative of Tiling Shell © Domenico Ferraro.
