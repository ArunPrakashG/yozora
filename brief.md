# Yozora — Project Vision, Architecture & Implementation Brief

## 1. Project Overview

**Project name:** Yozora  
**First application:** Yozora Bar  
**Category:** High-performance Linux desktop interface  
**Initial target:** CachyOS / Arch Linux running Hyprland  
**Primary language:** Rust  
**UI framework:** GPUI, subject to a technical proof of concept  
**Primary goal:** Build a beautiful, macOS-inspired Waybar alternative with exceptionally low idle overhead and smooth 120 FPS animations.

Yozora is intended to grow into a cohesive Linux desktop ecosystem. The initial release is only the top panel, but the architecture and branding should allow future applications to share a common design system, configuration format, IPC layer and system integration infrastructure.

### Product philosophy

- Performance is a first-class feature, not an afterthought.
- Visual polish should rival the quality of macOS interfaces.
- Animations should feel fluid, precise and intentional.
- Idle applications should consume minimal resources.
- Native Linux and Wayland integration takes precedence over cross-platform portability.
- Components should be modular and independently maintainable.
- Avoid unnecessary abstractions, dependencies and background services.
- Do not compromise desktop security for visual effects.

## 2. Brand Identity

### Name

**Yozora** (夜空), Japanese for “night sky.”

The brand represents a cohesive desktop environment inspired by the night sky, celestial geometry, light and darkness.

### Visual identity

Use a premium, dark, cosmic aesthetic:

- Midnight navy and graphite surfaces.
- Restrained ice-blue accents.
- Subtle translucency and carefully controlled glass effects.
- Crisp typography with excellent readability.
- Rounded surfaces with consistent radii.
- Fine borders, soft shadows and selective luminous edges.
- Geometric, minimal icons.
- Fluid transitions inspired by macOS, but with an original identity.

Suggested initial design tokens:

- Midnight: `#0A0E17`
- Graphite: `#252E40`
- Starlight blue: `#8BBEFF`
- Primary text: `#E8F1FF`
- Secondary text: `#98A5BE`

Treat these as initial values, not immutable design decisions. Support theme tokens and future customization.

### Design principles

1. Prefer restrained glass over excessive blur.
2. Use animation to communicate state changes, not as decoration.
3. Make every interaction feel deliberate.
4. Maintain visual consistency across the entire product family.
5. Keep the bar compact; reserve richer visuals for interactive popovers.
6. Support reduced motion and accessible contrast.

Brand tagline concept: **“Your desktop. A universe of its own.”**

## 3. Product Roadmap

The long-term ecosystem may include:

| Application | Purpose | Priority |
|---|---|---|
| Yozora Bar | Top panel and system indicators | Initial release |
| Yozora Launch | Application launcher and command palette | Later |
| Yozora Notify | Notifications, history and focus modes | Later |
| Yozora Studio | Wallpapers, themes and appearance | Later |
| Yozora Core | Shared configuration and desktop services | Gradual introduction |
| Yozora Lock | Dedicated lockscreen experience | Later, security-critical |

Do not implement these applications now. Design Yozora Bar so that reusable infrastructure can be extracted when multiple applications actually need it.

## 4. Initial Product Scope: Yozora Bar

Build a full-width, native Wayland top panel for Hyprland.

### Core modules

**Workspace and window context**
- Hyprland workspace indicators.
- Active workspace highlighting.
- Workspace switching.
- Active-window information where available.
- Optional workspace animations.

**System status**
- Clock and date.
- Network status.
- Audio volume and output device.
- Battery status.
- Bluetooth status where applicable.
- Media playback controls.
- System tray integration.

**Interactive popovers**
- Control Center.
- Audio controls and output selection.
- Network information and settings.
- Calendar.
- Media controls.
- Notification entry point, when a notification backend exists.

**Configuration**
- TOML configuration.
- Module visibility and ordering.
- Icon and spacing preferences.
- Theme selection.
- Animation settings.
- Per-monitor settings where supported.

Prioritize a working, polished core over implementing every module at once.

## 5. Technology Stack

### Primary stack

