---
layout: layouts/doc.njk
title: History commands
eyebrow: CLI · §06
---

parc keeps a per-fragment edit history under `<vault>/history/<fragment-id>/`. Snapshots are full files named by ISO timestamp, written automatically before every save.

## history

```bash
parc history <id>
             [--show <timestamp>]
             [--diff [<timestamp>]]
             [--restore <timestamp>]
             [--limit <n>]
```

Without flags, lists every snapshot for the fragment, newest first:

```bash
parc history 01JQ7V
```

```text
2026-02-23T09:04:41Z    (current)
2026-02-22T14:12:09Z    edited title and body
2026-02-21T10:30:00Z    created
```

The "(current)" row is the live file in `fragments/`, not a snapshot — but it lines up with the rest of the timeline for easy reference.

## Showing a snapshot

```bash
parc history 01JQ7V --show 2026-02-21T10:30:00Z
```

Renders that snapshot the same way `parc show` renders a current fragment. The frontmatter shown is whatever the snapshot contained, not what's live.

## Diffing

```bash
parc history 01JQ7V --diff
```

Diffs the current version against the most recent snapshot. Pass a timestamp to diff against an older one:

```bash
parc history 01JQ7V --diff 2026-02-21T10:30:00Z
```

Diffs use [`similar`](https://crates.io/crates/similar) and render with hunk headers, line numbers, and colour in the terminal.

`--json` returns a structured diff as an array of hunks with `op` (`equal` / `insert` / `delete`) and `text` fields, suitable for piping into other tools.

## Restoring

```bash
parc history 01JQ7V --restore 2026-02-21T10:30:00Z
```

Overwrites the current `fragments/<id>.md` with the snapshot's contents — but first creates a fresh snapshot of the current state, so a restore is itself reversible. The `updated_at` field is bumped to now; the rest of the frontmatter comes from the restored snapshot.

If the restore would invalidate the fragment against the type's schema (e.g. the schema gained a required field since the snapshot), parc errors out and leaves the live file untouched.

## Pruning history

History snapshots are full files, not deltas. For large or noisy fragments you can prune by hand:

```bash
rm .parc/history/01JQ7V3XKP5GQZ2N8R6T1WBMVH/2026-01-*.md
```

The index will catch up at the next `parc reindex` or the next edit to that fragment.
