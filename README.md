# Yozora

Yozora (夜空, "night sky") is an open-source desktop for Hyprland, written in Rust with
[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).

- **Yozora Bar** is a top panel and Waybar alternative. It has spring-based motion, glass
  popovers and near-zero cost when idle.
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
| `notifications` | A bell with a dot | History, Do Not Disturb | mako |
| `clock` | Date and time | Calendar with ISO weeks | — |
| `control-center` | Quick settings | Wi-Fi, Bluetooth, DND, dark mode, volume, power | — |
| `custom/<name>` | Your command's output | — (runs `on_click` when clicked) | — |

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
cargo build --release -p yozora-bar -p yozora-settings
```

Then install the binaries and the app manifests, so Settings can find the bar:

```bash
install -Dm755 target/release/yozora-bar target/release/yozora-settings -t ~/.local/bin
```

```bash
install -Dm644 packaging/apps/bar.toml packaging/apps/bar.settings.toml -t ~/.local/share/yozora/apps
```

## Start the bar with Hyprland

Pick one.

- **systemd** (the PKGBUILD installs the unit):

  ```bash
  systemctl --user enable --now yozora-bar.service
  ```

- **Hyprland autostart.** Copy [`packaging/hyprland/yozora.lua`](packaging/hyprland/yozora.lua)
  (Hyprland 0.55 and later) or [`yozora.conf`](packaging/hyprland/yozora.conf) (0.53 and 0.54)
  into `~/.config/hypr/` and include it. Both also add the blur layer rule and make Settings
  open as a floating window.

To bind a key to a popover:

```lua
hl.bind("SUPER + A", hl.dsp.exec_cmd("yozora-bar msg toggle control-center"))
```

### Do Not Disturb with mako

The bar switches mako to a `do-not-disturb` mode. mako only hides notifications in that mode
if your mako config says so:

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

The full reference is [docs/configuration.md](docs/configuration.md).

```bash
yozora-bar check
```

`check` validates both files, with line numbers for anything it doesn't understand, and probes
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
