# Configuration

Yozora reads its configuration from `~/.config/yozora/` (or `$XDG_CONFIG_HOME/yozora/`). Every
file is optional: a missing file or key means the default. Apps watch the directory and apply
changes as soon as a file is saved.

| File | Read by | Holds |
|---|---|---|
| `theme.toml` | every Yozora app | theme, accent, glass, font, motion |
| `bar.toml` | Yozora Bar | the bar, its modules and monitors |

Settings edits these same files, keeping your comments and formatting. If a file has a syntax
error, apps keep running with the last good configuration and Settings shows the error instead
of editing. Unknown keys are reported with their line number:

```bash
yozora-bar check
```

## theme.toml

```toml
version = 1
theme   = "midnight"   # midnight | daybreak | auto (follows the desktop's light or dark setting)
accent  = "starlight"  # starlight | aurora | nebula | dawn
glass   = "balanced"   # clear | balanced | frosted: how much wallpaper shows through
font    = "Geist"      # falls back to Inter, then the system UI font

[motion]
reduced = false        # every spring becomes a 120 ms fade; nothing slides

[motion.popover]       # how popovers open
stiffness = 380.0
damping   = 28.0

[motion.indicator]     # workspace indicator, switches and highlights
stiffness = 520.0
damping   = 34.0
```

`auto` reads `color-scheme` from xdg-desktop-portal and follows it live. To switch at sunset,
use a tool such as darkman that sets that value.

Settings → Motion offers three popover presets: Crisp (560 / 40), Balanced (380 / 28, the
default) and Soft (220 / 20). It shows the response curve for any values you set by hand.

## bar.toml

Every key below is shown with its default.

```toml
version = 1

[bar]
style         = "flush"  # flush (edge to edge) | floating (inset, rounded)
height        = 34       # px; icons stay 16 px at every height
reserve_space = true     # tiled windows stop below the bar instead of going under it
blur          = true     # ask Hyprland to blur behind the bar; off makes the tint more opaque
monitors      = []       # connector names to show the bar on; empty means every monitor

[modules]
left   = ["workspaces"]
center = []
right  = ["clock", "control-center"]

[clock]
format       = "%a %-d %b  %H:%M"  # strftime; two spaces split date and time into two parts
week_numbers = true                # ISO weeks in the calendar

[workspaces]
count      = 5      # workspaces 1 to this are shown even when empty
per_monitor = false # each bar shows only the workspaces on its own monitor

[window]
max_length = 60     # longest window title, in characters

[media]
max_length = 36

# Commands behind the menu and the control center. Each runs through `sh -c`.
[session]
lock     = "loginctl lock-session"
logout   = "uwsm stop || loginctl terminate-session \"$XDG_SESSION_ID\""
reboot   = "systemctl reboot"
poweroff = "systemctl poweroff"
settings = "yozora-settings"
```

### Modules

Put module names in `left`, `center` or `right`, in the order you want them.

| Name | What it is |
|---|---|
| `launcher` | The Yozora mark; opens a menu with About, bar settings, reload and session actions |
| `workspaces` | Hyprland workspaces |
| `window` | The focused window's title |
| `media` | The current MPRIS player |
| `tray` | StatusNotifierItem tray icons and their menus |
| `network` | NetworkManager: Wi-Fi networks and wired status |
| `sound` | Output and input volume and devices, through PulseAudio (pipewire-pulse works) |
| `devices` | Bluetooth devices and their batteries, through BlueZ |
| `battery` | UPower battery and the power profile |
| `notifications` | mako's history and Do Not Disturb |
| `clock` | Date and time, with a calendar |
| `control-center` | Quick settings |
| `custom/<name>` | A command you define, below |

Destructive session actions (log out, restart, shut down) always ask first.

### Custom modules

A custom module shows the output of a command. Define it under `[custom.<name>]` and add
`custom/<name>` to a zone.

```toml
[modules]
right = ["custom/load", "clock", "control-center"]

[custom.load]
exec     = "cut -d' ' -f1 /proc/loadavg"
interval = 5          # seconds; run exec again on this interval
on_click = "foot htop" # optional; runs through sh -c
```

- **With `interval`**, the bar runs `exec` every `interval` seconds and shows the last
  non-empty line it printed.
