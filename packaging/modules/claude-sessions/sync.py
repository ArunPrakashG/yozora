#!/usr/bin/env python3
"""Claude sessions module: report and share Claude desktop (Code tab) sessions.

Logic copied from ~/.local/bin/claude-session-sync so this module is self-contained.

Copies missing session metadata files (local_*.json) between every account/org
folder that already has sessions. Transcripts live in ~/.claude/projects and are
shared already. Never overwrites, never deletes. Scratch-folder sessions are
skipped (tied to one account).

Usage:
  sync.py --status   print one line of Update JSON for the bar and popover
  sync.py --apply    copy missing sessions, then send a desktop notification
"""
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path.home() / ".config/Claude/claude-code-sessions"
LABELS = Path.home() / ".config/claude-session-sync/accounts.json"
MAX_ROWS = 8
MAX_TITLE = 40


def load_labels():
    """Optional {"<account-uuid-prefix>": "name"} map. Missing or bad file means no labels."""
    try:
        data = json.loads(LABELS.read_text())
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def label_for(acct, labels):
    for prefix, name in labels.items():
        if acct.startswith(prefix):
            return str(name)[:MAX_TITLE]
    return acct[:8]


def org_dirs():
    return sorted(d for d in ROOT.glob("*/*") if d.is_dir())


def sync_dirs():
    """Account/org folders that hold at least one session file."""
    return [d for d in org_dirs() if any(d.glob("local_*.json"))]


def read_metas(d):
    metas = {}
    for f in d.glob("local_*.json"):
        try:
            meta = json.loads(f.read_text())
        except (OSError, ValueError):
            continue
        if isinstance(meta, dict):
            metas[f.name] = meta
    return metas


def is_scratch(meta):
    return "scratch-workspaces" in (meta.get("cwd") or "")


def ago(ms):
    """Age of a millisecond timestamp, like 13h, 1m or now."""
    s = max(0.0, time.time() - float(ms) / 1000)
    for unit, n in (("d", 86400), ("h", 3600), ("m", 60)):
        if s >= n:
            return f"{int(s // n)}{unit}"
    return "now"


def account_row(title, count, missing, last):
    if last:
        age = ago(last)
        active = "active just now" if age == "now" else f"active {age} ago"
        subtitle = f"{count} sessions · {active}"
    else:
        subtitle = f"{count} sessions · no recent activity"
    if missing:
        tone, badge = "warning", {"text": f"{missing} missing", "tone": "warning"}
    else:
        tone, badge = "success", {"text": "synced", "tone": "success"}
    return {"type": "row", "title": title[:MAX_TITLE], "subtitle": subtitle,
            "tone": tone, "badge": badge}


def status_update():
    labels = load_labels()
    dirs = sync_dirs()
    if len(dirs) < 2:
        return {
            "bar": {"icon": "sync", "tone": "neutral"},
            "popover": {"title": "Claude sessions", "body": [{
                "type": "empty",
                "title": "Nothing to sync",
                "message": "Sign in with a second account to share sessions",
            }]},
        }

    folders = {d: read_metas(d) for d in org_dirs()}
    shared = {n for m in folders.values() for n, v in m.items() if not is_scratch(v)}

    accounts = {}
    for d, metas in folders.items():
        if metas:
            accounts.setdefault(d.parent.name, {}).update(metas)

    rows, total = [], 0
    for acct, metas in accounts.items():
        missing = len(shared - metas.keys())
        total += missing
        last = max((m.get("lastActivityAt") or 0 for m in metas.values()), default=0)
        rows.append(account_row(label_for(acct, labels), len(metas), missing, last))

    if total:
        bar = {"icon": "sync", "badge": total, "tone": "warning"}
        caption = f"{total} out of sync"
        actions = [{"type": "buttons", "items": [
            {"label": "Sync now", "action": "sync", "primary": True}]}]
        note = [{"type": "note",
                 "text": "Restart the Claude app after syncing to see new sessions."}]
    else:
        bar = {"icon": "sync", "tone": "neutral"}
        caption = "all synced"
        actions = [{"type": "buttons", "items": [{"label": "Check again", "action": "refresh"}]}]
        note = []

    sections = []
    for i in range(0, len(rows), MAX_ROWS):
        section = {"type": "section", "rows": rows[i:i + MAX_ROWS]}
        if i == 0:
            section["heading"] = "Accounts"
        sections.append(section)

    return {
        "bar": bar,
        "popover": {"title": "Claude sessions", "caption": caption,
                    "body": sections + actions + note},
    }


def notify(title, body):
    try:
        subprocess.run(["notify-send", "-a", "Claude", title, body], check=False)
    except OSError:
        pass  # no notification daemon or notify-send: the sync still happened


def apply_sync():
    dirs = sync_dirs()
    if len(dirs) < 2:
        print(f"Need at least two account folders with sessions; found {len(dirs)}",
              file=sys.stderr)
        return 1

    copied = 0
    for src in dirs:
        for f in sorted(src.glob("local_*.json")):
            try:
                meta = json.loads(f.read_text())
            except (OSError, ValueError):
                continue
            if not isinstance(meta, dict) or is_scratch(meta):
                continue
            for dst in dirs:
                if dst == src or (dst / f.name).exists():
                    continue
                print("copied    ", meta.get("title"), "->", dst.parent.name[:8])
                shutil.copy2(f, dst / f.name)
                copied += 1

    summary = f"{copied} session(s) copied."
    if copied:
        summary += " Restart the app to see them."
        print(summary)
        notify("Sessions synced", summary)
    else:
        print(summary)
        notify("Sessions already in sync", "Nothing to copy.")
    return 0


def main(argv):
    if "--apply" in argv:
        return apply_sync()
    if "--status" in argv:
        print(json.dumps(status_update(), separators=(",", ":")))
        return 0
    print("usage: sync.py --status | --apply", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
