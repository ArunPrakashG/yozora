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
| `yozora-notify` | GPUI app and D-Bus server (`org.freedesktop.Notifications`); one layer-shell window for the toasts, only while there are any | Session-long, autostarted |
| `yozora-lock` | GPUI app, one `ext-session-lock-v1` surface per monitor | While locked; started by hypridle |
| `yozora-launch` | GPUI app, one layer-shell window per monitor, kept mapped and empty while closed | Session-long, autostarted |
| Future: Studio, … | One binary each | Their own |

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
  yozora-notify/       bin
    src/server.rs        the freedesktop interface, the bar's dev.yozora.Notify1, commands
    src/jev.rs           optional answers from Jev over curl, off the main thread: urgency, privacy, kind, action
    src/store.rs         notifications, history on disk, Do Not Disturb
    src/toasts.rs        the toast window: stack, springs, timers, inline reply
    src/hypr.rs          which monitor toasts open on; the blur rule
  yozora-lock/         bin
    src/main.rs          run, single instance, the abort-on-panic hook, a lock surface per monitor
    src/view.rs          the lock screen: clock, password field, media, power; unlocking
    src/auth.rs          the password buffer, tries and waits, the hint under the field
    src/pam.rs           a small libpam binding: authenticate, then refresh credentials
    src/system.rs        the user, battery, network, keyboard layout, each monitor's wallpaper
    src/background.rs    the blurred wallpaper, made once and cached
    src/services.rs      MPRIS, the notification count, logind power and LockedHint
  yozora-launch/       bin
    src/main.rs          run, the client commands, the socket, watching .desktop folders
    src/app.rs           the search state shared by every monitor; a window per monitor
    src/view.rs          the launcher: field, sections, actions, keys, open and close
    src/search.rs        modes, rows and ranking across every source
    src/matcher.rs       nucleo, plus prefix and initials bonuses and highlighting
    src/desktop.rs       .desktop parsing and the Exec field codes
    src/sources.rs       windows, commands, cliphist, emoji, recent files and plocate
    src/calc.rs          the calculator and unit conversion
    src/money.rs         currency conversion with the ECB's cached daily rates
    src/clock.rs         times in other zones, through chrono-tz
    src/scripts.rs       Raycast and Vicinae script commands: headers, arguments, running
    src/projects.rs      git repositories and the folders Zed and VS Code opened recently
    src/ssh.rs           hosts from ~/.ssh/config, following Include
    src/browser.rs       bookmarks and history from Chromium-based browsers and Firefox
    src/usage.rs         frecency, the hour-of-day boost, pins
    src/spawn.rs         launching into a systemd scope, terminals
  yozora-settings/     bin
    src/app.rs           window, file and manifest watching, status polling, portal theme
    src/apps.rs          discovered apps and whether they're running
    src/doc.rs           one config file being edited
    src/view.rs          sidebar, header, page, TOML preview, footer
    src/pages/           appearance, motion, bar, modules, monitors, about, and generic
  yozora-cli/          bin `yozora`
    src/main.rs          commands, help, and the socket client
    src/notify.rs        notifications through yozora-notify's socket; sending over D-Bus
    src/desktop.rs       the bar's state and controls
    src/config.rs        settings through each app's schema, secrets masked
    src/apps.rs          status, apps, launcher, Settings and lock
    src/agent.rs         the agent skill: written once, linked into each harness
  yozora-config/       bar.toml, notify.toml, lock.toml, launch.toml and theme.toml types, loading with warnings, editing, watching,
                       manifests and settings schemas
  yozora-ipc/          the one-line JSON control socket
  yozora-ui/           theme tokens, icons, icon theme lookup, springs, frame timing, controls, text input,
                       Hyprland's socket and monitors
  yz-pointer/          virtual-pointer clicks and drags for scripted UI tests
packaging/             manifests, PKGBUILD, systemd units, Hyprland snippets, desktop files, the agent skill
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
| Notifications | yozora-notify's `dev.yozora.Notify1` (history, Do Not Disturb, actions, a `Changed` signal); otherwise mako's D-Bus interface (the one `makoctl` uses) | The bar checks which server owns `org.freedesktop.Notifications` and switches when it changes. mako can't clear history, so with mako cleared entries are hidden by the bar and remembered in `$XDG_STATE_HOME/yozora/` |
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
- **OSD.** A `WindowKind::LayerShell` surface on the overlay layer, anchored to the bottom (or
  top) of the focused monitor, with an empty input region so it never takes a click. It lives
  in the bar process: the bar already runs the audio and Hyprland services, so the OSD observes
  their snapshots and shows what changed, from any source, with no process or pipe of its own.
  The window opens on the first change (about 20 ms in a release build, since GPUI's GPU context
  is already up) and closes after its leave animation, so an idle OSD has no surface. Brightness
  has no kernel change events, so `yozora-bar osd brightness` changes it through logind's
  `SetBrightness` and shows it in one step. The pactl snapshot behind every audio change runs
  its three queries in parallel, which brings a key press to the capsule's first frame to about
  45 ms.
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
| `notify.toml` | `yozora-notify` | toast timeout, how many at once, monitor, history limit, Jev sorting |
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
| `state [section…]` | Bar: the services' snapshots as JSON, starting any the bar doesn't run yet |
| `media`, `volume`, `profile` | Bar: playback, volume and the power profile, answering with the result |
| `list`, `get`, `dismiss`, `pin`, `snooze`, `action`, `dnd`, `status`, `clear` | Notify: the center, as the bar's D-Bus interface has it |

The `yozora` command is the client for all of these. It is what scripts and AI agents use, through
the skill in `packaging/agent/yozora/SKILL.md`.

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

## The lock screen

