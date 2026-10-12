# Phase 1 spike: GPUI on Hyprland

**Verdict: GPUI works for Yozora Bar.** Everything the design needs either works as-is or has a
clear workaround. Two findings change the plan: popover dismissal (below) and the rotated-monitor
bug, which we should report upstream.

Tested 2026-10-13 on Hyprland 0.56.2 with a GTX 1650 Super on the proprietary 615 driver
(Vulkan). The monitors were DP-1 at 1920×1080 and 143.86 Hz, and HDMI-A-1 at 2560×1080 and
75 Hz, rotated 270°. GPUI is pinned to Zed rev `26363aab` and built with Rust 1.98.1, in a
release build. The spike ran alongside ironbar.

## What was built

The spike is in `crates/yozora-bar`:

- A layer-shell bar on every monitor. It has five workspace pills with a spring-driven
  indicator, and a clock that redraws only at minute boundaries.
- A control-center button that opens a spring-animated popover in its own `xdg_popup`.

Every animation logs its frame pacing when it settles (`yozora::frames`).

`crates/yz-pointer` and `scripts/click.sh` provide scripted clicks through
`zwlr_virtual_pointer_v1`. The script refuses to click unless `hyprctl cursorpos` confirms the
cursor is at the target.

## Results

| Check | Result |
|---|---|
| Layer shell, exclusive zone | ✅ Hyprland reserves 34 px on both monitors and stacks it under ironbar |
| One bar per monitor via `display_id` | ✅ |
| Transparent surface, blur from the compositor | ✅ needs a `hl.layer_rule`; see below |
| Pointer input on the bar | ✅ |
| `xdg_popup` with grab, opened on mouse-down | ✅ |
| Edge handling (`SLIDE_X`, `FLIP_X`) | ✅ the card slides left to stay on screen |
| Input region limited to the card | ✅ clicks on the shadow margin pass through to the window below |
| Compositor blur under the popup (`blur_popups`) | ✅ |
| Spring stops requesting frames once settled | ✅ the bar sits idle between clock ticks |
| Animation transforms | ⚠️ no subtree scale; the popover uses fade plus an 8 px drop |

### Frame pacing

Each row is one real animation, with the frame interval measured inside `render`.

| Animation | Monitor | Frames | p50 | p99 | Max | Late |
|---|---|---|---|---|---|---|
| Popover open | DP-1, 144 Hz | 99 | 6.93 ms | 7.32 ms | 7.52 ms | 0 |
| Popover open | HDMI-A-1, 75 Hz | 53 | 13.31 ms | 13.76 ms | 16.00 ms | 0 |
| Workspace indicator | DP-1, 144 Hz | 86 | 6.94 ms | 7.16 ms | 7.30 ms | 0 |

A frame counts as late when its interval exceeds 1.5× the refresh interval. One run of the
indicator on DP-1 had 3 late frames, with a 16.7 ms max, out of 85. That run overlapped another
monitor's popover opening on the same process. Phase 2 should keep measuring this.

### Resources, idle, against ironbar

| | Yozora Bar (spike) | ironbar |
|---|---|---|
| CPU, 20 s idle | 0.15% | 0.15% |
| Wakeups/s | ~247 | ~274 |
| Anonymous RSS | 35.6 MB | 28.7 MB |
| Total RSS | 159 MB | 110 MB |
| Startup to first frame | 0.25–0.30 s warm, 1.9 s first-ever run | not measured |

Almost all the wakeups in both processes come from NVIDIA's Vulkan driver threads
(`[vkps]_Update` at about 144/s and another at about 101/s). Our own threads wake under 5 times a
second. Most of the extra file-backed RSS is the Vulkan driver and GPUI's text stack, and it is
shared with other processes. Anonymous memory, the part that is really ours, is 7 MB above
ironbar while running two windows.

## Findings that change the plan

1. **GPUI caps unfocused windows at 30 fps by default.** `WindowOptions::inactive_frame_interval`
   defaults to 33.3 ms, and a bar never has keyboard focus. Every Yozora window must set it to
   `None`. This needs to live in a shared window-options helper, not be left to each surface.

2. **Display bounds ignore output rotation.** GPUI builds `PlatformDisplay::bounds` from the raw
   `wl_output` mode, so the rotated HDMI-A-1 reports 2560×1080 instead of 1080×2560. The bar now
   passes width 0, which tells layer shell to stretch between the anchors, so it is unaffected.
   Anything that sizes itself from display bounds will still be wrong. We should file this
   upstream, and xdg-output logical size is the right fix. Display bounds also carry no
   connector name, so mapping to Hyprland monitors still has to go through position.

3. **Popover dismissal belongs to the compositor while a grab is active.** A click anywhere
   outside the popup, including on our own bar, makes Hyprland send `popup_done`. That click is
   consumed, and GPUI closes the window at once. Two consequences:
   - There is no fade-out on dismissal. The spring close path only runs when we close the
     popover ourselves.
   - Clicking a different bar item while a popover is open takes two clicks.

   Phase 2 should try `grab: false`, dismissing on our own pointer events and on
   `wl_keyboard.leave`. If that dismisses reliably when another app is clicked, it gives the
   designed behaviour: animated close, and switching popovers in one click. If not, we keep the
   grab and accept the instant close.

4. **Blur needs a layer rule.** These match the existing noctalia rules:

   ```lua
   hl.layer_rule({ match = { namespace = "^yozora-" }, blur = true })
   hl.layer_rule({ match = { namespace = "^yozora-" }, blur_popups = true })
   hl.layer_rule({ match = { namespace = "^yozora-" }, ignore_alpha = 0.2 })
   hl.layer_rule({ match = { namespace = "^yozora-" }, no_anim = true })
   ```

   For the spike they were applied at runtime with `hyprctl eval`, and nothing on disk changed.
   The install docs should ship them, and Settings could offer to add them.

5. **Hyprland 0.56 config is Lua.** `hyprctl dispatch movecursor X Y` no longer works, so the
   test tooling moves the cursor itself through the virtual-pointer protocol.

## Smaller notes

- Text uses the system font. Geist still needs bundling.
- Reduced motion is wired up as a 120 ms linear fade, but was not tested with the setting on.
- GPUI logs "server-side decorations requested" once per surface. It's harmless, but noisy.

## Next

Phase 2 is the real bar, with services, the module registry and real Hyprland workspaces. Try
`grab: false` popovers first, because the answer shapes the popover API.
