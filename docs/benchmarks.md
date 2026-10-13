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
| CPU | 0.20 to 0.27% | 0.23 to 0.33% |
| Wakeups | 255 to 261/s | 272/s |
| Anonymous RSS | 37.2 to 37.4 MB | 25.5 to 28.5 MB |
| Total RSS | 165 to 167 MB | 102 to 103 MB |

Ranges are two runs. Nearly all wakeups in both bars come from NVIDIA's Vulkan driver threads,
as the spike found; the bar's own threads wake a few times a second.

Yozora's anonymous memory is about 9 to 12 MB above ironbar's, for two bar windows with a full
set of modules. Most of the extra total RSS is the Vulkan driver and the text stack, mapped from
shared libraries.

Settings, open on the Bar page and left alone: 0.10% CPU (its 2 s status poll), 52.9 MB
anonymous and 212 MB total RSS. It doesn't run unless it's open.

## Startup

Time from process start to the first frame on each monitor:

| | DP-1 | HDMI-A-1 |
|---|---|---|
| Warm (four runs) | 268 to 404 ms | 319 to 365 ms |
| First run of a new binary | 2.0 s | 2.1 s |

The warm figure includes connecting to every service and Vulkan setup (about 200 ms on this
GPU). The first run after a build is slow because nothing is in the page cache yet.

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