- Rust.
- GPUI for UI composition and rendering, pending integration validation.
- Cargo workspace when shared packages become useful.
- TOML with Serde for configuration.
- `tracing` and `tracing-subscriber` for structured logging.
- `zbus` for D-Bus communication.
- Hyprland IPC for workspace and compositor information.
- Wayland client protocols for surface management.
- PipeWire integration for audio.
- MPRIS over D-Bus for media players.

### Wayland integration

Investigate `wayland-client` and `smithay-client-toolkit` for protocol-level integration, including layer-shell support.

Do not assume GPUI's standard Linux window backend can create a correct layer-shell panel automatically.

**Mandatory first task:** establish a working technical proof of concept that combines GPUI rendering with a top-layer surface under Hyprland.

Verify:

- The panel is anchored to the top edge.
- It reserves the appropriate screen space through an exclusive zone.
- Pointer interaction works.
- Transparency and rendering work together.
- Multi-monitor and scaling behavior are understood.
- The application starts and recovers reliably.

If the integration is blocked by the existing backend, investigate a minimal backend extension before considering a completely different renderer.

Do not build a custom rendering engine prematurely.

## 6. Architecture

Keep the architecture simple, event-driven and testable.

### Logical layers

**Platform adapters**
- Hyprland IPC.
- Wayland surface integration.
- D-Bus services.
- PipeWire.
- MPRIS.
- Linux system interfaces such as `/sys` and `/proc`.

**State and event layer**
- Typed events.
- Centralized application state.
- State-change detection.
- Module-specific updates.
- Asynchronous operations where necessary.

**UI layer**
- GPUI components.
- Bar layout.
- Popovers.
- Theme tokens.
- Animation state.
- Input and keyboard interaction.

**Configuration**
- TOML parsing and validation.
- Defaults.
- Error reporting.
- Configuration reloads where practical.

### Suggested future package boundaries

Introduce these only when justified by real usage:

- `yozora-core`: shared types, events and lifecycle.
- `yozora-theme`: colors, typography, spacing and motion tokens.
- `yozora-ui`: reusable interface components.
- `yozora-config`: configuration parsing and validation.
- `yozora-ipc`: communication between ecosystem applications.
- `yozora-platform`: Linux and compositor integrations.

For the first release, a small, well-structured application is preferable to a large collection of empty crates.

### Event-driven behavior

- Prefer event subscriptions over frequent polling.
- Avoid shell subprocesses for routine status updates.
- Never perform blocking system operations on the UI thread.
- Update only components affected by state changes.
- Create expensive popover content lazily.
- Stop animation scheduling when animations finish.
- Keep the UI responsive when system services are unavailable.

## 7. Performance Requirements

Performance is a core differentiator.

### Animation target

Target 120 FPS on a 120 Hz display, where the hardware, compositor and rendering path permit it.

At 120 FPS, the frame budget is 8.33 ms.

Do not claim 120 FPS merely because the interface renders smoothly by eye. Instrument and measure frame pacing.

### Initial performance goals

These are targets to validate, not guarantees:

| Metric | Initial goal |
|---|---|
| Animation frame budget | 8.33 ms at 120 FPS |
| Frame pacing | At least 99% of frames within budget in the reference workload |
| Idle CPU | Near zero when the UI is static |
| Input responsiveness | Under 50 ms for ordinary interactions |
| Startup | Under 100 ms as a stretch goal |
| Memory | Approximately 20–30 MB as a stretch goal |

Record actual results and document the test environment.

### Performance rules

- No continuous redraw loop when idle.
- No unnecessary periodic full-tree rebuilds.
- No blocking I/O on the UI thread.
- Minimize heap allocations during animation.
- Avoid expensive blur and shadow effects over large surfaces.
- Prefer transforms and opacity for simple transitions when supported.
- Avoid unnecessary cross-process messages.
- Measure CPU, memory, wakeups and frame times independently.
- Test on actual Hyprland hardware, not only in isolated benchmarks.

Reference development hardware: Ryzen 5 3600 and GTX 1650 Super, running CachyOS and Hyprland.

## 8. Animation and Interaction Design

Aim for a premium, fluid interface without wasting resources.

### Animation behavior

- Spring-based movement for popovers and workspace indicators.
- Smooth opacity and translation transitions.
- Subtle hover and pressed states.
- Consistent animation timing across modules.
- No gratuitous movement in static indicators.
- Respect reduced-motion preferences.

