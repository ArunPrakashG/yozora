#!/bin/sh
# Offline checks for sudo-toggle. Fake sudo, pkexec and notify-send; nothing real is touched.
set -eu
DIR=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
FAKE="$WORK/bin"; mkdir -p "$FAKE"
fail() { echo "FAIL: $*" >&2; exit 1; }

cat > "$FAKE/sudo" <<'EOF'
#!/bin/sh
[ -f "$FAKE_STATE" ] && echo "    (ALL) NOPASSWD: ALL"
exit 0
EOF
# pkexec runs the command directly (as the test user); visudo and install are faked.
cat > "$FAKE/pkexec" <<'EOF'
#!/bin/sh
exec "$@"
EOF
cat > "$FAKE/visudo" <<'EOF'
#!/bin/sh
exit 0
EOF
cat > "$FAKE/install" <<'EOF'
#!/bin/sh
# install -m 0440 -o root -g root SRC DST
for last; do :; done
cp "$5" "$last" 2>/dev/null || cp "$(eval echo \${$(($#-1))})" "$last"
EOF
cat > "$FAKE/notify-send" <<'EOF'
#!/bin/sh
exit 0
EOF
chmod +x "$FAKE"/*
export FAKE_STATE="$WORK/on" YOZORA_SUDOERS_FILE="$WORK/dropin"
export PATH="$FAKE:$PATH"

# off
out=$(sh "$DIR/run.sh")
python3 -I - "$out" <<'EOF'
import json, sys
u = json.loads(sys.argv[1])
assert u["bar"] == {"icon": "lock"}, u["bar"]
b = u["popover"]["body"][1]["items"][0]
assert b == {"label": "Enable", "action": "enable", "destructive": True}, b
EOF

# on
touch "$FAKE_STATE"
out=$(sh "$DIR/run.sh")
python3 -I - "$out" <<'EOF'
import json, sys
u = json.loads(sys.argv[1])
assert u["bar"] == {"icon": "lock-open", "tone": "warning"}, u["bar"]
assert u["popover"]["body"][1]["items"][0]["action"] == "disable"
EOF

# disable removes the drop-in; enable writes a NOPASSWD line
echo x > "$YOZORA_SUDOERS_FILE"
sh "$DIR/toggle.sh" disable
[ ! -e "$YOZORA_SUDOERS_FILE" ] || fail "disable left the drop-in"
sh "$DIR/toggle.sh" enable
grep -q 'NOPASSWD: ALL' "$YOZORA_SUDOERS_FILE" || fail "enable wrote no rule"
if sh "$DIR/toggle.sh" bogus 2>/dev/null; then fail "bad argument accepted"; fi

sh -n "$DIR/run.sh"; sh -n "$DIR/toggle.sh"
echo "sudo-toggle tests passed"
