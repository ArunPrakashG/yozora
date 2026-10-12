# Yozora architecture

Status: proposal, 2026-10-13. Nothing here is built yet. Items marked **(verify)** are
assumptions that the Phase 1 spike checks before we depend on them.

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
  yozora-bar/          bin, Phase 1
    src/main.rs          CLI: run, msg, check; logging; single instance
    src/app.rs           monitor manager: a BarWindow per output, follows hotplug
    src/bar/             zones, item chrome, hover and press states
    src/popover/         popover host: open, close, spring, lazy content
    src/modules/         clock, workspaces, window, media, network, sound, devices,
                         notifications, tray, control_center, launcher, custom
    src/services/        hyprland, pulse, networkmanager, upower, bluez, mpris, sni, mako, portal
    src/ipc.rs           control socket
  yozora-config/       schema, defaults, validation, file watching    (split out when Settings starts)
  yozora-ui/           theme tokens, Spring, glass surface, controls  (split out when Settings starts)
  yozora-settings/     bin, Phase 3
docs/
```

Platform adapters stay inside `yozora-bar` until a second app needs them, then move to
`yozora-platform`. The control-socket protocol moves to `yozora-ipc` at the same point.

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
| Audio | PulseAudio protocol via pipewire-pulse | Client library chosen in the spike |
| Network | NetworkManager over D-Bus (`zbus`) | Secured new networks may hand off to a secret agent at first **(verify)** |
| Power | UPower, power-profiles-daemon | Peripheral batteries come from UPower devices too |
| Bluetooth | BlueZ `ObjectManager` | Not testable on the dev machine: fake backend plus python-dbusmock |
| Media | MPRIS | Position is extrapolated locally and only ticks while a popover shows it |
| Tray | StatusNotifierItem host (plus watcher if none runs), DBusMenu | Menus render as our own popovers |
| Notifications | mako's D-Bus interface (the one `makoctl` uses) | Whether mako signals new notifications is **(verify)**; otherwise refresh on open |
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

- Frame times are recorded in a histogram, read through `msg stats`. Presentation timestamps
  are used if GPUI exposes them **(verify)**, otherwise frame-callback times.
- A benchmark script measures idle CPU and wakeups over 60 s, resident memory (anonymous and
  total separately), startup time, and frame times while it drives popover open/close and
  workspace switches through the socket and `hyprctl`.
- Baseline is ironbar on the same machine: about 31 MB anonymous, 130 MB total RSS, 0.2% CPU.
- Only release builds count for performance claims.

## Testing

- Service parsers: unit tests against recorded fixtures (Hyprland event lines, D-Bus property
  dumps, `upower --dump` output).
- `yozora-bar --fake`: the whole UI on scripted fake backends, for development, screenshots and
  hardware the dev machine lacks (battery, Bluetooth).
- D-Bus integration tests against python-dbusmock.
- CI: build, `clippy`, tests. None of these need a Wayland session.

## Where the design meets reality

1. **All blur is the compositor's.** A Wayland client cannot read the pixels behind it. Yozora
   paints tint, hairline, highlight and shadow; a Hyprland layer rule on the `yozora-bar`
   namespace does the blur. The Settings switch becomes "Blur behind the bar (Hyprland)", and
   when it is off the tint gets more opaque. Whether Hyprland 0.56 also blurs a layer surface's
   popups is **(verify)**. If not, popovers become their own overlay layer surfaces.
2. **Popover scale-in.** GPUI animates opacity and position; a scale transform for a whole
   element subtree is **(verify)**. Fallback: translate, fade and clip reveal, which reads almost
   the same at a 6% scale.
3. **Automatic theme** follows the portal's `color-scheme` instead of sunset. Sunset switching is
   left to tools like darkman, which set that value.
4. **Calendar events** need a source (Evolution Data Server, CalDAV or khal). Out of scope until
   one is chosen.
5. **Memory.** The 20 to 30 MB stretch goal is at risk because the Vulkan driver alone maps a lot
   on NVIDIA. Anonymous and total RSS are reported separately.

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
