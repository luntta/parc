---
layout: layouts/doc.njk
title: History
eyebrow: Concepts · §06
---

Every edit creates a snapshot. No git required, no extra commands to run — saving a fragment automatically writes the previous version to `<vault>/history/<fragment-id>/`.

## How snapshots are stored

Each snapshot is a full copy of the Markdown file, named by its `updated_at` timestamp:

```
.parc/history/01JQ7V3XKP5GQZ2N8R6T1WBMVH/
├── 2026-02-21T10-30-00Z.md
├── 2026-02-22T14-12-09Z.md
└── 2026-02-23T09-04-41Z.md
```

Snapshots are written **before** the new content lands, so the latest live file in `fragments/` is always the current version and `history/` contains everything that came before.

## Inspecting history

```bash
# List every version
parc history 01JQ7V

# Show a specific version
parc history 01JQ7V --show 2026-02-22T14:12:09Z

# Diff against the previous version
parc history 01JQ7V --diff

# Diff against a specific older version
parc history 01JQ7V --diff 2026-02-21T10:30:00Z
```

Diffs use [`similar`](https://crates.io/crates/similar) and render with hunk headers + colour in the terminal.

## Restoring

```bash
parc history 01JQ7V --restore 2026-02-21T10:30:00Z
```

Restoring is itself an edit — it creates a new snapshot of the *current* state before overwriting it, so you can always undo a restore.

## Storage cost

Snapshots are full files, not deltas. For text fragments this is fine — a few KB per save. If you edit very large fragments often you can prune by hand: `rm` files from `history/<id>/` and the index will catch up at the next `parc reindex`.

## Why not just use git?

You can use git too — local vaults are designed to live alongside source code. But git's atomic unit is the commit, not the file, so a single edit to one fragment forces you to think about commit messages and staging. parc's history system is a per-file, zero-effort version chain that doesn't compete with git for that role.

Use both: parc's history for fine-grained per-fragment edit tracking, git for project-level snapshots that group fragment edits with code changes.
