#!/bin/sh
# Enables or disables passwordless sudo through a sudoers.d drop-in, using pkexec.
# Usage: toggle.sh enable|disable
# The drop-in goes through `visudo -c` before it is installed.
FILE="${YOZORA_SUDOERS_FILE:-/etc/sudoers.d/99-passwordless-sudo}"
USERNAME="$(id -un)"

NO_AGENT="Authentication was cancelled, or no polkit agent is running (start polkit-kde-authentication-agent-1)."

is_on() { sudo -n -l 2>/dev/null | grep -q 'NOPASSWD: ALL'; }

case "$1" in
disable)
    pkexec rm -f "$FILE"
    if ! is_on; then
        notify-send -a Sudo "Passwordless sudo disabled" "sudo will ask for your password again." 2>/dev/null
    else
        notify-send -a Sudo -u critical "Could not disable passwordless sudo" "$NO_AGENT" 2>/dev/null
    fi
    ;;
enable)
    TMP=$(mktemp) || exit 1
    echo "$USERNAME ALL=(ALL) NOPASSWD: ALL" > "$TMP"
    chmod 0440 "$TMP"
    if pkexec sh -c "visudo -c -f '$TMP' && install -m 0440 -o root -g root '$TMP' '$FILE'"; then
        notify-send -a Sudo -u critical "Passwordless sudo ENABLED" "Every command as $USERNAME can run sudo with no password. Turn it off when you're done." 2>/dev/null
    else
        notify-send -a Sudo -u critical "Could not enable passwordless sudo" "$NO_AGENT" 2>/dev/null
    fi
    rm -f "$TMP"
    ;;
*)
    echo "usage: toggle.sh enable|disable" >&2
    exit 2
    ;;
esac