- **Without `interval`**, the bar starts `exec` once and keeps it running. Each line it prints
  replaces the text, for scripts that watch something themselves. If it exits, the bar starts
  it again, waiting up to 30 s between tries if it keeps failing.

A line is plain text, or JSON with a `text` field:

```json
{"text": "3 updates", "tooltip": "pacman"}
```

`tooltip` shows under the module when the pointer rests on it. Use `\n` for more lines. A
failing command shows its error there instead.

### Per-monitor settings

Monitors are keyed by their Hyprland connector name, as `hyprctl monitors` shows it.

```toml
# Turn the bar off on one monitor.
[monitor."HDMI-A-1"]
enabled = false

# Or keep it, with fewer modules. Zones you leave out come from [modules].
[monitor."DP-2".modules]
left  = ["workspaces"]
right = ["clock", "control-center"]
```

`enabled` wins over `bar.monitors`. With `bar.monitors` set, monitors plugged in later don't get
a bar unless they're listed. Settings → Yozora Bar → Monitors writes these for you.

### Overriding the theme for the bar

A `[theme]` table in `bar.toml` overrides `theme.toml` for the bar only:

```toml
[theme]
accent = "dawn"
```

## The control socket

Each running app listens on `$XDG_RUNTIME_DIR/yozora/<id>.sock`. The bar's commands, also
available as `yozora-bar msg …`:

| Command | Does |
|---|---|
| `toggle MODULE [MONITOR]` | Opens or closes a module's popover. `tray:N` or `tray:NAME` opens a tray menu |
| `close` | Closes any open popover |
| `reload` | Re-reads the configuration now |
| `stats` | Frame timing and memory |
| `bench [workspaces\|popover] [N] [MONITOR]` | Animates N times and reports frame timing |
| `ping` | Version and configuration warnings (what Settings uses) |
| `quit` | Exits |

For example, in Hyprland's Lua config:

```lua
hl.bind("SUPER + A", hl.dsp.exec_cmd("yozora-bar msg toggle control-center"))
```

## Adding an app to Settings

Settings finds apps through manifests in `yozora/apps/` under `$XDG_DATA_HOME` (usually
`~/.local/share`) and each directory in `$XDG_DATA_DIRS` (usually `/usr/share`). An app
installed while Settings is open appears within a few seconds. A user manifest wins over a
system one with the same `id`.

### The manifest

```toml
# /usr/share/yozora/apps/launch.toml
id          = "launch"
name        = "Yozora Launch"
binary      = "yozora-launch"            # what Settings runs for "Start"
icon        = "yozora-launch"            # optional, a freedesktop icon name
description = "Find and open apps."      # optional
config      = "launch.toml"              # in ~/.config/yozora/
settings    = "launch.settings.toml"     # optional settings schema, next to this file
socket      = "launch.sock"              # optional, in $XDG_RUNTIME_DIR/yozora/
```

With a `socket`, Settings sends `ping` every two seconds to show whether the app is running and
which version. Reply with `{"ok": true, "data": {"version": "0.1.0", "warnings": []}}`. Without
a socket, Settings says changes apply when the app reloads.

### The settings schema

The schema describes the app's options. Settings builds a page from it, so the app gets a
Settings page without any change to Settings.

```toml
[[section]]
title       = "Results"
description = "What shows up as you type."   # optional

[[section.field]]
key     = "results.max"   # dotted path into the app's config file
kind    = "int"           # bool | enum | int | float | string | list
label   = "Results shown"
help    = "How many matches to list."   # optional
default = 8
min     = 1               # int and float only, optional
max     = 30
step    = 1

[[section.field]]
key           = "results.layout"
kind          = "enum"
label         = "Layout"
options       = ["list", "grid"]
option_labels = ["List", "Grid"]   # optional; otherwise the values themselves
default       = "list"
```

How each kind is shown:

| Kind | Control |
|---|---|
| `bool` | A switch |
| `enum` | A segmented picker for up to four short options, pills otherwise |
| `int`, `float` | A number field with − and + buttons, kept within `min` and `max` |
| `string` | A text field; clearing it removes the key |
| `list` | Chips you can remove, and a field that adds one on Enter |

Each row shows its key and a Reset link once the key is set. "Restore defaults" removes every
key the schema lists. If the schema doesn't parse, the page shows the error instead.

The bar's own manifest and schema are in
[`packaging/apps/`](../packaging/apps/) and make a working example.
