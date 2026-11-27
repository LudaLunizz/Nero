#!/usr/bin/env bash
# Windows-like taskbar for GNOME: one Dash to Panel taskbar at the BOTTOM of every
# monitor (app grid button left; clock, system menu and show-desktop right), with the
# sysmon Argos script in it and, optionally, the Argos buttons on another monitor.
#
#   taskbar/setup-taskbar.sh [--monitor CONNECTOR]   set up (CONNECTOR e.g. DP-1)
#   taskbar/setup-taskbar.sh --undo                  load the settings saved by the last run
#
# Needs Dash to Panel and Argos installed from extensions.gnome.org first.
# Every run saves the current Dash to Panel settings to
# ~/.local/state/nero/dash-to-panel-<time>.dconf before it changes anything.
set -euo pipefail
cd "$(dirname "$0")"

DTP=dash-to-panel@jderose9.github.com
ARGOS=argos@pew.worldwidemann.com
SPLIT=nero-argos-split@LudaLunizz
EXT_DIR=${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions
STATE=${XDG_STATE_HOME:-$HOME/.local/state}/nero
DCONF_PATH=/org/gnome/shell/extensions/dash-to-panel/

monitor=""; undo=0
while (($#)); do
  case $1 in
    --monitor) monitor=${2:?--monitor needs a connector name, e.g. DP-1}; shift 2 ;;
    --undo) undo=1; shift ;;
    -h|--help) sed -n 2,11p "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done
[[ -z $monitor || $monitor =~ ^[A-Za-z0-9-]+$ ]] || { echo "bad connector name: $monitor" >&2; exit 2; }

if ((undo)); then
  last=$(ls -1 "$STATE"/dash-to-panel-*.dconf 2>/dev/null | tail -n1 || true)
  [[ -n $last ]] || { echo "nothing to undo: no saved settings in $STATE" >&2; exit 1; }
  dconf reset -f "$DCONF_PATH"
  dconf load "$DCONF_PATH" < "$last"
  echo "Dash to Panel settings put back from $last"
  exit 0
fi

have() { gnome-extensions info "$1" >/dev/null 2>&1; }
have "$DTP" || { echo "Dash to Panel is not installed: https://extensions.gnome.org/extension/1160/dash-to-panel/" >&2; exit 1; }

# 1. back up, then the taskbar at the bottom of every monitor
mkdir -p -m 700 "$STATE"
backup="$STATE/dash-to-panel-$(date +%Y%m%d-%H%M%S).dconf"
dconf dump "$DCONF_PATH" > "$backup"
echo "saved the current Dash to Panel settings to $backup"

# Dash to Panel keeps its schema in its own folder, where plain gsettings does not look
dtp_dir=$(gnome-extensions info "$DTP" | sed -n 's/^ *Path: //p')
[[ -d $dtp_dir/schemas ]] || { echo "can't find Dash to Panel's schemas folder ($dtp_dir)" >&2; exit 1; }
s() { gsettings --schemadir "$dtp_dir/schemas" set org.gnome.shell.extensions.dash-to-panel "$@"; }
s panel-position 'BOTTOM'
s panel-positions '{}'                 # no per-monitor override: every monitor uses panel-position
s dot-position 'BOTTOM'                # running-app dots under the icons
s show-favorites-all-monitors true     # pinned apps on every monitor's taskbar
s group-apps true                      # one icon per app, like Windows' combined buttons
s click-action 'TOGGLE-SHOWPREVIEW'    # click an app with several windows: window previews
s window-preview-title-position 'TOP'
s panel-element-positions '{}'         # Dash to Panel's default order: apps button left, clock + tray right
gnome-extensions disable ubuntu-dock@ubuntu.com 2>/dev/null || true   # Ubuntu's own dock would sit next to it
gnome-extensions enable "$DTP"

# 2. the sysmon script in Argos
if have "$ARGOS"; then
  mkdir -p ~/.config/argos
  if [[ -e ~/.config/argos/sysmon.1s.sh ]] && ! cmp -s argos/sysmon.1s.sh ~/.config/argos/sysmon.1s.sh; then
    cp ~/.config/argos/sysmon.1s.sh "$STATE/sysmon.1s.sh.$(date +%Y%m%d-%H%M%S).bak"
  fi
  install -m 755 argos/sysmon.1s.sh ~/.config/argos/sysmon.1s.sh
  gnome-extensions enable "$ARGOS"
  echo "sysmon.1s.sh installed in ~/.config/argos"
else
  echo "Argos is not installed, skipped the sysmon script: https://extensions.gnome.org/extension/1176/argos/"
fi

# 3. optional: the Argos buttons on another monitor's taskbar
if [[ -n $monitor ]]; then
  mkdir -p "$EXT_DIR"
  rm -rf "${EXT_DIR:?}/$SPLIT"
  cp -r "$SPLIT" "$EXT_DIR/"
  glib-compile-schemas "$EXT_DIR/$SPLIT/schemas"
  gsettings --schemadir "$EXT_DIR/$SPLIT/schemas" \
    set org.gnome.shell.extensions.nero-argos-split monitor "$monitor"
  gnome-extensions enable "$SPLIT" 2>/dev/null \
    || echo "log out and back in once, then: gnome-extensions enable $SPLIT"
  echo "Argos buttons go to the taskbar on $monitor"
fi
echo "done (a new extension shows up after the next log in on Wayland)"
