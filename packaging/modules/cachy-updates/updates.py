#!/usr/bin/env python3
"""CachyOS updates module for Yozora.

Modes:
  (no args) or --status  Print one line of Update JSON built from the cache. Never waits on
                         the network. Starts a detached --refresh when the cache is missing,
                         older than 15 minutes, and no refresh is already running.
  --refresh              Gather repo updates (checkupdates), AUR updates (paru -Qua) and Arch
                         news, then rewrite the cache atomically.
  --selftest             Run the parser and filter checks in this file.

Environment overrides (used by test.sh, normally unset):
  XDG_CACHE_HOME                 cache root; the cache is $XDG_CACHE_HOME/yozora/cachy-updates.json
  YOZORA_UPDATES_NO_REFRESH=1    never spawn a background refresh from --status
  YOZORA_UPDATES_NEWS_URL        feed URL to fetch instead of the Arch news feed
  YOZORA_UPDATES_PACMAN_LOG      pacman log to read instead of /var/log/pacman.log
"""

import datetime as dt
import email.utils
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.request
import xml.etree.ElementTree as ET

REFRESH_AFTER = 15 * 60  # seconds before the cache counts as stale
LOCK_STALE = 10 * 60  # seconds before a refresh lock counts as abandoned
NEWS_FALLBACK_DAYS = 14
NEWS_URL = "https://archlinux.org/feeds/news/"
PACMAN_LOG = "/var/log/pacman.log"
UPGRADE_MARKER = "starting full system upgrade"
SHOWN_ROWS = 5  # rows listed per section before an "and K more" row
TITLE_MAX = 40
SUBTITLE_MAX = 60
UPDATE_ICON = "sync"
NEWS_ICON = "warning"
TOOL_TIMEOUT = 120

UPDATE_RE = re.compile(r"^(\S+) (\S+) -> (\S+)")
UTC = dt.timezone.utc


# ---------------------------------------------------------------- paths and locks


def cache_dir():
    base = os.environ.get("XDG_CACHE_HOME") or os.path.join(os.path.expanduser("~"), ".cache")
    return os.path.join(base, "yozora")


def cache_path():
    return os.path.join(cache_dir(), "cachy-updates.json")


def lock_path():
    return os.path.join(cache_dir(), "cachy-updates.lock")


def file_age(path):
    """Seconds since the file was modified, or None when it does not exist."""
    try:
        return time.time() - os.stat(path).st_mtime
    except OSError:
        return None


def should_spawn(cache_age, lock_age):
    """Refresh when the cache is missing or stale, unless a refresh is already running."""
    stale = cache_age is None or cache_age > REFRESH_AFTER
    running = lock_age is not None and lock_age < LOCK_STALE
    return stale and not running


def acquire_lock():
    """Create the lock file atomically. A stale lock is taken over."""
    path = lock_path()
    for _ in range(2):
        try:
            fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o644)
        except FileExistsError:
            age = file_age(path)
            if age is not None and age < LOCK_STALE:
                return False
            try:
                os.unlink(path)
            except FileNotFoundError:
                pass
            continue
        with os.fdopen(fd, "w") as f:
            f.write(str(os.getpid()))
        return True
    return False


def release_lock():
    try:
        os.unlink(lock_path())
    except FileNotFoundError:
        pass


# ---------------------------------------------------------------- parsing


def parse_update_lines(text):
    """Parse `name old -> new` lines from checkupdates or paru -Qua."""
    out = []
    for line in text.splitlines():
        m = UPDATE_RE.match(line.strip())
        if m:
            out.append({"name": m.group(1), "old": m.group(2), "new": m.group(3)})
    return out


def collapse(text):
    return " ".join(str(text).split())


def as_utc(when):
    return when.replace(tzinfo=UTC) if when.tzinfo is None else when


def parse_feed(xml_text):
    """Return [{'title', 'published'}] from an RSS document. Bad items are skipped."""
    root = ET.fromstring(xml_text)
    items = []
    for item in root.iter("item"):
        title = collapse(item.findtext("title") or "")
        if not title:
            continue
        try:
            published = as_utc(email.utils.parsedate_to_datetime((item.findtext("pubDate") or "").strip()))
        except Exception:
            continue
        items.append({"title": title, "published": published})
    return items


