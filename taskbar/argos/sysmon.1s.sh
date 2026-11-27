#!/usr/bin/env bash
# Argos panel monitor: [Claude 5h] [Claude week]  DOWN UP [RAM bar] CPU°C CPU% GPU°C GPU%
# Install: taskbar/setup-taskbar.sh copies it to ~/.config/argos/ (the "1s" = refresh every second)

RED='#ff3b3b'; GREEN='#3bd13b'; BLUE='#4a9bff'
# Claude usage bar colours: <50 green, 50+ blue, 80+ red
C_GREEN='#5fb85f'; C_BLUE='#3d8bfd'; C_RED='#e5484d'; C_TRACK='#4a4a4a'

# never fall back to /tmp: another user could pre-create the folder and plant state
STATE="${XDG_RUNTIME_DIR:-$HOME/.cache}/argos-sysmon"
mkdir -p -m 700 "$STATE"
now=$(date +%s%N)
# state values end up in bash arithmetic, which runs $(...) hidden in them; accept digits only
isnum() { [[ $1 =~ ^[0-9]+$ ]]; }

# ---------- Claude usage (fetched in background every 5 min) ----------
# Uses the login from Claude Code (~/.claude/.credentials.json).
USAGE="$STATE/claude"
fetch_usage() {
  # hard limit: urlopen's timeout doesn't cover DNS, and the lock goes stale after 30 s
  timeout 20 python3 - "$1" <<'PY'
import json, os, sys, time, urllib.error, urllib.request
out = sys.argv[1]
# keep the last good numbers (and when we got them) on failure so the bars don't blank out
try:
    prev = open(out).read().split()[1:6]
except Exception:
    prev = []
prev = " ".join((prev + ["-"] * 5)[:5])
try:
    cred = json.load(open(os.path.expanduser("~/.claude/.credentials.json")))
    token = cred["claudeAiOauth"]["accessToken"]
    req = urllib.request.Request("https://api.anthropic.com/api/oauth/usage", headers={
        "Authorization": f"Bearer {token}",
        "anthropic-beta": "oauth-2025-04-20",
        "User-Agent": "argos-sysmon"})
    d = json.load(urllib.request.urlopen(req, timeout=10))
    def get(k):
        p = d.get(k) or {}
        u = p.get("utilization")
        return ("-" if u is None else str(round(u))), (p.get("resets_at") or "-")
    s, sr = get("five_hour"); w, wr = get("seven_day")
    line = f"ok {s} {w} {sr} {wr} {int(time.time())}"
except FileNotFoundError:
    line = "nologin - - - - -"
except urllib.error.HTTPError as e:
    line = f"err:HTTP{e.code} {prev}"
except Exception as e:
    line = f"err:{type(e).__name__} {prev}"
tmp = out + ".tmp"
open(tmp, "w").write(line + "\n"); os.replace(tmp, out)
PY
}
age=999
[[ -f $USAGE ]] && age=$(( $(date +%s) - $(stat -c %Y "$USAGE") ))
# "Refresh usage" in the dropdown runs this script with "refresh": fetch now,
# unless the last fetch was under 30 s ago (keeps clicks clear of the rate limit)
if [[ $1 == refresh ]]; then
  (( age >= 30 )) && mkdir "$STATE/claude.lock" 2>/dev/null \
    && { fetch_usage "$USAGE"; rmdir "$STATE/claude.lock"; }
  exit 0
fi
if (( age >= 300 )) && mkdir "$STATE/claude.lock" 2>/dev/null; then
  setsid bash -c "$(declare -f fetch_usage); fetch_usage '$USAGE'; rmdir '$STATE/claude.lock'" \
    </dev/null >/dev/null 2>&1 &
fi
# clear a stale lock (e.g. after a crash)
[[ -d $STATE/claude.lock ]] && (( $(date +%s) - $(stat -c %Y "$STATE/claude.lock") > 30 )) \
  && rmdir "$STATE/claude.lock" 2>/dev/null
read -r c_status c_sess c_week c_sess_reset c_week_reset c_ok_at 2>/dev/null < "$USAGE"
: "${c_status:=wait}" "${c_sess:=-}" "${c_week:=-}"
[[ $c_status =~ ^(ok|nologin|err:[A-Za-z0-9_]+)$ ]] || c_status=wait
isnum "$c_sess" || c_sess=-
isnum "$c_week" || c_week=-
[[ $c_sess_reset =~ ^[0-9T:.+-]+$ ]] || c_sess_reset=-
[[ $c_week_reset =~ ^[0-9T:.+-]+$ ]] || c_week_reset=-
isnum "$c_ok_at" || c_ok_at=

