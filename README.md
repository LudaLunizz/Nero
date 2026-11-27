# Nero

A window manager for GNOME: window placement tweaks for GNOME Shell (Wayland, GNOME 46–50), made for a
multi-monitor desktop with portrait and small screens. Five parts:

1. **keep-onscreen**: a small standalone extension. New windows are shrunk and
   moved so they fit the monitor they open on (no more windows half off the edge).
2. **Tiling Shell patch**: a patch set for [Tiling Shell](https://github.com/domferr/tilingshell)
   **18.0** by Domenico Ferraro, which changes how tiling, dragging and resizing behave.
3. **nero-layout**: a standalone extension that remembers where every app window is and
   puts the windows back after the next login (after a shutdown, a crash or a log out).
4. **nero-workspaces**: a standalone extension that gives every monitor its own workspaces:
   scroll on a monitor's panel (or press the workspace keys) and only that monitor switches.
5. **taskbar**: a Windows-like taskbar setup: Dash to Panel at the bottom of every monitor,
   a system monitor in it (Argos script), and the Argos buttons moved to another monitor if you want.

The placement logic itself is **Rust** (`core/`), built to WebAssembly and run inside
GNOME Shell by a small JS loader (`core/js/nero.js`). GNOME Shell only lets JavaScript
move windows, so the extensions stay JS for the signals and window calls, and ask the
Rust core where things go. The call is synchronous and in-process (no daemon, no D-Bus);
a Rust panic becomes a JS exception and cannot take the shell down.

## 0. Build the Rust core

Needs Rust (edition 2024) with the WebAssembly target:

```sh
rustup target add wasm32-unknown-unknown
./core/build.sh       # builds nero_core.wasm and copies it, nero.js and helpers.js into the extensions
./core/build.sh --check [DIR...]   # fails if a copy (or an installed extension directory) is not from this build
cd core && cargo test # the Rust tests
```

`apply.sh` (below) runs `core/build.sh` itself.

## 1. keep-onscreen

```sh
./core/build.sh
cp -r keep-onscreen/nero-keep-onscreen@LudaLunizz ~/.local/share/gnome-shell/extensions/
# log out and back in (Wayland), then:
gnome-extensions enable nero-keep-onscreen@LudaLunizz
```

The extension is called `nero-keep-onscreen@LudaLunizz` (it was `keep-onscreen@LudaLunizz` until
26 Nov 2026: disable and remove the old directory when you upgrade; the settings carry over, the
schema is the same). Run only one extension that handles GNOME's password prompts: a second copy
under another name replaces the same `ModalDialog.open` function, and the two fight over it.

Only acts once per window, on its first frame. Maximized and full-screen windows are left alone.

The copy step above also needs the settings schema compiled once:
`glib-compile-schemas ~/.local/share/gnome-shell/extensions/nero-keep-onscreen@LudaLunizz/schemas`

**Password prompts on one monitor (optional).** GNOME Shell opens its password prompts
(polkit, keyring unlock, Wi-Fi/VPN secrets, encrypted disk unlock) on the monitor under the
pointer. To always get them on one monitor, open the extension's settings (Extensions app →
Keep On Screen → Settings, or `gnome-extensions prefs nero-keep-onscreen@LudaLunizz`) and pick it
under **Password prompts → Open on**. The list shows every connected monitor with its name, size
and place (leftmost, 2nd from the left, ...). **Show a prompt** opens a real administrator prompt
(`pkexec true`), so you can see where the next one will open. Cancel it, or type your password:
it only runs `true`. The same page has the dark-overlay switch below.

Without the settings window, give the connector name directly (for example `HDMI-2`;
`ls /sys/class/drm` lists them as `card0-HDMI-A-2`, GNOME names that `HDMI-2`):