def filter_news(items, since):
    """Keep the items published after `since`."""
    return [i for i in items if i["published"] > since]


def parse_log_time(stamp):
    """Parse a pacman log timestamp such as 2026-10-01T10:11:12+0200 or 2026-10-01 10:11."""
    if re.search(r"[+-]\d{4}$", stamp):
        stamp = stamp[:-2] + ":" + stamp[-2:]
    when = dt.datetime.fromisoformat(stamp)
    if when.tzinfo is None:
        when = when.astimezone()  # naive stamps are local time
    return when


def upgrade_cutoff(log_lines, now):
    """Time of the last 'starting full system upgrade' line, else 14 days before `now`."""
    for line in reversed(log_lines):
        if UPGRADE_MARKER not in line:
            continue
        m = re.match(r"\[([^\]]+)\]", line)
        if not m:
            continue
        try:
            return parse_log_time(m.group(1))
        except ValueError:
            continue
    return now - dt.timedelta(days=NEWS_FALLBACK_DAYS)


def trunc(text, limit):
    text = collapse(text)
    if len(text) <= limit:
        return text
    return text[: limit - 1].rstrip() + "…"


# ---------------------------------------------------------------- gathering


def run_tool(argv):
    """Returns (returncode or None, stdout, error text)."""
    try:
        p = subprocess.run(
            argv,
            capture_output=True,
            text=True,
            stdin=subprocess.DEVNULL,
            timeout=TOOL_TIMEOUT,
        )
        return p.returncode, p.stdout, p.stderr
    except FileNotFoundError:
        return None, "", "not installed"
    except subprocess.TimeoutExpired:
        return None, "", "timed out"


def gather_repo():
    """Returns (packages, error text or None). checkupdates exit 2 means nothing to update."""
    rc, out, err = run_tool(["checkupdates"])
    if rc in (0, 2):
        return parse_update_lines(out), None
    if rc is None and err == "not installed":
        return [], "checkupdates is not installed (pacman-contrib)"
    return [], "checkupdates failed: %s" % (err.strip() or rc or err)


def gather_aur():
    """Returns (packages, note). A missing paru or a failure means no AUR updates."""
    rc, out, err = run_tool(["paru", "-Qua"])
    if rc is None:
        return [], "paru: %s" % err
    if rc != 0:
        return [], ("paru: %s" % err.strip()) if err.strip() else None
    return parse_update_lines(out), None


def gather_news(now):
    """Returns the news items published after the last full upgrade. Network errors give none."""
    url = os.environ.get("YOZORA_UPDATES_NEWS_URL") or NEWS_URL
    log_path = os.environ.get("YOZORA_UPDATES_PACMAN_LOG") or PACMAN_LOG
    try:
        with open(log_path, encoding="utf-8", errors="replace") as f:
            log_lines = f.read().splitlines()
    except OSError:
        log_lines = []
    since = upgrade_cutoff(log_lines, now)
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "yozora-cachy-updates"})
        with urllib.request.urlopen(req, timeout=8) as resp:
            text = resp.read().decode("utf-8", errors="replace")
        items = filter_news(parse_feed(text), since)
    except Exception:
        return []
    return [{"title": i["title"], "published": i["published"].isoformat()} for i in items]


def write_cache(data):
    os.makedirs(cache_dir(), exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=cache_dir(), prefix=".cachy-updates-", suffix=".tmp")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump(data, f)
    os.replace(tmp, cache_path())


def refresh():
    os.makedirs(cache_dir(), exist_ok=True)
    if not acquire_lock():
        return 0
    try:
        now = dt.datetime.now(UTC)
        try:
            repo, repo_err = gather_repo()
            aur, aur_note = gather_aur()
            news = gather_news(now)
            data = {
                "checked_at": time.time(),
                "repo": repo,
                "aur": aur,
                "news": news,
                "error": repo_err,
                "aur_note": aur_note,
            }
        except Exception as e:  # keep the cache valid whatever goes wrong
            data = {"checked_at": time.time(), "repo": [], "aur": [], "news": [],
                    "error": "refresh failed: %s" % e, "aur_note": None}
        write_cache(data)
    finally:
        release_lock()
    return 0


