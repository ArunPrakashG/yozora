#!/bin/sh
# Opens the user's terminal and runs the system update in it. The window stays open until Enter.
# Nothing runs in the bar process; this only launches a terminal.

CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/yozora/cachy-updates.json"
RUN='paru -Syu; printf "\nDone. Press Enter to close."; read _'

have() { command -v "$1" >/dev/null 2>&1; }

term=""
for candidate in "$TERMINAL" ghostty alacritty kitty foot wezterm konsole xterm; do
    if [ -n "$candidate" ] && have "$candidate"; then
        term="$candidate"
        break
    fi
done

if [ -z "$term" ]; then
    notify-send -a Updates "No terminal found" "Set \$TERMINAL or install a terminal, then run: paru -Syu" 2>/dev/null
    exit 1
fi

case "$term" in
    wezterm) "$term" start -- sh -c "$RUN" >/dev/null 2>&1 & ;;
    kitty) "$term" sh -c "$RUN" >/dev/null 2>&1 & ;;
    *) "$term" -e sh -c "$RUN" >/dev/null 2>&1 & ;;
esac

# Forget the cached result so the next status run starts a refresh.
rm -f "$CACHE"
exit 0
