# CachyOS updates

A Yozora module that shows pending repository and AUR updates, and Arch news that needs
attention before you update. The bar item shows the total pending count. The popover lists
the news warnings, the repository updates and the AUR updates, with an Update button that opens
your terminal and runs `paru -Syu` there.

## Files

- `module.toml` is the manifest. It refreshes every 5 minutes and declares the `update` and
  `check` actions.
- `run.sh` runs `updates.py`. With no arguments it prints the status line that Yozora reads.
- `updates.py` reads the cache and prints one JSON line. With `--refresh` it gathers the data
  and rewrites the cache. `--selftest` runs its parser checks.
- `update.sh` opens a terminal running `paru -Syu`. It keeps the window open until Enter is
  pressed. It uses the first of `$TERMINAL`, ghostty, alacritty, kitty, foot, wezterm, konsole
  or xterm that is installed. With none, it sends a desktop notification.
- `test.sh` runs the offline checks. It uses a temporary cache and fake package tools, and it
  does not touch the network.

## Tools

- `checkupdates`, from pacman-contrib, for repository updates. Required for repository updates.
- `paru`, optional, for AUR updates (`paru -Qua`). Without it the AUR section is empty.
- `notify-send` is optional. It is used only to report that no terminal was found.

## Cache

The cache is `${XDG_CACHE_HOME:-~/.cache}/yozora/cachy-updates.json`. A refresh is started in
the background when the cache is missing or older than 15 minutes. A lock file,
`cachy-updates.lock` in the same directory, keeps only one refresh running. A lock older than
10 minutes is ignored. After the update terminal opens, the cache is deleted so the next status
run refreshes it.

News is read from https://archlinux.org/feeds/news/ and kept when it was published after the
last `starting full system upgrade` line in `/var/log/pacman.log`. If there is no such line, news
from the last 14 days is kept. When the feed cannot be fetched, the news list is empty.

## Environment overrides for testing

- `YOZORA_UPDATES_NO_REFRESH=1` stops `--status` from starting a background refresh.
- `YOZORA_UPDATES_NEWS_URL` replaces the news feed URL.
- `YOZORA_UPDATES_PACMAN_LOG` replaces the pacman log path.