def maybe_spawn_refresh(cache_age):
    if os.environ.get("YOZORA_UPDATES_NO_REFRESH") == "1":
        return
    if not should_spawn(cache_age, file_age(lock_path())):
        return
    try:
        subprocess.Popen(
            [sys.executable, os.path.abspath(__file__), "--refresh"],
            start_new_session=True,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except OSError:
        pass


# ---------------------------------------------------------------- Update JSON


def ago_text(seconds):
    s = max(0, int(seconds))
    if s < 60:
        return "just now"
    minutes = s // 60
    if minutes < 60:
        return "%dm ago" % minutes
    return "%dh ago" % (minutes // 60)


def checking_update():
    return {
        "bar": {"icon": UPDATE_ICON},
        "popover": {
            "title": "Updates",
            "body": [{"type": "empty", "title": "Checking for updates", "message": "This takes a moment"}],
        },
    }


def news_row(item):
    return {
        "type": "row",
        "icon": NEWS_ICON,
        "title": "Manual step before updating",
        "subtitle": trunc("Arch news: " + item["title"], SUBTITLE_MAX),
        "tone": "warning",
    }


def more_row(count):
    return {"type": "row", "title": "and %d more" % count}


def package_rows(packages):
    rows = []
    for p in packages[:SHOWN_ROWS]:
        row = {
            "type": "row",
            "icon": UPDATE_ICON,
            "title": trunc(p["name"], TITLE_MAX),
            "subtitle": trunc("%s → %s" % (p["old"], p["new"]), SUBTITLE_MAX),
        }
        if p["name"].startswith("linux"):
            row["badge"] = {"text": "kernel", "tone": "accent"}
            row["tone"] = "accent"
        rows.append(row)
    if len(packages) > SHOWN_ROWS:
        rows.append(more_row(len(packages) - SHOWN_ROWS))
    return rows


def section(heading, rows):
    return {"type": "section", "heading": heading, "rows": rows}


def build_update(data, now):
    """Build the Update object from a cache dict. `now` is an epoch float."""
    repo = data.get("repo") or []
    aur = data.get("aur") or []
    news = data.get("news") or []
    err = data.get("error")
    ago = ago_text(now - float(data["checked_at"]))
    total = len(repo) + len(aur)

    if total == 0:
        if err:
            return {
                "bar": {"icon": NEWS_ICON, "tone": "warning"},
                "popover": {
                    "title": "Updates",
                    "caption": "checked " + ago,
                    "body": [{"type": "empty", "title": "Can't check updates",
                              "message": trunc(err, SUBTITLE_MAX)}],
                },
            }
        return {
            "bar": {"icon": "check", "tone": "success"},
            "popover": {
                "title": "Updates",
                "caption": "checked " + ago,
                "body": [
                    {"type": "empty", "title": "Up to date",
                     "message": "No repository or AUR updates are pending."},
                    {"type": "buttons", "items": [{"label": "Check now", "action": "check"}]},
                ],
            },
        }

    body = []
    if news:
        rows = [news_row(n) for n in news[:SHOWN_ROWS]]
        if len(news) > SHOWN_ROWS:
            rows.append(more_row(len(news) - SHOWN_ROWS))
        body.append(section("Needs attention", rows))
    if repo:
        body.append(section("Repositories · %d" % len(repo), package_rows(repo)))
    if aur:
        body.append(section("AUR · %d" % len(aur), package_rows(aur)))
    body.append({"type": "buttons", "items": [
        {"label": "Check now", "action": "check"},
        {"label": "Update…", "action": "update", "primary": True},
    ]})
    body.append({"type": "note", "text": "Update opens your terminal and runs the update there."})
    return {
        "bar": {"icon": UPDATE_ICON, "badge": total, "tone": "warning" if news else "accent"},
        "popover": {"title": "Updates", "caption": "%d pending" % total, "body": body},
    }


def read_cache():
    try:
        with open(cache_path(), encoding="utf-8") as f:
            data = json.load(f)
        float(data["checked_at"])
        return data
    except (OSError, ValueError, KeyError, TypeError):
        return None


def status():
    data = read_cache()
    cache_age = None
    if data is not None:
        cache_age = max(0.0, time.time() - float(data["checked_at"]))
    maybe_spawn_refresh(cache_age)
    if data is None:
        update = checking_update()
    else:
        try:
            update = build_update(data, time.time())
        except Exception:
            update = checking_update()
    print(json.dumps(update, separators=(",", ":")))
    return 0


# ---------------------------------------------------------------- selftest


def selftest():
    checkupdates_out = (
        "linux-cachyos 7.2.8-1 -> 7.2.9-2\n"
        "mesa 26.1.2-1 -> 26.1.3-1\n"
        "warning: something odd\n"
        "\n"
        "firefox 141.0-1 -> 141.0.1-1\n"
    )
    repo = parse_update_lines(checkupdates_out)
    assert [p["name"] for p in repo] == ["linux-cachyos", "mesa", "firefox"], repo
    assert repo[0] == {"name": "linux-cachyos", "old": "7.2.8-1", "new": "7.2.9-2"}, repo[0]
    assert parse_update_lines("yozora-git r48-1 -> r50-1\n")[0]["new"] == "r50-1"
    assert parse_update_lines("") == []

    feed = """<?xml version="1.0"?><rss><channel>
      <item><title>Old news</title><pubDate>Mon, 01 Sep 2026 10:00:00 +0000</pubDate></item>
      <item><title>  nvidia-utils 580 drops
        Maxwell support </title><pubDate>Sat, 10 Oct 2026 08:00:00 GMT</pubDate></item>
      <item><title>Broken date</title><pubDate>not a date</pubDate></item>
    </channel></rss>"""
    items = parse_feed(feed)
    assert [i["title"] for i in items] == ["Old news", "nvidia-utils 580 drops Maxwell support"], items
    since = dt.datetime(2026, 9, 15, tzinfo=UTC)
    assert [i["title"] for i in filter_news(items, since)] == ["nvidia-utils 580 drops Maxwell support"]

    now = dt.datetime(2026, 10, 16, 12, 0, tzinfo=UTC)
    log = [
        "[2026-09-01T10:00:00+0000] [ALPM] starting full system upgrade",
        "[2026-09-20T09:00:00+0000] [ALPM] installed foo (1.0)",
        "[2026-09-25T09:00:00+0000] [ALPM] starting full system upgrade",
        "[2026-09-26T09:00:00+0000] [ALPM] installed bar (1.0)",
    ]
    cutoff = upgrade_cutoff(log, now)
    assert cutoff == dt.datetime(2026, 9, 25, 9, 0, tzinfo=UTC), cutoff
    fallback = upgrade_cutoff(["[2026-09-26T09:00:00+0000] [ALPM] installed bar"], now)
    assert fallback == now - dt.timedelta(days=14), fallback
    assert upgrade_cutoff(["[2026-09-25 09:00] [PACMAN] starting full system upgrade"], now).year == 2026

    assert trunc("short", 40) == "short"
    t = trunc("x" * 50, 40)
    assert len(t) == 40 and t.endswith("…"), t

    assert should_spawn(None, None) is True
    assert should_spawn(60, None) is False
    assert should_spawn(REFRESH_AFTER + 1, None) is True
    assert should_spawn(REFRESH_AFTER + 1, 30) is False
    assert should_spawn(None, LOCK_STALE + 1) is True

    empty = build_update({"checked_at": now.timestamp(), "repo": [], "aur": []}, now.timestamp())
    assert empty["bar"] == {"icon": "check", "tone": "success"}, empty
    assert empty["popover"]["body"][0]["title"] == "Up to date"

    print("selftest ok")
    return 0


def main(argv):
    if "--selftest" in argv:
        return selftest()
    if "--refresh" in argv:
        return refresh()
    return status()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
