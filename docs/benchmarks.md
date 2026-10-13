# Benchmarks

Measured 2026-10-13 with `scripts/bench.sh` on release builds, against ironbar running on the
same session at the same time.

The machine: Hyprland 0.56.2, a GTX 1650 Super on the proprietary NVIDIA driver (Vulkan). Two
monitors: DP-1 at 1920×1080 and 143.9 Hz, and HDMI-A-1 at 2560×1080 and 75 Hz, rotated. The
bar ran with `--fake=all`, so the module content was the same in every run. It showed the
launcher, workspaces, window title, tray, a custom module, media, network, sound, Bluetooth,
battery, notifications, clock and control center on both monitors.

The desktop wasn't idle: other apps, including several Claude Code sessions, kept the load
average between 3.7 and 7.4. Treat outliers in the frame timing with that in mind.

## Idle

30 s each, nothing touched.

| | Yozora Bar | ironbar |
|---|---|---|
| CPU | 0.20 to 0.27% | 0.23 to 0.67% |
| Wakeups | 253 to 261/s | 272 to 277/s |
| Anonymous RSS | 22.7 to 24.3 MB | 25.4 to 28.5 MB |
| Total RSS | 119 MB | 102 to 103 MB |

CPU and wakeups are ranges over three runs. Memory is from the latest run, after the memory
changes below. Nearly all wakeups in both bars come from NVIDIA's Vulkan driver threads, as the
spike found. The bar's own threads wake a few times a second.

The bar's anonymous memory, its own data for two bar windows with a full set of modules, is now
at or below ironbar's. Its total RSS is still higher, because of the Vulkan driver's shared
libraries. After heavy use (100 popover opens and 100 workspace sweeps) the anonymous memory
levels off at 29.7 MB, as glyph and popover caches fill.

Settings, open and left alone, after the same changes: 24.4 MB anonymous and 106 MB total RSS.
It polls app status every 2 s, which costs about 0.10% CPU. It only runs while its window is
open.

## Memory

Measured from `smaps_rollup` 12 s after start, the same configuration each time.

| Build | Total RSS | Anonymous | File-backed |
|---|---|---|---|
| Before, GL allowed (`YOZORA_GL=1`) | 146.5 to 150.7 MB | 32.9 to 38.1 MB | 102 to 104 MB |
| GL hidden | 110.8 to 111.2 MB | 27.6 to 28.6 MB | 76 MB |
| GL hidden, mmap threshold fixed | 106.1 to 106.5 MB | 23.7 MB | 76 MB |
| Also images shrunk to their drawn size (now) | 105.9 to 106.2 MB | 22.5 to 22.7 MB | 76 MB |

Three changes:

- **The GL stack isn't loaded.** GPUI asks wgpu for Vulkan and GL together. Asking for GL makes
  glvnd load every EGL vendor: NVIDIA's EGL and Mesa's libLLVM and Gallium. Vulkan wins, so all
  of it is unused. The apps now hide the EGL vendors when a Vulkan driver is installed, which
  saves about 40 MB of RSS and 10 MB of anonymous memory. Warm startup got faster too: 0.20 to
  0.33 s to the first frame, against 0.68 to 0.87 s with GL allowed, measured back to back.
- **glibc keeps large blocks out of the heap.** By default its mmap threshold rises to the size
  of each big block freed, so render buffers end up kept in the heap. Fixing it at 128 KiB
  saves about 4 MB, with no change in frame timing (see below).
- **Images are shrunk to the size they're drawn at.** GPUI keeps an image decoded at its full
  size, and uploads it to the GPU at that size. One tray app on the test machine ships only a
  512 px icon, which cost 1 MB for a 16 px icon. Tray and notification icons and album art
  larger than twice their drawn size are now scaled down once and cached in
  `~/.cache/yozora/thumbs`. On the test machine this saved 1 MB of Rust heap. With album art
  showing, it saves more: a 1000 px cover is 4 MB decoded.

What's left of the total RSS, largest first:

| Mapping | RSS | What it is |
|---|---|---|
| `libnvidia-gpucomp` | 28 MB | NVIDIA's shader compiler, loaded by the Vulkan driver |
| `yozora-bar` | 21 MB | The binary's own code |
| Heap | 17 MB | The bar's data |
| `libnvidia-glcore` | 13 MB | Needed by NVIDIA's Vulkan driver |
| `/dev/nvidiactl` | 12 MB | Driver mappings |
| `Inter.ttc` | 5 MB | The UI font, mapped from disk |

