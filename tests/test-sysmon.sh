#!/usr/bin/env bash
# Runs the Argos sysmon script twice in a throwaway HOME (no Claude login, so no
# network call) and checks the panel line and the dropdown.
set -euo pipefail
script=$(cd "$(dirname "$0")/.." && pwd)/taskbar/argos/sysmon.1s.sh
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
run() { HOME=$tmp XDG_RUNTIME_DIR=$tmp bash "$script"; }
run >/dev/null; sleep 1; out=$(run)
# the background usage fetch may still be writing; give it a moment
for _ in 1 2 3 4 5; do grep -q '^nologin' "$tmp/argos-sysmon/claude" 2>/dev/null && break; sleep 1; done
fail() { echo "FAIL: $1"; echo "$out"; exit 1; }
[[ $(sed -n 1p <<<"$out") == "<tt>"*"</tt> | useMarkup=true" ]] || fail "panel line"
grep -qx -- '---' <<<"$out" || fail "dropdown separator"
grep -q '^RAM used: [0-9]\+%$' <<<"$out" || fail "RAM line"
grep -q '^CPU: [0-9]\+%' <<<"$out" || fail "CPU line"
grep -q '^nologin' "$tmp/argos-sysmon/claude" || fail "usage state without a login"
[[ $(stat -c %a "$tmp/argos-sysmon") == 700 ]] || fail "state folder mode"
echo "sysmon ok"
