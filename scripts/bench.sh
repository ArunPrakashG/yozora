#!/usr/bin/env bash
# Measures a running yozora-bar against another bar (ironbar by default) on the same session.
#
# Usage: scripts/bench.sh [idle-seconds] [other-bar-process-name]
#
# For each process: CPU while idle, wakeups (context switches across all threads), anonymous
# and total RSS. For yozora-bar it then drives the workspace indicator and the popover through
# `msg bench` on every monitor and prints the frame timing. Run it on a quiet machine with a
# release build; nothing else should be compiling.
set -euo pipefail

idle=${1:-30}
other=${2:-ironbar}
bin="$(dirname "$0")/../target/release/yozora-bar"
clk=$(getconf CLK_TCK)

cpu_ticks() { awk '{print $14 + $15}' "/proc/$1/stat"; }
switches() {
  local total=0 n
  for t in /proc/"$1"/task/*/status; do
    n=$(awk '/^(voluntary|nonvoluntary)_ctxt_switches/ {s += $2} END {print s}' "$t" 2>/dev/null || echo 0)
    total=$((total + n))
  done
  echo "$total"
}
mem() { awk '/^RssAnon/ {a = $2} /^VmRSS/ {t = $2} END {printf "%.1f MB anon, %.1f MB total", a / 1024, t / 1024}' "/proc/$1/status"; }

measure() {
  local name=$1 pid
  pid=$(pgrep -xo "$name") || { echo "$name: not running"; return; }
  local c0 s0 c1 s1
  c0=$(cpu_ticks "$pid"); s0=$(switches "$pid")
  sleep "$idle"
  c1=$(cpu_ticks "$pid"); s1=$(switches "$pid")
  awk -v n="$name" -v c="$((c1 - c0))" -v s="$((s1 - s0))" -v t="$idle" -v hz="$clk" -v m="$(mem "$pid")" \
    'BEGIN {printf "%-12s CPU %.2f%%  wakeups %.0f/s  %s\n", n, 100 * c / hz / t, s / t, m}'
}

echo "Idle for ${idle}s each. Don't touch the mouse."
measure yozora-bar
measure "$other"

report=$(cat <<'PY'
import json, sys
r = json.load(sys.stdin)
f = r["frames"]
hz = 1000 / f["refresh_ms"]
print(f"{r['monitor']:<9} {r['what']:<10} {hz:5.1f} Hz  {f['frames']:5d} frames  "
      f"mean {f['mean_ms']:5.2f}  p50 {f['p50_ms']:5.2f}  p99 {f['p99_ms']:5.2f}  "
      f"max {f['max_ms']:6.2f} ms  late {f['late']}")
PY
)

echo
echo "Frame timing (msg bench, 20 runs each):"
monitors=$("$bin" msg stats | python3 -c 'import json, sys; print(" ".join(json.load(sys.stdin)["bars"]))')
for mon in $monitors; do
  for what in workspaces popover; do
    "$bin" msg bench "$what" 20 "$mon" | python3 -c "$report"
  done
done
