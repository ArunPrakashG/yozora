# Claude sessions

Shows how many Claude desktop (Code tab) sessions are out of sync between your accounts,
lists each account with its session count and last activity, and copies the missing sessions
when you press **Sync now**.

## What it does

- The bar item shows a sync icon. When sessions are out of sync it shows a warning badge with
  the number of sessions still to copy.
- The popover lists each account with its session count and age. Accounts that are missing
  sessions get a warning badge ("N missing"); the rest show "synced".
- **Sync now** copies missing `local_*.json` session files between account folders. It never
  overwrites an existing file, never deletes anything, and skips scratch-workspace sessions,
  which are tied to one account. Transcripts are shared by the Claude app already; this only
  copies session metadata.
- After a sync, a desktop notification reports the result. Restart the Claude app to see new
  sessions.

## Files

- `module.toml`: the manifest. Refreshes every 30 seconds.
- `run.sh`: starts `sync.py --status`, which prints one update line for each refresh.
- `sync.py`: the status report (`--status`) and the sync (`--apply`). Python 3, standard library only.
- `test.sh`: tests the sync and status logic against a temporary fake `HOME`. It never touches
  real session folders.

## Configuration

Account names are optional. Create `~/.config/claude-session-sync/accounts.json` to label accounts:

```json
{"caac412b": "arunprakashgo+claude1"}
```

Each key is the start of an account folder id under `~/.config/Claude/claude-code-sessions/`.
Accounts without a label show the first eight characters of their folder id.

## Checking

```bash
yozora module check packaging/modules/claude-sessions
sh packaging/modules/claude-sessions/test.sh
```
