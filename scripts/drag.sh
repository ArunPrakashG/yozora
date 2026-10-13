#!/usr/bin/env bash
# Drags from X1 Y1 to X2 Y2, then puts the cursor back. Refuses unless the cursor really is at
# X1 Y1, so a misfire never lands in another window.
# Usage: scripts/drag.sh X1 Y1 X2 Y2 [steps]
set -euo pipefail
ptr="$(dirname "$0")/../target/release/yz-pointer"
x1=$1 y1=$2 x2=$3 y2=$4 steps=${5:-20}
read -r ox oy < <(hyprctl cursorpos | tr -d ,)
"$ptr" move "$x1" "$y1"
sleep 0.1
read -r cx cy < <(hyprctl cursorpos | tr -d ,)
if [[ $cx != "$x1" || $cy != "$y1" ]]; then
  echo "cursor is at $cx,$cy, not $x1,$y1; not dragging" >&2
  "$ptr" move "$ox" "$oy"
  exit 1
fi
"$ptr" drag "$x2" "$y2" "$steps"
"$ptr" move "$ox" "$oy"
