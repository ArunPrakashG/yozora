# Sudo toggle

A Yozora module that shows whether passwordless sudo is on and lets you switch it. The bar item
is a lock, open and warning-toned while passwordless sudo is on.

Enabling writes `/etc/sudoers.d/99-passwordless-sudo` (`USER ALL=(ALL) NOPASSWD: ALL`) through
`pkexec`, after `visudo -c` accepts it, so it asks you to authenticate. The Enable button is
destructive, so Yozora asks you to confirm first. Disabling removes the file.

State comes from `sudo -n -l`, because `/etc/sudoers.d` is not readable by your user.

Needs `sudo`, `polkit` (`pkexec`) and optionally `notify-send`. This module is opt-in: add
`module/sudo-toggle` to your bar config. Run `test.sh` for the offline checks.