# filling bar: 6 cells, 1/8-cell resolution, coloured by threshold
ubar() {
  local p=$1 w=6 col out="" i
  if [[ $p == - ]]; then
    printf "<span color='%s'>%s</span>" "$C_TRACK" "██████"; return
  fi
  (( p > 100 )) && p=100
  if   (( p >= 80 )); then col=$C_RED
  elif (( p >= 50 )); then col=$C_BLUE
  else                     col=$C_GREEN; fi
  local eighths=$(( p * w * 8 / 100 )) full part
  full=$(( eighths / 8 )); part=$(( eighths % 8 ))
  local parts=("" ▏ ▎ ▍ ▌ ▋ ▊ ▉)
  for ((i=0; i<full; i++)); do out+="█"; done
  local fill="$out" rest=""
  local used=$full
  [[ $part -gt 0 ]] && { fill+="${parts[$part]}"; used=$((used+1)); }
  for ((i=used; i<w; i++)); do rest+="█"; done
  # partial cell sits on the track colour so the bar reads as one piece
  printf "<span color='%s' bgcolor='%s'>%s</span><span color='%s'>%s</span>" \
    "$col" "$C_TRACK" "$fill" "$C_TRACK" "$rest"
}
upct() { [[ $1 == - ]] && echo " - " || printf '%3s' "$1"; }
ucol() { [[ $1 == - ]] && echo "$C_TRACK" && return
         (( $1 >= 80 )) && echo "$C_RED" && return
         (( $1 >= 50 )) && echo "$C_BLUE" && return; echo "$C_GREEN"; }
reset_in() {   # ISO time -> "2h 14m" / "3d 5h"
  [[ -z $1 || $1 == - ]] && { echo "-"; return; }
  local t; t=$(date -d "$1" +%s 2>/dev/null) || { echo "-"; return; }
  local s=$(( t - $(date +%s) )); (( s < 0 )) && s=0
  if (( s >= 86400 )); then echo "$((s/86400))d $(( s%86400/3600 ))h"
  else echo "$((s/3600))h $(( s%3600/60 ))m"; fi
}

# ---------- Network (MB/s, all real interfaces) ----------
read -r rx tx < <(awk 'NR>2 {sub(/^ +/,""); split($0,a,/[: ]+/);
  # skip virtual and VPN interfaces; VPN traffic also passes through the real NIC
  if (a[1] !~ /^(lo|docker|veth|br-|virbr|vnet|tun|tap|wg|ppp|ipsec|pvpn|proton|nordlynx|tailscale|zt)/) {rx+=a[2]; tx+=a[10]}}
  END {printf "%.0f %.0f\n", rx, tx}' /proc/net/dev)
down="0"; up="0"
if [[ -f $STATE/net ]]; then
  read -r pt prx ptx < "$STATE/net"
  isnum "$pt" && isnum "$prx" && isnum "$ptx" && read -r down up < <(awk -v dt=$((now-pt)) -v drx=$((rx-prx)) -v dtx=$((tx-ptx)) '
    function fmt(x) { if (x < 0.05) return "0"; if (x < 10) return sprintf("%.1f", x); return sprintf("%.0f", x) }
    BEGIN { if (dt<=0) dt=1; print fmt(drx*1e9/dt/1048576), fmt(dtx*1e9/dt/1048576) }')
fi
echo "$now $rx $tx" > "$STATE/net"

# ---------- RAM (vertical bar) ----------
ram_pct=$(awk '/^MemTotal/{t=$2} /^MemAvailable/{a=$2} END{printf "%d", (t-a)*100/t}' /proc/meminfo)
bars=(▁ ▂ ▃ ▄ ▅ ▆ ▇ █)
ram_bar=${bars[$(( (ram_pct*7 + 50) / 100 ))]}

# ---------- CPU usage ----------
read -r _ u n s i w q sq st _ < /proc/stat
total=$((u+n+s+i+w+q+sq+st)); idle=$((i+w))
cpu_use=0
if [[ -f $STATE/cpu ]]; then
  read -r pt pi < "$STATE/cpu"
  if isnum "$pt" && isnum "$pi"; then
    dt=$((total-pt)); (( dt > 0 )) && cpu_use=$(( (100*(dt-(idle-pi)) + dt/2) / dt ))
  fi
fi
echo "$total $idle" > "$STATE/cpu"

# ---------- CPU temp ----------
cpu_temp="-"
for h in /sys/class/hwmon/hwmon*; do
  case "$(cat "$h/name" 2>/dev/null)" in
    coretemp|k10temp|zenpower|cpu_thermal|acpitz)
      [[ -r $h/temp1_input ]] && { cpu_temp=$(( $(cat "$h/temp1_input") / 1000 )); break; } ;;
  esac
