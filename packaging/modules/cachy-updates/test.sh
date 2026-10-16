#!/bin/sh
# Offline checks for the cachy-updates module. Uses a temp cache, fake checkupdates and paru,
# and a local feed, so no network access and no real package tools are touched.
set -eu

DIR=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

export XDG_CACHE_HOME="$WORK/cache"
export YOZORA_UPDATES_NO_REFRESH=1
CACHE="$XDG_CACHE_HOME/yozora/cachy-updates.json"
mkdir -p "$XDG_CACHE_HOME/yozora"

fail() { echo "FAIL: $*" >&2; exit 1; }

# (a) pending updates with a news warning
python3 -I - "$CACHE" <<'EOF_A'
import json, sys, time
repo = [{"name": n, "old": "1.0-1", "new": "1.1-1"} for n in
        ["linux-cachyos", "mesa", "firefox", "vim", "git", "curl", "zlib"]]
aur = [{"name": "yozora-git", "old": "r48", "new": "r50"},
       {"name": "a-very-long-aur-package-name-that-goes-past-forty", "old": "1", "new": "2"},
       {"name": "tool", "old": "1", "new": "2"}]
data = {"checked_at": time.time(), "repo": repo, "aur": aur,
        "news": [{"title": "nvidia-utils 580 drops Maxwell support", "published": "2026-10-10T00:00:00+00:00"}],
        "error": None, "aur_note": None}
json.dump(data, open(sys.argv[1], "w"))
EOF_A
out_a=$(sh "$DIR/run.sh")
python3 -I - "$out_a" <<'EOF_CHECK_A'
import json, sys
line = sys.argv[1]
assert "\n" not in line
u = json.loads(line)
bar = u["bar"]
assert bar["badge"] == 10, bar
assert bar["tone"] == "warning", bar
assert bar["icon"] == "sync", bar
pop = u["popover"]
assert pop["title"] == "Updates" and pop["caption"] == "10 pending", pop
body = pop["body"]
assert body[0]["heading"] == "Needs attention", body[0]
news_row = body[0]["rows"][0]
assert news_row["title"] == "Manual step before updating", news_row
assert news_row["subtitle"] == "Arch news: nvidia-utils 580 drops Maxwell support", news_row
repo = body[1]
assert repo["heading"] == "Repositories · 7", repo["heading"]
assert len(repo["rows"]) == 6, len(repo["rows"])
assert repo["rows"][0]["badge"] == {"text": "kernel", "tone": "accent"}, repo["rows"][0]
assert repo["rows"][-1]["title"] == "and 2 more", repo["rows"][-1]
aur = body[2]
assert aur["heading"] == "AUR · 3", aur["heading"]
assert len(aur["rows"]) == 3
assert len(aur["rows"][1]["title"]) <= 40 and aur["rows"][1]["title"].endswith("…"), aur["rows"][1]
buttons = body[3]
assert buttons["type"] == "buttons"
assert [b["action"] for b in buttons["items"]] == ["check", "update"]
assert buttons["items"][1]["primary"] is True
assert body[4] == {"type": "note", "text": "Update opens your terminal and runs the update there."}, body[4]
print("check a ok")
EOF_CHECK_A

# (b) nothing pending
python3 -I - "$CACHE" <<'EOF_B'
import json, sys, time
json.dump({"checked_at": time.time() - 120, "repo": [], "aur": [], "news": [],
           "error": None, "aur_note": None}, open(sys.argv[1], "w"))
EOF_B
out_b=$(sh "$DIR/run.sh")
python3 -I - "$out_b" <<'EOF_CHECK_B'
import json, sys
u = json.loads(sys.argv[1])
assert "badge" not in u["bar"], u["bar"]
assert u["bar"] == {"icon": "check", "tone": "success"}, u["bar"]
pop = u["popover"]
assert pop["caption"] == "checked 2m ago", pop["caption"]
assert pop["body"][0] == {"type": "empty", "title": "Up to date",
                          "message": "No repository or AUR updates are pending."}, pop["body"][0]
assert [b["action"] for b in pop["body"][1]["items"]] == ["check"]
print("check b ok")
EOF_CHECK_B

