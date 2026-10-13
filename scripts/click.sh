#!/usr/bin/env bash
# Clicks at layout coordinates X Y, then puts the cursor back. Refuses to click unless the
# cursor really is at X Y, so a misfire never lands in another window.
# Usage: scripts/click.sh X Y [hold-ms]
set -euo pipefail
ptr="$(dirname "$0")/../target/release/yz-pointer"
x=$1 y=$2 hold=${3:-40}
read -r ox oy < <(hyprctl cursorpos | tr -d ,)
"$ptr" move "$x" "$y"
sleep 0.1
read -r cx cy < <(hyprctl cursorpos | tr -d ,)
if [[ $cx != "$x" || $cy != "$y" ]]; then
  echo "cursor is at $cx,$cy, not $x,$y; not clicking" >&2
  "$ptr" move "$ox" "$oy"
  exit 1
fi
"$ptr" click "$hold"
"$ptr" move "$ox" "$oy"