done
if [[ $cpu_temp == "-" ]]; then
  for z in /sys/class/thermal/thermal_zone*; do
    [[ $(cat "$z/type" 2>/dev/null) == x86_pkg_temp ]] && { cpu_temp=$(( $(cat "$z/temp") / 1000 )); break; }
  done
fi

# ---------- GPU temp / usage ----------
gpu_temp="-"; gpu_use="-"
if command -v nvidia-smi >/dev/null; then
  IFS=', ' read -r gpu_temp gpu_use < <(nvidia-smi --query-gpu=temperature.gpu,utilization.gpu \
    --format=csv,noheader,nounits 2>/dev/null | head -n1)
else
  for d in /sys/class/drm/card*/device; do            # AMD
    if [[ -r $d/gpu_busy_percent ]]; then
      gpu_use=$(cat "$d/gpu_busy_percent")
      t=$(cat "$d"/hwmon/hwmon*/temp1_input 2>/dev/null | head -n1)
      [[ -n $t ]] && gpu_temp=$(( t / 1000 ))
      break
    fi
  done
fi
: "${gpu_temp:=-}" "${gpu_use:=-}"

# ---------- Panel line ----------
c() { printf "<span color='%s'>%s</span>" "$1" "$2"; }
printf "<tt>%s %s  %s %s  %s %s %s %s %s %s %s</tt> | useMarkup=true\n" \
  "$(ubar "$c_sess")" "$(c "$(ucol "$c_sess")" "$(upct "$c_sess")")" \
  "$(ubar "$c_week")" "$(c "$(ucol "$c_week")" "$(upct "$c_week")")" \
  "$(c $RED   "$(printf '%4s' "$down")")" \
  "$(c $RED   "$(printf '%-4s' "$up")")" \
  "$(c $GREEN "$ram_bar")" \
  "$(c $BLUE  "$(printf '%3s' "$cpu_temp")")" \
  "$(c $BLUE  "$(printf '%3s' "$cpu_use")")" \
  "$(c $BLUE  "$(printf '%3s' "$gpu_temp")")" \
  "$(c $BLUE  "$(printf '%3s' "$gpu_use")")"

# ---------- Dropdown ----------
echo "---"
printf "<tt>Session %s %s%%   Week %s %s%%</tt> | useMarkup=true\n" \
  "$(ubar "$c_sess")" "$c_sess" "$(ubar "$c_week")" "$c_week"
echo "Session resets in $(reset_in "$c_sess_reset")   Week resets in $(reset_in "$c_week_reset")"
case $c_status in
  nologin) echo "Claude usage: sign in to Claude Code first (run: claude)" ;;
  err:HTTP401|err:HTTP403)
           echo "Claude usage: login expired (${c_status#err:}), run: claude" ;;
  err:HTTP429)
           echo "Claude usage: rate-limited, showing last values" ;;
  err*)    echo "Claude usage: couldn't fetch (${c_status#err:}), showing last values" ;;
  wait)    echo "Claude usage: loading…" ;;
esac
# time since the last successful fetch, not the last attempt (errors rewrite the file too)
ok_age=$(( $(date +%s) - ${c_ok_at:-0} ))
if [[ -z $c_ok_at ]];       then updated="never"
elif (( ok_age < 60 ));     then updated="just now"
elif (( ok_age < 86400 ));  then updated="$(( ok_age / 60 ))m ago"
else                             updated="$(( ok_age / 86400 ))d ago"; fi
echo "Refresh usage (updated $updated) | bash=$(readlink -f "$0") param1=refresh terminal=false refresh=true"
echo "Open usage page | href=https://claude.ai/settings/usage"
echo "---"
echo "Download: $down MB/s   Upload: $up MB/s"
echo "RAM used: $ram_pct%"
echo "CPU: $cpu_use%   ${cpu_temp}°C"
echo "GPU: $gpu_use%   ${gpu_temp}°C"
echo "---"
echo "System Monitor | bash=gnome-system-monitor terminal=false"