# (c) missing cache: checking state, returns quickly, no refresh
rm -f "$CACHE"
start=$(date +%s)
out_c=$(timeout 5 sh "$DIR/run.sh") || fail "status hung or failed with no cache"
[ $(( $(date +%s) - start )) -lt 4 ] || fail "status took too long with no cache"
python3 -I - "$out_c" <<'EOF_CHECK_C'
import json, sys
u = json.loads(sys.argv[1])
assert u["bar"] == {"icon": "sync"}, u["bar"]
assert u["popover"]["title"] == "Updates"
assert u["popover"]["body"] == [{"type": "empty", "title": "Checking for updates",
                                 "message": "This takes a moment"}], u["popover"]["body"]
print("check c ok")
EOF_CHECK_C

# (d) refresh with fake checkupdates and paru, and a local news feed
FAKE="$WORK/bin"
mkdir -p "$FAKE"
cat > "$FAKE/checkupdates" <<'EOF_FAKE_CU'
#!/bin/sh
printf 'linux-cachyos 7.2.8-1 -> 7.2.9-2\nmesa 26.1.2-1 -> 26.1.3-1\n'
EOF_FAKE_CU
cat > "$FAKE/paru" <<'EOF_FAKE_PARU'
#!/bin/sh
printf 'yozora-git r48-1 -> r50-1\n'
EOF_FAKE_PARU
chmod +x "$FAKE/checkupdates" "$FAKE/paru"
cat > "$WORK/feed.xml" <<'EOF_FEED'
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0"><channel>
<item><title>Before the upgrade</title><pubDate>Mon, 01 Sep 2026 10:00:00 +0000</pubDate></item>
<item><title>nvidia-utils 580 drops Maxwell support</title><pubDate>Sat, 10 Oct 2026 08:00:00 GMT</pubDate></item>
</channel></rss>
EOF_FEED
cat > "$WORK/pacman.log" <<'EOF_LOG'
[2026-09-15T09:00:00+0000] [ALPM] starting full system upgrade
[2026-09-15T09:05:00+0000] [ALPM] upgraded foo (1.0 -> 1.1)
EOF_LOG
rm -f "$CACHE"
PATH="$FAKE:$PATH" \
YOZORA_UPDATES_NEWS_URL="file://$WORK/feed.xml" \
YOZORA_UPDATES_PACMAN_LOG="$WORK/pacman.log" \
    python3 -I "$DIR/updates.py" --refresh || fail "refresh failed"
[ -f "$CACHE" ] || fail "refresh did not write the cache"
[ ! -e "$XDG_CACHE_HOME/yozora/cachy-updates.lock" ] || fail "refresh left its lock behind"
python3 -I - "$CACHE" <<'EOF_CHECK_D'
import json, sys
d = json.load(open(sys.argv[1]))
assert [p["name"] for p in d["repo"]] == ["linux-cachyos", "mesa"], d["repo"]
assert [p["name"] for p in d["aur"]] == ["yozora-git"], d["aur"]
assert [n["title"] for n in d["news"]] == ["nvidia-utils 580 drops Maxwell support"], d["news"]
assert d["error"] is None, d["error"]
print("check d ok")
EOF_CHECK_D

# (e) a fresh lock blocks a second refresh from touching the cache
python3 -I - "$CACHE" <<'EOF_E'
import json, os, sys
open(os.path.join(os.path.dirname(sys.argv[1]), "cachy-updates.lock"), "w").write("1")
EOF_E
PATH="$FAKE:$PATH" YOZORA_UPDATES_NEWS_URL="file://$WORK/feed.xml" \
    python3 -I "$DIR/updates.py" --refresh
python3 -I - "$CACHE" <<'EOF_CHECK_E'
import json, os, sys
d = json.load(open(sys.argv[1]))
assert [p["name"] for p in d["repo"]] == ["linux-cachyos", "mesa"]
os.unlink(os.path.join(os.path.dirname(sys.argv[1]), "cachy-updates.lock"))
print("check e ok")
EOF_CHECK_E

# (f) the status output of the refreshed cache is still one valid line
out_f=$(sh "$DIR/run.sh")
python3 -I -c 'import json,sys; json.loads(sys.argv[1])' "$out_f"
case "$out_f" in *'"badge":3'*) ;; *) fail "expected badge 3 after refresh: $out_f" ;; esac

# update.sh and run.sh must at least parse
sh -n "$DIR/update.sh"
sh -n "$DIR/run.sh"
python3 -I "$DIR/updates.py" --selftest

echo "cachy-updates tests passed"
