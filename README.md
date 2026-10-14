# Yozora

Yozora (夜空, "night sky") is an open-source desktop for Hyprland, written in Rust with
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).

- **Yozora Bar** is a top panel and Waybar alternative. It has spring-based motion, glass
  popovers and near-zero cost when idle.
- **Yozora Notify** is the notification server. Toasts match the bar's popovers, and the bar's
  notification center shows the history, grouped by app. It replaces mako or dunst.
- **Yozora Lock** is the lock screen: the blurred wallpaper, a clock, the password field, what's
  playing and the power buttons. It locks through `ext-session-lock-v1`, so if it crashes the
  session stays locked. It replaces hyprlock or swaylock.
- **Yozora Launch** is the launcher: apps, open windows, files, clipboard history, emoji, math
  and unit conversion, shell commands and web search, from one field. It stays running with a
  window ready on each monitor, so it shows on the next frame. It replaces rofi, wofi or fuzzel.
- **Yozora Settings** edits every Yozora app's configuration. It finds installed apps on its
  own, so a new tool shows up without Settings being rebuilt.

## Status

Alpha. The bar and Settings work day to day on the author's machine (Hyprland 0.56, NVIDIA).
Expect rough edges; see [Known limitations](#known-limitations).

## What the bar has

| Module | Shows | Popover | Needs |
|---|---|---|---|
| `launcher` | The Yozora mark | About, bar settings, reload, lock, log out, restart, shut down | — |
| `workspaces` | Workspace pills with a sprung indicator | — | Hyprland |
| `window` | The focused window's title | — | Hyprland |
| `media` | What's playing | Artwork, progress, controls, other players | MPRIS |
| `tray` | Tray icons | Each app's own menu | StatusNotifierItem |
| `network` | Wi-Fi or Ethernet | Networks, connect, Wi-Fi switch | NetworkManager |
| `sound` | Volume | Output and input sliders, devices | PulseAudio or pipewire-pulse |
| `devices` | Bluetooth | Paired devices and their batteries | BlueZ |
| `battery` | Charge | Time left, power profile | UPower, power-profiles-daemon |
| `notifications` | A bell with a dot | History grouped by app, Do Not Disturb | yozora-notify, or mako |
| `clock` | Date and time | Calendar with ISO weeks | — |
| `control-center` | Quick settings | Wi-Fi, Bluetooth, DND, dark mode, volume, power | — |
| `custom/<name>` | Your command's output | — (runs `on_click` when clicked) | — |

The bar also has an **on-screen display**: a capsule at the bottom of the focused monitor that
shows volume, mute, the microphone, the output device, brightness and the keyboard layout as
they change, whatever changed them, and leaves 1.4 s later. It replaces wob or swayosd; see
[Volume and brightness keys](#volume-and-brightness-keys).

A missing service never crashes the bar. Its module shows that it's unavailable and reconnects
when the service comes back.

The default bar is deliberately small: workspaces on the left, clock and control center on the
right. Add the rest in Settings → Yozora Bar → Modules, or in `~/.config/yozora/bar.toml`.

## Install

### Arch Linux

```bash
git clone https://github.com/ArunPrakashG/yozora.git
```

```bash
cd yozora/packaging/arch && makepkg -si
```

### From source

You need Rust (the toolchain is pinned in `rust-toolchain.toml`), clang, cmake and the Wayland,
xkbcommon, fontconfig and Vulkan development files.

```bash
cargo build --release -p yozora-bar -p yozora-notify -p yozora-lock -p yozora-launch -p yozora-settings
```

Then install the binaries and the app manifests, so Settings can find the apps:

```bash
install -Dm755 target/release/yozora-bar target/release/yozora-notify target/release/yozora-lock target/release/yozora-launch target/release/yozora-settings -t ~/.local/bin
```

```bash
install -Dm644 packaging/apps/*.toml -t ~/.local/share/yozora/apps
```

The lock screen checks passwords through its own PAM service, which needs root to install. Without
it, the lock uses the `login` service, which works on most systems.

```bash
sudo install -Dm644 packaging/pam/yozora-lock -t /etc/pam.d
```

## Start the bar with Hyprland

Only one notification server can run at a time, so stop mako or dunst first (remove its
`exec-once`, or `systemctl --user disable --now mako`). Then pick one.

- **systemd** (the PKGBUILD installs the units):

  ```bash
  systemctl --user enable --now yozora-bar.service yozora-notify.service
  ```

- **Hyprland autostart.** Copy [`packaging/hyprland/yozora.lua`](packaging/hyprland/yozora.lua)
  (Hyprland 0.55 and later) or [`yozora.conf`](packaging/hyprland/yozora.conf) (0.53 and 0.54)
  into `~/.config/hypr/` and include it. Both also add the blur layer rule and make Settings
  open as a floating window.

To bind a key to a popover:

```lua
hl.bind("SUPER + A", hl.dsp.exec_cmd("yozora-bar msg toggle control-center"))
```

## Volume and brightness keys

The bar notices volume, mute, output and keyboard layout changes by itself, so keys only need
to make the change:

```lua
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("pactl set-sink-volume @DEFAULT_SINK@ +5%"), { repeating = true, locked = true })
hl.bind("XF86AudioMute", hl.dsp.exec_cmd("pactl set-sink-mute @DEFAULT_SINK@ toggle"), { locked = true })
```

The kernel doesn't announce brightness changes, so brightness keys go through the bar, which
changes it through logind (no udev rule or `video` group needed) and shows the result:

```lua
hl.bind("XF86MonBrightnessUp", hl.dsp.exec_cmd("yozora-bar osd brightness +5%"), { repeating = true, locked = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("yozora-bar osd brightness -5%"), { repeating = true, locked = true })
```

The full set is in [`packaging/hyprland/yozora.lua`](packaging/hyprland/yozora.lua). Stop wob or
swayosd if you ran one. Turn the OSD off, move it under the bar or change how long it stays in
`[osd]` in `bar.toml`.

## Lock the screen

hypridle starts the lock. In `~/.config/hypr/hypridle.conf`:

```ini
general {
    lock_cmd = yozora-lock
    before_sleep_cmd = loginctl lock-session
    inhibit_sleep = 3  # suspend only once the screen is locked
}
```

Then `loginctl lock-session`, the bar's Lock item and idle timeouts all show it. To try the
screen without locking anything, run `yozora-lock --preview`; Escape closes it.

While locked, wrong passwords in a row (5 by default) make the field wait 30 s, then twice as
long each time. Media controls and the power buttons work without the password; the
notification pill only counts what arrived, never what it says.

## Open the launcher

Start `yozora-launch` with the session (`systemctl --user enable --now yozora-launch.service`,
or the commented lines in the Hyprland snippets), then bind a key:

```lua
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("yozora-launch toggle"))
```

Type to search apps, then open windows, commands and script commands, projects, SSH hosts, files
and pages from your browser. A math expression or a conversion (`12% of 340`, `5 km to mi`,
`0xff to bin`), a currency (`100 usd to inr`, using the ECB's daily rates) or a time (`time in
tokyo`, `3pm ist to pst`) shows its answer first. A prefix searches one thing only:

| Prefix | Searches |
|---|---|
| `@win` or `@w` | Open windows; Enter focuses one |
| `@clip` or `@c` | Clipboard history, from cliphist |
| `@files` or `@f` | Recent files, then your home folder through plocate |
| `@code` or `@p` | Git repositories and the folders Zed or VS Code opened recently; Enter opens the editor, Shift+Enter a terminal there |
| `@ssh` or `@s` | Hosts in `~/.ssh/config`; Enter connects in your terminal |
| `@browser` or `@b` | Bookmarks and history from Chromium-based browsers and Firefox |
| `@emoji`, `@e` or `:` | Emoji; Enter copies one |
| `=` | Math only |
| `$` | A shell command, run in your terminal (Shift+Enter runs it without one) |
| `?` | The web |

Up and Down (or Ctrl+N and Ctrl+K) move, Enter opens, Tab shows the other actions for the
selected row (an app's own actions, pin, hide, close a window, open a file's folder), Ctrl+P pins
an app to the empty view, and Ctrl+Enter searches the web for what's typed. Escape backs out one
step at a time; clicking outside closes it.

Script commands in the Raycast or Vicinae format (`# @raycast.title …` header lines) in
`~/.config/yozora/scripts` or `~/.local/share/vicinae/scripts` show up like commands. One with
arguments asks for them in the field before it runs; silent ones show their last line of output
as a notification, and `fullOutput` ones run in your terminal.

Apps you open often, and at this time of day, rank higher. That history stays on your machine in
`~/.local/state/yozora/launch.json`.

### Keeping mako

The bar also works with mako instead of yozora-notify, showing mako's history. For Do Not
Disturb, the bar switches mako to a `do-not-disturb` mode, which only hides notifications if
your mako config says so:

```ini
[mode=do-not-disturb]
invisible=1
```

`yozora-bar check` reports whether this is set up.

## Configure

Open **Yozora Settings** from your launcher, from the bar's menu, or with `yozora-settings`.
Every change is written to `~/.config/yozora/` and applies live. You can also edit the files
by hand. Comments and formatting survive edits from Settings.

- `theme.toml`: theme, accent, glass, font and motion, shared by every Yozora app.
- `bar.toml`: everything about the bar.
- `notify.toml`: how long toasts stay, how many show at once, where, and how much history.
- `lock.toml`: the lock screen's background, clock format, what it shows, and how long wrong
  passwords make it wait.
- `launch.toml`: the launcher's terminal, web search, results per section, and the commands
  behind Lock, Log out, Suspend, Restart and Shut down.

The full reference is [docs/configuration.md](docs/configuration.md).

```bash
yozora-bar check
```

`check` validates the files, with line numbers for anything it doesn't understand, and probes
each system service the enabled modules need.

## Known limitations

- **Clicking outside a popover closes it, and that click is used up.** The compositor owns the
  dismissal while a popover holds a grab. So switching from one open popover to another takes
  two clicks, and a popover the compositor dismisses closes without its animation. Popovers
  opened with `yozora-bar msg toggle` don't grab, so they close with the animation.
- **The popover animation is a fade and a short drop**, not the design's scale-in. GPUI can't
  scale a whole element tree.
- **Blur is Hyprland's.** A Wayland app can't see the pixels behind it, so the bar paints a tint
  and asks Hyprland to blur through a layer rule. Whether popovers get blurred on Hyprland
  0.56.2 hasn't been confirmed.
- **Notifications are plain text.** Bold and links in a body are removed, and only the first
  three lines show on a toast.
- **The calendar has no events.** No calendar backend is chosen yet.
- **The control center's dark mode tile lasts for the session.** To change the theme for good,
  use Settings → Appearance.
- **The first start after installing takes about 2 s to the first frame** on the test machine
  (NVIDIA, Vulkan), while the files load from disk. Later starts take 0.2 to 0.3 s.
- **The Vulkan driver dominates memory.** The bar's own memory is about 23 MB, but total RSS is
  about 110 MB on NVIDIA, mostly the driver's shared libraries. See
  [docs/benchmarks.md](docs/benchmarks.md).
- **Yozora apps skip OpenGL when a Vulkan driver is installed**, which saves about 40 MB. If
  Vulkan doesn't work, an app restarts itself with GL allowed. Set `YOZORA_GL=1` to always
  allow GL.
- **The lock screen takes about 2.4 s to appear** (release build, 2,752 installed font faces):
  1.6 s of that is GPUI reading every system font. Set hypridle's `inhibit_sleep = 3` so suspend
  waits until the screen is locked.
- **The lock screen needs a GPUI fork.** Upstream GPUI has no `ext-session-lock-v1` yet; Yozora
  pins a fork with [zed#65055](https://github.com/zed-industries/zed/pull/65055) applied. See
  [docs/architecture.md](docs/architecture.md#the-gpui-fork).
- **The launcher keeps about 120 MB resident**, most of it the Vulkan driver, so that it never
  starts cold. Each monitor's window waits under everything, empty and taking no input, so it
  doesn't block a fullscreen game's direct scanout.
- **The log shows `ERROR gpui::window: window not found` each time a popover closes.** It's
  harmless: Wayland sends one last event for a window GPUI has already dropped.
- **Tested on one machine:** Hyprland 0.56.2, a GTX 1650 Super, a 144 Hz and a 75 Hz rotated
  monitor. Battery and Bluetooth were tested against fake backends (`--fake`), because that
  machine has neither.

## Develop

```bash
cargo test --workspace
```

CI runs the tests along with `cargo fmt --check` and clippy with warnings as errors, so run
`cargo fmt` before pushing.

```bash
./target/release/yozora-bar --config /tmp/yozora-test --fake=all
```

`--fake` runs the whole UI on scripted backends, for hardware you don't have. Settings takes
`--config DIR` and `--apps DIR` for the same reason. `scripts/bench.sh` measures a running bar
against ironbar.

## Docs

- [Configuration reference](docs/configuration.md)
- [Benchmarks](docs/benchmarks.md)
- [Architecture](docs/architecture.md)
- [Phase 1 spike report](docs/spike-report.md)
- [Product brief](brief.md)

## License

[MIT](LICENSE)
