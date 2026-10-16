#!/bin/sh
# Test sync.py against a temporary fake HOME. Never touches the real session folders.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() { echo "FAIL: $*" >&2; exit 1; }

home="$tmp/home"
root="$home/.config/Claude/claude-code-sessions"
acct_a="$root/aaaaaaaa-1111-4111-8111-111111111111/org-a"
acct_b="$root/bbbbbbbb-2222-4222-8222-222222222222/org-b"
mkdir -p "$acct_a" "$acct_b" "$tmp/bin"

# Fake notify-send that records its arguments instead of showing a notification.
cat > "$tmp/bin/notify-send" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> "$tmp/notify.log"
EOF
chmod +x "$tmp/bin/notify-send"

# Account A: one shared session, one scratch session, one only-in-A session.
printf '{"title":"shared one","cwd":"/work/one","lastActivityAt":%s}' "$(( $(date +%s) * 1000 ))" > "$acct_a/local_1.json"
printf '{"title":"scratch","cwd":"/home/u/scratch-workspaces/x"}' > "$acct_a/local_2.json"
printf '{"title":"only a","cwd":"/work/a","lastActivityAt":1000}' > "$acct_a/local_3.json"

# Account B: has its own local_1.json with different content (must never be overwritten),
# and one session only in B.
printf '{"title":"B copy of one","cwd":"/work/one"}' > "$acct_b/local_1.json"
printf '{"title":"only b","cwd":"/work/b"}' > "$acct_b/local_4.json"
cp "$acct_b/local_1.json" "$tmp/b_local_1.before"
cp "$acct_a/local_1.json" "$tmp/a_local_1.before"

export HOME="$home"
export PATH="$tmp/bin:$PATH"
py="$here/sync.py"

# Status before sync: one line of JSON, warning badge with 2 sessions to sync.
before="$(python3 "$py" --status)"
[ "$(printf '%s\n' "$before" | wc -l)" -eq 1 ] || fail "status must print exactly one line"
printf '%s' "$before" | python3 -c '
import json, sys
u = json.load(sys.stdin)
assert u["bar"] == {"icon": "sync", "badge": 2, "tone": "warning"}, u["bar"]
p = u["popover"]
assert p["caption"] == "2 out of sync", p["caption"]
rows = p["body"][0]["rows"]
assert len(rows) == 2, rows
assert rows[0]["badge"]["text"] == "1 missing", rows[0]
assert any(b["action"] == "sync" and b["primary"] for b in p["body"][1]["items"])
assert p["body"][2]["type"] == "note"
' || fail "status JSON before sync is wrong"

# Apply the sync.
python3 "$py" --apply > "$tmp/apply.out"

# Copies: B gets A's local_3.json, A gets B's local_4.json. Scratch session is never copied.
[ -f "$acct_b/local_3.json" ] || fail "local_3.json was not copied to account B"
[ -f "$acct_a/local_4.json" ] || fail "local_4.json was not copied to account A"
[ ! -e "$acct_b/local_2.json" ] || fail "scratch session local_2.json was copied to account B"
cmp -s "$acct_b/local_3.json" "$acct_a/local_3.json" || fail "copied local_3.json differs from source"
cmp -s "$acct_a/local_4.json" "$acct_b/local_4.json" || fail "copied local_4.json differs from source"

# Never overwrite: existing files keep their content.
cmp -s "$acct_b/local_1.json" "$tmp/b_local_1.before" || fail "B's local_1.json was overwritten"
cmp -s "$acct_a/local_1.json" "$tmp/a_local_1.before" || fail "A's local_1.json was overwritten"

# Never delete: the scratch file and the originals are still there.
[ -f "$acct_a/local_2.json" ] || fail "scratch session was deleted from account A"
[ -f "$acct_a/local_3.json" ] && [ -f "$acct_b/local_4.json" ] || fail "source session was deleted"

# Exactly the expected number of files now exist in each folder.
[ "$(ls "$acct_a" | wc -l)" -eq 4 ] || fail "account A should have 4 files"
[ "$(ls "$acct_b" | wc -l)" -eq 3 ] || fail "account B should have 3 files"

grep -q "2 session(s) copied" "$tmp/apply.out" || fail "apply summary is wrong"
grep -q "Sessions synced" "$tmp/notify.log" || fail "no 'Sessions synced' notification"

# Status after sync: all synced, no badge.
after="$(python3 "$py" --status)"
printf '%s' "$after" | python3 -c '
import json, sys
u = json.load(sys.stdin)
assert "badge" not in u["bar"], u["bar"]
assert u["bar"]["tone"] == "neutral", u["bar"]
p = u["popover"]
assert p["caption"] == "all synced", p["caption"]
assert p["body"][1]["items"][0]["action"] == "refresh"
assert all(r["badge"]["text"] == "synced" for r in p["body"][0]["rows"])
assert len(p["body"]) == 2
' || fail "status JSON after sync is wrong"

# Second apply has nothing to copy and says so.
python3 "$py" --apply > /dev/null
grep -q "Sessions already in sync" "$tmp/notify.log" || fail "second apply should report already in sync"

# With fewer than two account folders, status is a valid update with an empty node.
rm -rf "$acct_b"
single="$(python3 "$py" --status)"
printf '%s' "$single" | python3 -c '
import json, sys
u = json.load(sys.stdin)
assert u["popover"]["body"][0]["type"] == "empty", u
assert "badge" not in u["bar"]
' || fail "single-account status should be an empty node"

echo "claude-sessions tests passed"