```sh
gsettings --schemadir ~/.local/share/gnome-shell/extensions/nero-keep-onscreen@LudaLunizz/schemas \
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

**No dark overlay behind password prompts (optional).** GNOME Shell darkens every monitor
while one of these prompts is open. To keep the other monitors as they are:

```sh
gsettings --schemadir ~/.local/share/gnome-shell/extensions/nero-keep-onscreen@LudaLunizz/schemas \
  set org.gnome.shell.extensions.keep-onscreen password-prompt-dim false
```

The prompt still takes the keyboard until you answer or cancel it, so other apps cannot
read what you type.

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
with the closest title and goes to its monitor, spot, workspace and state. Windows open in whatever
order their apps start, so after each one is placed the saved windows on that monitor are raised back
into their saved stacking order (the file lists them bottom first). After 15 seconds, saved apps
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

## 4. nero-workspaces

```sh
./core/build.sh
cp -r nero-workspaces/nero-workspaces@LudaLunizz ~/.local/share/gnome-shell/extensions/
glib-compile-schemas ~/.local/share/gnome-shell/extensions/nero-workspaces@LudaLunizz/schemas
# log out and back in (Wayland), then:
gnome-extensions enable nero-workspaces@LudaLunizz
```

GNOME switches workspaces on all monitors at once. With nero-workspaces every monitor has its own
row of workspaces (4 by default) and switches on its own:

- **Scroll wheel / touchpad on a Dash to Panel panel** switches the monitor that panel is on. One
  wheel notch = one workspace, also on high-resolution wheels and touchpads (which send many small
  scroll events); after a step, scroll is ignored for 250 ms, so a fast spin or a flick moves one
  workspace and not all of them. Scrolling over an app icon still does what Dash to Panel's icon
  scroll action says. This takes over Dash to Panel's own "Switch workspace" panel scroll action
  (set to "Nothing" while nero-workspaces runs, put back when it is switched off); any other panel
  scroll action you picked in Dash to Panel is left alone.
- **Three-finger swipe** left or right on a touchpad switches the monitor under the pointer
  (GNOME's own swipe, which slides all monitors, is switched off while this is on).
- **Workspace keys** (Super+Page Up/Down, Ctrl+Alt+arrows, Super+Shift+Page Up/Down to take the
  window along, and the "switch to / move to workspace N" keys) act on the monitor of the
  focused window.
- **Activating a window on a hidden workspace** (taskbar click, Alt+Tab, a notification) switches
  only that window's monitor to the window's workspace.
- A window dragged to another monitor joins the workspace that monitor shows. A "2 / 4" OSD shows
  on the monitor that switched. What each monitor shows is kept in `~/.local/state/nero/workspaces.json`.

**How.** GNOME stays on its first workspace, which is the one on screen; a window on a workspace
that its monitor does not show is parked on GNOME's workspace *n + 1*. So while nero-workspaces runs,
GNOME has fixed workspaces (workspaces per monitor + 1, *dynamic workspaces* off) on all monitors
(*workspaces on primary display only* off). Your values for those three settings are kept and put
back when it is switched off, and every window comes back onto the screen then. The Activities
overview still shows GNOME's real workspaces (the first one = what is on screen now).

**Touchpad scroll speed.** GNOME has no setting for the two-finger scroll speed. nero-workspaces
writes a small libinput plugin (`core/src/scroll_speed.lua`) to `~/.config/libinput/plugins/nero-scroll-speed.lua`;
Mutter (GNOME 49+, libinput 1.30+) loads plugins from there at login. While exactly two fingers
touch, it moves each finger's position *speed %* as far as the finger really moved, so scrolling
(and pinch-to-zoom) slows down; one finger and three-or-more-finger gestures stay 1:1. At 100 % the
file is removed. A new speed works from the next login. Delete the file to undo it by hand.

**Settings window**: Extensions app → Nero Workspaces → Settings, or
`gnome-extensions prefs nero-workspaces@LudaLunizz`. A *Touchpad* page (pointer speed = GNOME's
own setting, two-finger scroll speed 0–100 %, the three-finger swipe) and a *Workspaces* page.

Settings (`gsettings --schemadir ~/.local/share/gnome-shell/extensions/nero-workspaces@LudaLunizz/schemas
set org.gnome.shell.extensions.nero-workspaces KEY VALUE`):

| Key | Default | |
|---|---|---|
| `workspaces-per-monitor` | `4` | Workspaces every monitor has (1–12) |
| `wrap-around` | `false` | Scrolling past the last workspace goes to the first one |
| `show-osd` | `true` | Show "2 / 4" on the monitor that switched |
| `take-over-panel-scroll` | `true` | Handle Dash to Panel's panel scroll per monitor |
| `scroll-threshold` | `1.0` | Scroll per step (1.0 = one wheel notch) |
| `scroll-cooldown-ms` | `250` | Pause after a step in which scroll is ignored |
| `three-finger-swipe` | `true` | Three-finger swipe switches the monitor under the pointer only |
| `touchpad-scroll-speed` | `100` | Two-finger scroll speed in % (0–100), from the next login |

## 5. taskbar

A Windows-like taskbar: one panel at the **bottom** of every monitor, the app grid button on the
left, the clock, system menu and show-desktop button on the right, one icon per app with window
previews on click. It is built from two extensions from extensions.gnome.org, which you install
first: [Dash to Panel](https://extensions.gnome.org/extension/1160/dash-to-panel/) and
[Argos](https://extensions.gnome.org/extension/1176/argos/). Then:

```sh
./taskbar/setup-taskbar.sh                  # taskbar at the bottom + the sysmon script
./taskbar/setup-taskbar.sh --monitor DP-1   # same, and the Argos buttons on DP-1's taskbar
./taskbar/setup-taskbar.sh --undo           # put back the Dash to Panel settings from before the last run
```

The script saves your current Dash to Panel settings to `~/.local/state/nero/` before it changes
anything, and turns off Ubuntu's own dock (it would sit next to the taskbar).

**sysmon** (`taskbar/argos/sysmon.1s.sh`, copied to `~/.config/argos/`, refreshes every second)
shows, left to right: your Claude 5-hour and weekly usage as bars (green under 50 %, blue from
50 %, red from 80 %; only when Claude Code is logged in on this account, fetched every 5 minutes
in the background, needs `python3`), download and upload in MB/s, RAM use as a small bar, CPU
temperature and load, GPU temperature and load (NVIDIA through `nvidia-smi`, AMD through sysfs).
Click it for the details, a "Refresh usage" entry and a link to GNOME System Monitor.
Its state lives in `$XDG_RUNTIME_DIR/argos-sysmon` (mode 700).

**nero-argos-split@LudaLunizz** moves the Argos buttons out of the main panel (the taskbar on the
primary monitor) into the taskbar of one other monitor. Pick the monitor by its connector name
(`--monitor` above, or later):

```sh
gsettings --schemadir ~/.local/share/gnome-shell/extensions/nero-argos-split@LudaLunizz/schemas \
  set org.gnome.shell.extensions.nero-argos-split monitor DP-1
```

Empty, the primary monitor or an unplugged one: the buttons stay in (or go back to) the main
panel. When Dash to Panel rebuilds its panels, the buttons are taken home first and moved again
within 2 seconds.
Run only one extension that moves the Argos buttons: two of them pull the same buttons back and
forth every 2 seconds.

## License

Nero is MIT licensed (see `LICENSE`): the Rust core (`core/`), `nero-keep-onscreen`,
`nero-layout`, `nero-workspaces`, `taskbar/` and the tests.

The one exception is `tilingshell-patch/`. That patch is a derivative of
[Tiling Shell](https://github.com/domferr/tilingshell) © Domenico Ferraro and stays
under the GPL-3.0, like Tiling Shell itself (see `tilingshell-patch/LICENSE`). A
Tiling Shell built with `apply.sh` is therefore GPL-3.0 as a whole.