Start with approximately 180–240 ms transitions, then tune spring stiffness and damping through real interaction testing.

Do not animate layout dimensions unnecessarily when a transform can achieve the same effect.

### Visual effects

Use blur, translucency and shadows selectively. Verify their actual cost on the target compositor and renderer.

A static glass surface should not require constant animation or expensive redraws.

## 9. Implementation Phases

### Phase 1 — Technical feasibility

Before building the complete application:

1. Inspect current GPUI Linux support and APIs.
2. Verify layer-shell integration under Hyprland.
3. Render a full-width test panel.
4. Verify exclusive-zone behavior.
5. Add one animated popover.
6. Measure idle CPU, memory and animation frame times.
7. Document blockers and decide whether GPUI is viable.

This phase is the primary technical risk.

### Phase 2 — Minimal functional bar

Implement:

- Full-width top panel.
- Workspace indicators.
- Clock.
- Volume.
- Network status.
- Battery status.
- Theme tokens.
- Basic configuration.
- Reliable startup and shutdown.

Get this stable before expanding.

### Phase 3 — Interactive system UI

Add:

- Control Center.
- Audio device selection.
- Media controls.
- Calendar.
- Network interactions.
- Hover states and polished transitions.

### Phase 4 — Performance and visual refinement

- Profile representative workloads.
- Measure frame pacing.
- Eliminate avoidable allocations and redraws.
- Tune blur and transparency.
- Verify behavior across monitors and scaling factors.
- Compare optimized release builds against the initial baseline.

### Phase 5 — Release readiness

- Configuration validation and helpful errors.
- Crash diagnostics.
- Clean autostart integration.
- Arch/CachyOS packaging.
- Documentation.
- Screenshots and demonstration video.
- Reproducible performance benchmarks.

### Phase 6 — Ecosystem expansion

Only after Yozora Bar is stable should development proceed to the launcher, notifications, wallpaper management and other applications.

## 10. Testing and Benchmarking

Create a repeatable benchmark suite.

Test these scenarios:

1. Static bar on an idle desktop.
2. Rapid workspace switching.
3. Repeated popover opening and closing.
4. Animated transitions at 120 Hz.
5. Multiple monitors with different scaling.
6. High-frequency system state changes.
7. Missing or restarting D-Bus services.
8. Application shutdown and restart.
9. Long-running sessions.
10. Reduced-motion mode.

Measure:
- Idle and active CPU usage.
- Resident memory.
- Startup time.
- Frame-time distribution.
- Dropped frames.
- UI input latency.
- Wakeups and unnecessary polling.

Use optimized release builds for performance claims.

## 11. Instructions for AI Coding Agents

Work incrementally and validate each stage.

**Start by inspecting the repository and environment.** Identify existing code, dependency versions, GPUI Linux capabilities and available Hyprland/Wayland protocols before proposing major changes.

Then:

1. Produce a concise feasibility report.
2. Implement the smallest end-to-end proof of concept.
3. Build and run it under Hyprland.
4. Measure its performance.
5. Document architectural decisions and known limitations.
6. Continue to the next phase only when the current one works.

Do not:
- Implement the entire ecosystem in one pass.
- Assume APIs exist without checking the installed versions.
- Replace GPUI without evidence that it is unsuitable.
- Introduce complex abstractions without a current use case.
- Claim performance targets are achieved without measurements.
- Stub system integrations and present them as functional.
- Spend time on branding and effects before validating panel integration.

Prefer small, compilable changes and real execution tests over large speculative implementations.

## 12. Definition of Success

The first release is successful when:

- Yozora Bar runs reliably under Hyprland.
- It behaves as a real desktop panel rather than a floating imitation.
- Core status modules work correctly.
- Popovers feel smooth and visually cohesive.
- Idle resource consumption is minimal and measured.
- Animation performance is benchmarked on a 120 Hz display.
- The codebase can accommodate future ecosystem components without unnecessary complexity.

The long-term objective is a cohesive, high-performance Linux desktop ecosystem. The immediate objective is a beautiful, dependable and genuinely fast Waybar alternative.

**Build the foundation first. Make it work, measure it, polish it, and expand only when the architecture earns that complexity.**
