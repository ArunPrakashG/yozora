# Modules

A module is a manifest plus a data process. The manifest, `module.toml`, says what the module is
called, how often it refreshes and which actions it offers. The process prints one JSON object
per line describing what the bar item and its popover should show. Yozora draws everything.

Modules cannot set colours, sizes in pixels, fonts or radii. This is deliberate: the system is
opinionated, so every module looks like it belongs to the same bar. A module picks a component,
a semantic tone (`success`, `warning` and so on) and a size (`compact` or `regular`). Yozora
decides what those look like. Unknown fields are errors, so a typo or a styling attempt fails
loudly instead of being ignored.

## A module in five minutes

Here is a complete `module.toml`:

```toml
schema  = 1
name    = "claude-sessions"
title   = "Claude"
version = "0.1.0"
icon    = "sparkles"
refresh = { interval = 30 }   # seconds, at least 5; or { push = true } if the module pushes its own updates
exec    = "claude-sessions"   # the process Yozora starts

[[actions]]
id          = "stop"
label       = "Stop"
destructive = true
```

The process writes one line per update to standard output. This is a valid update, shown on
one line:

```json
{"bar":{"icon":"sparkles","text":"2","badge":2,"tone":"accent"},"popover":{"title":"Sessions","body":[{"type":"section","heading":"Running","rows":[{"type":"row","icon":"terminal","title":"api-server","subtitle":"Idle for 5 min","badge":{"text":"Idle","tone":"warning"},"action":"open"}]}]}}
```

Check a module before you run it with:

```bash
yozora module check PATH
```

`PATH` is the module directory containing `module.toml`. The check reads the manifest, rejects
unknown fields and out-of-range values, and reports every problem it finds with its field name.
Run it after every change to the manifest or the process output.

## Components

Every popover is a tree of components. A `section` holds rows, and rows hold a `badge` when they
need one. The other components stand alone in the popover body. Each entry below lists what the
component is for, a Do and a Don't, and the fields it takes.

### `section`

A titled group of rows. Use it to split a popover into named parts, such as "Running" and
"Recent".

- Do give each section a short heading that says what its rows have in common.
- Do keep a section to eight rows or fewer. Split longer lists into two sections.
- Don't put a `section` inside another `section`. Sections hold rows.

Fields: `heading` (optional), `rows` (required, a list of `row` nodes).

### `row`

One item in a list: an icon, a title, an optional subtitle, value or badge, and an optional
action that runs when the row is clicked.

- Do make the title the thing the user recognises, such as a project or session name.
- Do use `subtitle` for one line of context, and keep it under 60 characters.
- Don't use a row for a long paragraph. Use `note` for explanatory text.
- Don't set `tone` on every row. Most rows stay `neutral`.

Fields: `icon` (optional), `title` (required), `subtitle` (optional), `value` (optional),
`badge` (optional, see `badge`), `tone` (optional, default `neutral`), `size` (optional,
`compact` or `regular`), `action` (optional, the id of a declared action).

### `toggle`

An on or off switch that calls an action when it changes.

- Do use it for a setting the user can flip, such as "Notify on finish".
- Don't use it for something that needs a confirmation. Use a `buttons` node with a destructive
  action instead.

Fields: `title` (required), `on` (required, `true` or `false`), `action` (required).

### `slider`

A continuous value from 0 to 1, such as a volume level. It calls its action as the user drags.

- Do use it only for a value with a natural range.
- Don't send values outside `0..=1`. Scale the value in the process before sending it.

Fields: `title` (required), `value` (required, a number from 0 to 1), `action` (required).

### `progress`

A read-only bar showing how far along something is.

- Do use it for work in progress, such as a build or a download.
- Don't use it for a value the user sets. Use `slider` for that.

Fields: `title` (required), `subtitle` (optional), `value` (required, a number from 0 to 1).

### `buttons`

A row of buttons, each calling a declared action. Mark the main choice `primary` and any
destructive choice `destructive`.

- Do use at most one `primary` button per `buttons` node.
- Do mark any button that removes or stops something as `destructive`. Yozora then asks the user
  to confirm.
- Don't use a `buttons` node for a choice that is really a list. Use `row` nodes with actions.

Fields: `items` (required, a list of buttons). Each button takes `label` (required), `action`
(required), `primary` (optional, default `false`) and `destructive` (optional, default `false`).

### `empty`

The state shown when there is nothing to list. It replaces a section or the whole body.

- Do say what is missing in the title, such as "No sessions running".
- Do use `message` for one sentence on what the user can do next.
- Don't show an `empty` node when the process has failed. Failures are covered in
  [Errors](#errors).

Fields: `title` (required), `message` (optional).

### `note`

A short line of plain text in the body of the popover, for context that does not belong to a
row.

- Do keep it to one or two sentences.
- Don't use it for error messages. Yozora shows its own error state for those.

Fields: `text` (required).

### `badge`

A small tone-coloured label used inside a `row`. It is not a node of its own: it appears only
as the `badge` field of a row.

- Do use a badge for a short status, such as "Idle" or "Failed".
- Do use a single word or two. Badges are small.
- Don't put a number in a badge when the row's `value` already shows it.

Fields: `text` (required), `tone` (optional, default `neutral`).

## Tones

A tone says what a piece of information means. Yozora chooses the colour.

| Tone | Meaning |
|---|---|
| `neutral` | The default. Ordinary information with nothing to report. |
| `accent` | The thing to look at. Use it for the one item the user should notice first. |
| `success` | Done or healthy. |
| `warning` | Needs attention, but has not failed. |
| `danger` | Failed, or a destructive action the user is about to take. |

Tones colour only the icon and the badge. They never change a row's background, so a popover
with many tones still reads as one calm list.

## Limits

These are the limits the validator enforces. A module that breaks one is rejected by
`yozora module check`.

| Limit | Value |
|---|---|
| Rows per section | 8 |
| Title length (characters) | 40 |
| Minimum refresh interval (seconds) | 5 |

## Wording

- Use sentence case. Write "Running sessions", not "Running Sessions".
- Titles are nouns or short noun phrases. Write "Sessions", not "View your sessions".
- Do not end titles or labels with a full stop. Write "Stop session", not "Stop session.".
- Keep subtitles under 60 characters.
- Write numbers with units. Write "5 min", not "5" and not "five minutes".

Do: `Idle for 5 min`
Don't: `idle since 5`

Do: `Sessions`
Don't: `Your Sessions.`

## Actions and destructive operations

Every action a module uses must be declared in `module.toml` under `[[actions]]`, with an `id`
and a `label`. A button, row or toggle refers to an action by its `id`. An undeclared action
is an error.

An action marked `destructive = true` always gets Yozora's confirm step before it runs. The
user sees the label and confirms or cancels. A module cannot skip this step, and it cannot
mark a destructive button as non-destructive to avoid it. Use `destructive` for anything that
stops, deletes, overwrites or spends something the user cannot easily get back.

## Errors

Invalid output never breaks the bar. When the process prints a line that does not match the
schema, or breaks a limit, Yozora keeps the last good state on the bar and in the popover and
shows Yozora's standard error state. Fix the output and the next valid line replaces the error.

A module whose process exits is restarted automatically. Each restart waits longer than the
last, up to 30 seconds, so a module that crashes on start does not spin the CPU.