The NVIDIA libraries and the binary are shared, file-backed pages. The kernel can drop them
under memory pressure, and other processes using the same driver share them.

The heap holds about 20 MB of live data, with only about 1.5 MB free, so `malloc_trim` returns
nothing. About 8 MB of it is Rust and the rest is C, mostly the NVIDIA driver. Measured with a
counting allocator and by turning things off:

| Part | Anonymous memory |
|---|---|
| GPUI, wgpu and the driver, with only a clock | 20.0 MB |
| All 13 modules on top | 3.8 MB |
| The second monitor's bar | 1.1 MB |
| The system font list (2752 faces, loaded by GPUI) | 1.3 MB |

Tried and not worth it:

- Fewer malloc arenas: no measurable difference.
- One shared D-Bus connection per bus instead of one per service: four extra connections cost
  0.1 MB. Sharing would let one slow service stall the others, because a zbus connection stops
  reading when any of its queues is full.
- Fat LTO: 4% less code and 0.3 MB less RSS, for a full 8-minute link on every release build.
- `panic = "abort"`: would shrink the binary, but a panic in one service would then take down
  the whole bar instead of that service.

What's left needs changes inside GPUI: a Vulkan-only first attempt, a smaller worker pool (it
starts one thread per CPU, 12 here), and loading the font list lazily.

## Startup

Time from process start to the first frame on each monitor:

| | DP-1 | HDMI-A-1 |
|---|---|---|
| Warm (five runs) | 202 to 333 ms | 200 to 299 ms |
| First run of a new binary | 1.9 to 2.0 s | 1.9 to 2.1 s |

The warm figure includes connecting to every service and Vulkan setup. The first run after a
build is slow because nothing is in the page cache yet.

## Frame timing

`msg bench` animates on one monitor and records the interval between renders. `workspaces`
sweeps the indicator across all pills and back, 20 times. `popover` opens and closes the
control center 20 times. A frame is late when its interval is over 1.5× the refresh interval.

| Monitor | Animation | Run | Frames | Mean | p50 | p99 | Max | Late |
|---|---|---|---|---|---|---|---|---|
| DP-1, 143.9 Hz (6.95 ms) | workspaces | 1 | 3507 | 6.97 | 6.96 | 7.25 | 27.96 | 5 (0.14%) |
| | | 2 | 3501 | 6.98 | 6.96 | 8.23 | 34.81 | 8 (0.23%) |
| | popover | 1 | 2389 | 6.92 | 6.95 | 7.50 | 33.01 | 4 (0.17%) |
| | | 2 | 2380 | 6.93 | 6.96 | 8.34 | 15.06 | 10 (0.42%) |
| HDMI-A-1, 75 Hz (13.33 ms) | workspaces | 1 | 1827 | 13.32 | 13.33 | 13.73 | 14.96 | 0 |
| | | 2 | 1839 | 13.32 | 13.33 | 16.16 | 18.93 | 0 |
| | popover | 1 | 1277 | 12.97 | 13.32 | 14.08 | 23.72 | 1 (0.08%) |
| | | 2 | 1244 | 13.51 | 13.29 | 17.91 | 326.29 | 6 (0.48%) |

Runs 1 and 2 are from before the memory changes. Run 3, after them, with the load average
above 9:

| Monitor | Animation | Frames | Mean | p50 | p99 | Max | Late |
|---|---|---|---|---|---|---|---|
| DP-1 | workspaces | 3509 | 6.96 | 6.96 | 7.28 | 14.13 | 3 |
| | popover | 2400 | 6.88 | 6.95 | 7.55 | 8.30 | 0 |
| HDMI-A-1 | workspaces | 1839 | 13.33 | 13.33 | 14.19 | 16.96 | 0 |
| | popover | 1276 | 12.98 | 13.32 | 14.07 | 34.29 | 1 |

Times are in milliseconds. The median frame lands on the refresh interval on both monitors, so
the bar keeps up at 144 Hz, above the 120 Hz target. Late frames are under half a percent. The
one 326 ms frame, in run 2 on HDMI-A-1, came with the machine under load from other apps, and
no other run came close. It is still a stall someone would see, so watch for it.

Between animations the bar renders nothing, so idle cost doesn't depend on the refresh rate.

## Reproduce

```bash
cargo build --release -p yozora-bar
```

```bash
./target/release/yozora-bar --fake=all &
```

```bash
scripts/bench.sh 30 ironbar
```

The script prints the idle figures for both processes, then the frame timing for every monitor
the bar is on.
