# Yozora architecture

Status: built, 2026-10-13. Yozora Bar and Yozora Settings follow this design. Where the build
differs from the original proposal, this document says what was built, and
[What the build changed](#what-the-build-changed) lists the differences. The Phase 1 findings
are in the [spike report](spike-report.md).

The product brief is [`brief.md`](../brief.md). The approved visual design (desktop prototype,
motion system, surfaces, variants, settings, design language) lives on a Claude Design canvas;
its tokens are restated in [Config and theme](#config-and-theme) so this repo stays the source
of truth.

## Principles that shape the design

- **Idle is free.** When nothing changes, nothing renders, polls or wakes up.
- **One process per tool.** Each Yozora app is its own binary. Config files are the interface
  between them, so hand edits and the Settings app behave the same.
- **Crates follow consumers.** A shared crate appears only when a second binary needs it.
- **Nothing blocks the UI thread.** System services are talked to from background tasks.
- **Missing services degrade, never crash.** A module whose service is gone shows that, and
  reconnects on its own.

## Processes

| Process | Kind | Lifetime |
|---|---|---|
| `yozora-bar` | GPUI app, one layer-shell window per monitor, popovers as `xdg_popup` | Session-long, autostarted |
| `yozora-settings` | GPUI app, one normal window | Only while open |
| Future: Launch, Notify, Studio, … | One binary each | Their own |

`yozora-bar` is a single process for all monitors. Services (Hyprland, PulseAudio, D-Bus) are
shared by every bar window, so a second monitor costs one more window, not more connections.

`yozora-settings` never talks to a tool's internals. It edits that tool's TOML file, and the tool
hot-reloads it. Launching Settings while it is already open focuses the existing window through
its control socket.

## Ecosystem discovery

Every Yozora app installs a small manifest. Settings finds apps by scanning for manifests, so a
newly installed tool shows up in Settings without Settings being rebuilt or restarted.

```toml
# /usr/share/yozora/apps/bar.toml  (or ~/.local/share/yozora/apps/ for user installs)
id       = "bar"
name     = "Yozora Bar"
binary   = "yozora-bar"
icon     = "yozora-bar"
config   = "bar.toml"            # in ~/.config/yozora/
settings = "bar.settings.toml"   # settings schema, next to this manifest
socket   = "bar.sock"            # in $XDG_RUNTIME_DIR/yozora/
```

How Settings uses it:

1. On start, scan `yozora/apps/` under `$XDG_DATA_HOME`, then each entry of `$XDG_DATA_DIRS`.
   A user manifest overrides a system one with the same `id`. Keep watching those directories,
   so installing or removing a tool adds or removes its sidebar section live.
2. For each app, show a sidebar entry. If Settings has a built-in panel for that `id` (the bar's
   module zones and monitor arrangement need one), use it. Otherwise render a **generic panel**
   from the app's settings schema.
3. Show whether the app is running by connecting to its socket and sending `ping`. Offer
   "Start" when it is not.

The settings schema is declarative, so a new tool gets a working Settings page for free:

```toml
# bar.settings.toml (sketch)
[[section]]
title = "Layout"

[[section.field]]
key     = "bar.height"        # dotted path into the app's config file
kind    = "enum"              # bool | enum | int | float | string | list
options = [30, 34, 38]
default = 34
label   = "Height"
help    = "Icons stay 16 px at every height."
```

## Repository layout

```
crates/
  yozora-bar/          bin
    src/main.rs          run, single instance, crash log, `check`
    src/cli.rs           argument parsing and `msg`
    src/app.rs           a bar per monitor, reconciled on hotplug and config changes; IPC
    src/bar.rs           zones, item chrome, the workspace indicator, benchmarks
    src/popover.rs       popover windows: open, spring, close
    src/modules/         one file per module, each with its bar item and popover
    src/services/        hyprland, audio (pactl), network, power, bluetooth, media, tray,
                         notifications (mako), custom; each with a fake
  yozora-settings/     bin
    src/app.rs           window, file and manifest watching, status polling, portal theme
    src/apps.rs          discovered apps and whether they're running
    src/doc.rs           one config file being edited
    src/view.rs          sidebar, header, page, TOML preview, footer
    src/pages/           appearance, motion, bar, modules, monitors, about, and generic
  yozora-config/       bar.toml and theme.toml types, loading with warnings, editing, watching,
                       manifests and settings schemas
  yozora-ipc/          the one-line JSON control socket
  yozora-ui/           theme tokens, icons, springs, frame timing, controls, text input
  yz-pointer/          virtual-pointer clicks and drags for scripted UI tests
packaging/             manifests, PKGBUILD, systemd unit, Hyprland snippets, desktop files
scripts/               click.sh, drag.sh, bench.sh
docs/
```

Platform adapters live in `yozora-bar/src/services` until a second app needs them.

## Inside the bar: services, models, views

```
 Hyprland socket2/socket1 ──┐
 PulseAudio (pipewire-pulse)┤   service tasks                GPUI main thread
 NetworkManager · UPower ·  ├─▶ (background executor)  ──▶  Entity<Model> per service
 BlueZ · MPRIS · SNI · mako ┘    parse → diff → send             │ cx.notify() only on change
            ▲                                                    ▼
            └──── Command channel ◀── clicks, sliders ──  BarWindow per monitor ──▶ Popover
 bar.toml ── inotify ──▶ Config entity ──▶ layout, theme, motion
 $XDG_RUNTIME_DIR/yozora/bar.sock ──▶ toggle popover, reload, stats
```

- **Services** are async tasks. Each owns its connection, turns raw events into a typed model
  that implements `PartialEq`, and sends it to the main thread only when it changed.
- **Models** are one GPUI `Entity` per service, not one global state. Views observe only the
  models they use, so a volume change re-renders the sound item and nothing else.
- **Commands** flow back over a channel (`SetVolume`, `SwitchWorkspace`, …). The UI updates
  optimistically. Slider writes are coalesced to the latest value per frame.
- **Lazy start.** A service starts only when an enabled module needs it. The default minimal bar
  (workspaces, clock, control center) connects to Hyprland and nothing else.
- **Loss and recovery.** When a D-Bus name disappears (`NameOwnerChanged`) or a socket closes,
  the model switches to `Unavailable` and the service reconnects with backoff.

Backends are traits with one real and one fake implementation each:

| Area | First backend | Notes |
|---|---|---|
| Workspaces, windows, monitors | Hyprland socket2 events, socket1 `j/` commands | Our own thin client; the `hyprland` crate tends to lag IPC changes |
| Audio | `pactl subscribe` for changes, `pactl -f json` for state | Works with PulseAudio and pipewire-pulse |
| Network | NetworkManager over D-Bus (`zbus`) | A new secured network asks the session's NetworkManager secret agent for its password; the bar has no password prompt of its own |
| Power | UPower, power-profiles-daemon | Peripheral batteries come from UPower devices too |
| Bluetooth | BlueZ `ObjectManager` | Not testable on the dev machine: fake backend plus python-dbusmock |
| Media | MPRIS | Position is extrapolated locally and only ticks while a popover shows it |
| Tray | StatusNotifierItem host (plus watcher if none runs), DBusMenu | Menus render as our own popovers |
| Notifications | mako's D-Bus interface (the one `makoctl` uses) | Re-reads history when mako emits a signal. mako can't clear history, so cleared entries are hidden by the bar and remembered in `$XDG_STATE_HOME/yozora/` |
| Theme follow | xdg-desktop-portal `color-scheme` | Drives the Automatic theme |

## Modules

A module is a GPUI view plus a descriptor in an explicit registry (no link-time magic):

```rust
// Sketch; final signatures depend on the spike.
pub struct ModuleDescriptor {
    pub id: &'static str,                      // "clock", "workspaces", …
    pub default_zone: Zone,
    pub services: &'static [ServiceKind],      // started lazily
    pub build: fn(&toml::Value, &Services, &mut App) -> anyhow::Result<ModuleHandle>,
}

pub struct ModuleHandle {
    pub item: AnyView,                         // what sits in the bar
    pub popover: Option<fn(&mut App) -> AnyView>, // built when opened
}
```

Ways to add a module:

1. **Rust, compiled in.** Implement a view, add a descriptor. This is the first-class path and
   what the Settings "Add a module" list shows.
2. **`custom` module.** Runs a long-lived command that prints JSON lines, for example
   `{"text": "23°", "icon": "weather-clear", "tooltip": "Clear", "state": "normal"}`.
   Event-driven by default, with an optional interval. Covers cpu, memory, weather and
   scripts without Rust.
3. **Not supported: loading Rust modules from `.so` files.** We pin a GPUI git revision and Rust
   has no stable ABI, so every update would break them. If sandboxed third-party modules are
   wanted later, the route is WASM.

## Windows, popovers and motion

- **Bar windows.** `WindowKind::LayerShell` on the top layer, anchored top, left and right, with
  an exclusive zone when "Reserve space" is on. One per output, opened and closed on hotplug.
- **Popovers.** `WindowKind::AnchoredPopup`, an `xdg_popup` parented to the bar surface on the
  monitor that was clicked. The compositor grab gives click-outside-to-close. The surface has a
  fixed size with transparent margins for the shadow, and the input region excludes the margins.
  Content is built on open and kept for about 10 s after close so reopening is instant.
- **Springs.** A small `Spring { x, v, target, k, c }` advanced by real frame time. While unsettled
  it requests the next frame; once position and velocity are within epsilon it stops, so a static
  bar renders nothing. Retargeting keeps velocity, which is what makes motion interruptible.
  Motion is time-based, so it looks the same at 144 Hz (DP-1) and 75 Hz (HDMI-A-1).
  - Popover: stiffness 380, damping 28 (about 3.9% overshoot, at rest by about 490 ms).
  - Indicator (workspaces, switches, highlights): stiffness 520, damping 34.
  - Reduced motion: every spring becomes a 120 ms opacity fade.
- **Compositor animations off.** Hyprland's own layer and popup animations are disabled for the
  `yozora-*` namespaces so motion is not applied twice.
- **Assets.** Geist and Geist Mono are bundled (SIL OFL). Our icons are SVG assets on a 24-unit
  grid, drawn at 16 px with a 1.8 stroke. Tray icons resolve through the freedesktop icon theme.

## Config and theme

Files live in `~/.config/yozora/`:

| File | Owner | Holds |
|---|---|---|
| `theme.toml` | shared by every Yozora app | theme, accent, glass, motion |
| `bar.toml` | `yozora-bar` | bar style, height, reserve space, modules per zone, per-monitor sections; may override `[theme]` keys for the bar only |
| `<id>.toml` | each future app | its own settings |

Precedence: built-in defaults, then `theme.toml`, then the app's own file.

- Schema is versioned (`version = 1`). Unknown keys produce warnings with line numbers.
- An invalid file keeps the last good config running and shows a small error badge.
  `yozora-bar check` validates without starting.
- Settings edits with `toml_edit` so comments and formatting survive, and writes atomically
  (temp file, then rename). Apps watch the directory, not the file, so renames are seen.
- Monitors are keyed by connector name (`[monitor."DP-1"]`), with optional matching by monitor
  description because connector names can change.

Initial tokens (from the approved design):

| Token | Midnight | Daybreak |
|---|---|---|
| Ink | `#E8F1FF` | `#0F1626` |
| Ink 2 | `#98A5BE` | `#4A5672` |
| Ink 3 | `#7A869F` | `#5F6A84` |
| Popover tint | `rgba(14,19,31,.70)` | `rgba(248,250,253,.76)` |
| Bar tint | `rgba(10,14,23,.55)` | `rgba(246,248,252,.60)` |
| Hairline | `rgba(170,200,255,.17)` | `rgba(20,40,90,.14)` |

Accents (Midnight / Daybreak): Starlight `#8BBEFF` / `#2E6CD9`, Aurora `#8FE3CF` / `#0D7F69`,
Nebula `#BBA8FF` / `#6A4FDB`, Dawn `#FFC58B` / `#AD5A0D`. Every text pairing meets WCAG AA.
Radii: 8 bar items, 10 rows and controls, 12 floating bar, 16 popovers and windows.

## Control socket

Each app listens on `$XDG_RUNTIME_DIR/yozora/<id>.sock` (directory mode 0700). The protocol is
one JSON object per line, request and reply.

| Command | Effect |
|---|---|
| `ping` | Liveness, used by Settings |
| `toggle <popover>` | For Hyprland keybinds, e.g. `yozora-bar msg toggle control-center` |
| `reload` | Re-read config now |
| `stats` | Frame-time histogram, frame count, RSS |
| `focus` | Settings only: raise the existing window |

## Performance and measurement

Targets come from the brief. The dev machine has no 120 Hz display, so budgets are measured at
144 Hz (6.94 ms) on DP-1 and at 75 Hz on HDMI-A-1 at the same time.

- Frame times are the intervals between renders, measured inside `render`, because GPUI
  doesn't expose presentation timestamps. Each animation logs a summary when it settles, and
  `msg stats` returns the latest per monitor.
- `scripts/bench.sh` measures idle CPU, wakeups and memory (anonymous and total separately)
  for the bar and ironbar, then drives the workspace indicator and the popover on every
  monitor with `msg bench`. Results are in [benchmarks.md](benchmarks.md).
- Baseline is ironbar on the same machine.
- Only release builds count for performance claims.

## Testing

- **Unit tests** (`cargo test --workspace`) cover config loading and warnings, editing that
  keeps formatting, manifests and schemas, the control socket, service parsers, module
  placement, springs and custom-module restarts. None need a Wayland session.
- **Fakes.** `yozora-bar --fake` runs the whole UI on in-process backends, for development,
  screenshots and hardware the dev machine lacks (battery, Bluetooth).
- **Scripted UI checks.** `scripts/click.sh` and `scripts/drag.sh` drive the real windows through
  `yz-pointer`. Each refuses to act unless `hyprctl cursorpos` confirms the cursor is where it
  should be.
- CI (`.github/workflows/ci.yml`) checks formatting, runs clippy with warnings as errors, and
  runs the tests.
- **Not yet:** D-Bus integration tests against python-dbusmock.

## Where the design meets reality

1. **All blur is the compositor's.** A Wayland client cannot read the pixels behind it. Yozora
   paints tint, hairline, highlight and shadow; a Hyprland layer rule on the `yozora-`
   namespaces does the blur. The Settings switch is "Blur behind the bar", and when it is off
   the tint gets more opaque. The spike saw `blur_popups` blur the popover. A later check on a
   busy desktop was inconclusive, so treat popover blur as unconfirmed.
2. **Popover scale-in.** GPUI has no scale transform for an element subtree. Popovers fade in
   with an 8 px drop instead, and their rows rise with a stagger.
3. **Automatic theme** follows the portal's `color-scheme` instead of sunset. Sunset switching is
   left to tools like darkman, which set that value.
4. **Calendar events** need a source (Evolution Data Server, CalDAV or khal). Out of scope until
   one is chosen.
5. **Memory.** The bar's anonymous memory is within the 20 to 30 MB stretch goal (23 MB idle,
   about 30 MB after heavy use). Total RSS is about 110 MB, because the NVIDIA Vulkan driver
   maps around 55 MB of shared libraries. Anonymous and total RSS are reported separately.

## Phase 1 spike: go or no-go for GPUI

Pin a Zed `main` revision (crates.io `gpui` 0.2.2 predates layer-shell and popups), then check:

1. A layer-shell bar on both outputs, exclusive zone working, pointer input working.
2. `AnchoredPopup` from the layer surface: grab, dismissal, positioning near screen edges, and
   Hyprland blur on it.
3. Transparency, shadow margins and input region together.
4. A spring that stops requesting frames once settled, confirmed with a frame counter.
5. Which transforms GPUI supports for the popover animation.
6. Startup time, memory and frame times on the GTX 1650 Super (proprietary 615 driver).

If popups are the blocker, try popovers as overlay layer surfaces, then a minimal GPUI patch
sent upstream, before considering another renderer.

## What the build changed

- **Audio goes through `pactl`**, subscribing to its event stream, rather than a PulseAudio
  client library. It's one small, well-tested dependency the system already has.
- **Popovers grab only when opened by a click.** A popover opened with `msg toggle` doesn't
  grab, and closes when Hyprland's focused window changes. A grabbed popover is dismissed by
  the compositor, so it closes without the animation, and the dismissing click is consumed.
- **The popover opens with a fade and an 8 px drop, rows rising with a stagger.** GPUI has no
  transform for a whole element tree, so there is no scale-in.
- **Blur rules are added at runtime.** The bar sends the layer rules through `hyprctl eval` on
  start, so blur works without editing the Hyprland config. They are lost on a Hyprland reload,
  so [`packaging/hyprland/`](../packaging/hyprland/) ships them for the config too.
- **GPUI quits when the last window closes**, by default on Linux. The bar sets
  `QuitMode::Explicit`, because it must survive having no monitor to show on, and reopening
  its bars for a style change. Settings keeps the default, so closing its window quits it.
- **Every Yozora window sets `inactive_frame_interval: None`.** GPUI otherwise caps windows
  without keyboard focus at 30 fps, and a bar never has focus.
- **Settings polls app status every 2 s** over each app's socket, and rescans manifests every
  6 s as well as watching the manifest directories.
- **The dark mode tile in the control center lasts for the session.** It overrides the theme in
  memory. Settings → Appearance changes `theme.toml`.
- **Apps hide the GL stack when a Vulkan driver is installed.** GPUI asks wgpu for Vulkan and
  GL together, and asking for GL makes glvnd load every EGL vendor: NVIDIA's EGL plus Mesa's
  libLLVM and Gallium, about 40 MB of RSS that's never used. Each app sets
  `__EGL_VENDOR_LIBRARY_FILENAMES=/dev/null` before any thread starts and removes it again for
  the commands it runs (`yozora_ui::gpu`). If the first window still can't open, the app
  restarts itself with `YOZORA_GL=1`, which leaves GL alone. Tested by hiding the Vulkan
  driver: the bar restarts, but on the test machine GL alone can't drive the surface either,
  with or without this change, so the GL path itself is untested. A GPUI change that tries
  Vulkan first would do this more cleanly.
- **Raster images are shrunk to twice their drawn size** before GPUI sees them
  (`services::icons::fit`). GPUI keeps images decoded at full size, so a 512 px tray icon
  cost 1 MB. The copies are cached in `~/.cache/yozora/thumbs`.
- **Apps fix glibc's mmap threshold at 128 KiB** (`yozora_ui::malloc`). By default it rises to
  the size of each big block freed, so render buffers end up kept in the heap. Fixing it saves
  about 4 MB of anonymous memory, with no change in frame timing.

## Decisions

| Date | Decision |
|---|---|
| 2026-10-12 | GPUI from Zed `main` at a pinned git revision; bumped deliberately |
| 2026-10-12 | Audio through the PulseAudio protocol (pipewire-pulse), not native PipeWire |
| 2026-10-12 | Battery, Bluetooth, network and audio behind generic backend traits |
| 2026-10-12 | Default bar is minimal; modules are added through a generic registry |
| 2026-10-12 | Keep mako; the bar shows notifications and toggles Do Not Disturb |
| 2026-10-13 | License: MIT |
| 2026-10-13 | Settings is its own process; config files are the interface between apps |
| 2026-10-13 | Ecosystem apps are discovered through installed manifests |
| 2026-10-13 | No dynamic loading of Rust modules |
| 2026-10-13 | Theme, accent, glass and motion are shared across apps in `theme.toml` |
| 2026-10-13 | Phase 1 verdict: GPUI is a go ([spike report](spike-report.md)) |
| 2026-10-13 | Audio through `pactl subscribe` instead of a client library |
| 2026-10-13 | Popovers grab only when opened by a click |
| 2026-10-13 | Custom modules speak plain text or JSON lines with `text` |