`yozora-lock` holds the session through `ext-session-lock-v1`. The compositor shows only lock
surfaces until the locker says to unlock, and if the locker dies without saying so the session
stays locked. On Hyprland that means a red screen until another locker takes over, which
`misc:allow_session_lock_restore` permits.

Upstream GPUI has layer-shell windows but no session lock, so Yozora builds against a fork:
[ArunPrakashG/zed, branch `yozora`](https://github.com/ArunPrakashG/zed/tree/yozora). It is Zed
`main` plus [zed#65055](https://github.com/zed-industries/zed/pull/65055) (lock surfaces, no
commit before the first buffer, closing the windows on `finished`), one commit exporting the namespace, and one letting a
layer-shell window change its layer and keyboard interactivity after it opens, for the launcher. A layer-shell window opened with `gpui::layer_shell::SESSION_LOCK_NAMESPACE` becomes a
lock surface on the window's display. The first one locks the session and closing the last one
unlocks it. When the PR lands upstream, the workspace goes back to Zed `main`.

That API has sharp edges, and the lock is written around them:

- **Dropping windows unlocks.** A panic that unwound would close the windows, so the lock
  installs a hook that aborts instead. Only an accepted password closes them.
- **The compositor can end the lock** (`finished`, for example when another locker already
  holds it). GPUI closes the windows then; the lock sees none left and exits with an error.
- **A new monitor needs its own surface.** The lock watches GPUI's displays and opens one on
  each new display.

PAM runs on a background thread. The password is kept in a `Zeroizing` buffer, copied once
into PAM's reply, and that copy is wiped before it's freed. Tests use `pam_start_confdir` with
`pam_permit` and `pam_deny` services in a temporary directory, never the real stack, because
`pam_faillock` would lock the account after three wrong tries.

## The launcher

The launcher is opened all the time, so opening it must cost nothing. `yozora-launch` runs for
the whole session and keeps a layer-shell window mapped on every monitor, on the background
layer, with no keyboard focus, an empty input region and nothing drawn. Opening one moves it to
the overlay layer, takes the keyboard exclusively and draws; closing does the reverse. There is
no surface to create, no buffer to allocate and no font to load on the way, so the first frame is
ready about 40 µs after the request (0.2 ms for the whole command, socket included). The
compositor shows it on the next vblank.

The fork exposes the two calls this needs, `Window::set_layer` and
`Window::set_keyboard_interactivity`, on top of the session-lock work. Upstream GPUI fixes both
when the window opens.

Everything searchable is in memory before the launcher opens: the app index (about 60 ms for
87 apps, icons resolved and sized on a background thread), the windows, the commands and the
recent files. Opening refreshes windows and recent files in the background. A search over all
of it takes about 0.1 ms, so every keystroke searches again from scratch. Only `plocate` runs
outside the process, 90 ms after typing stops, and its results join when they arrive.

Scripts and SSH hosts are re-read on every open; they're a few small files. Projects (a walk of
`project_roots`, four levels deep and capped at 40,000 folders, about 5 ms on the dev machine),
browser pages (about 40 ms for 2,000 pages) and currency rates load in the background at start
and again at most every ten minutes. Other apps' SQLite databases are opened read-only with
`immutable=1`, so the launcher never waits on a browser's or an editor's lock. A currency or time
answer never waits on the network: the ECB rates are fetched by `curl` into a cache and used
from there.

Ranking is nucleo's fuzzy score plus bonuses: the name starting with the query, the query
spelling the initials (`vsc`), a later word starting with it. Generic names, keywords and
descriptions only match when every typed word starts one of their words, so letters scattered
through a sentence don't surface an app. Use adds `25·ln(1 + frecency)`, where frecency halves
each week and rises up to 1.6 times for apps usually opened at this hour, and a pin adds 60.
Windows, commands and files join the app results only when they match strongly.

Closing a window goes through Hyprland's focus. Hyprland's `window.close` dispatcher ignores
arguments it doesn't know and closes the focused window, so the launcher focuses the target,
checks that Hyprland reports it as active, and only then closes.

## Decisions

| Date | Decision |
|---|---|
| 2026-10-12 | GPUI from Zed `main` at a pinned git revision; bumped deliberately (since 2026-10-13, the fork's) |
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
| 2026-10-13 | Our own notification server, yozora-notify, replaces mako; the bar still works with mako |
| 2026-10-14 | Jev sorting in yozora-notify is opt-in and never delays a toast: the answer only shortens a toast or lets one through Do Not Disturb. Not used in the launcher, where it would be slower than search and would send every query away |
| 2026-10-14 | Jev's privacy answer hides toasts only while Hyprland reports screen sharing, and a toast stays hidden until Jev answers; the habits sent to Jev are counts per app, never content. Clipboard history and window titles are never sent |
| 2026-10-13 | Audio through `pactl subscribe` instead of a client library |
| 2026-10-13 | Popovers grab only when opened by a click |
| 2026-10-13 | Custom modules speak plain text or JSON lines with `text` |
| 2026-10-13 | Our own lock screen on a GPUI fork with `ext-session-lock-v1`, until upstream has it |
| 2026-10-13 | The lock checks passwords through PAM's `yozora-lock` service, falling back to `login` |
| 2026-10-14 | The launcher keeps a window mapped per monitor and switches its layer, rather than opening one each time |
| 2026-10-14 | Launched apps run in their own systemd scope, not as the launcher's children |
| 2026-10-14 | Script commands use the Raycast/Vicinae header format, so existing scripts work unchanged |
| 2026-10-14 | Browser history and editor recents are read in place with SQLite `immutable=1`, not copied |
| 2026-10-14 | One `yozora` command for scripts and AI agents, over the existing control sockets, not a second API. Its skill is linked into each harness's skills folder at login by a user service, only for harnesses that are installed, and dismissals it makes don't count toward Jev's habits |
