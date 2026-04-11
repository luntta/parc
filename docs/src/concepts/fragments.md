---
layout: layouts/doc.njk
title: Fragments
eyebrow: Concepts · §01
---

A **fragment** is the atomic unit in parc — a single Markdown file with YAML frontmatter, stored under `<vault>/fragments/`. Every fragment carries a common envelope (id, type, title, tags, links, timestamps) plus type-specific fields defined by its schema.

## Anatomy

```markdown
---
id: 01JQ7V3XKP5GQZ2N8R6T1WBMVH
type: todo
title: Upgrade auth library
tags: [security, backend]
links: [01JQ7V4Y]
status: open
priority: high
due: 2026-02-28
created_at: 2026-02-21T10:30:00Z
updated_at: 2026-02-21T10:30:00Z
---

The current JWT library has a known timing vulnerability.
See #cve-2026-1234 for details.

Related: [[01JQ7V4Y|auth service refactor]]
```

The frontmatter is a strict envelope; the body is freeform Markdown with two parc-specific extensions:

- **Inline tags** — `#hashtag` syntax in the body. Merged with frontmatter `tags:` at index time, case-insensitive.
- **Wiki-links** — `[[id-prefix]]` or `[[id-prefix|label]]`. Bidirectional at query time — link A → B and parc knows B ← A.

## Identifiers

Every fragment gets a [ULID](https://github.com/ulid/spec) — a 26-character, lexicographically sortable identifier. The ULID is the filename: `<vault>/fragments/01JQ7V3XKP5GQZ2N8R6T1WBMVH.md`.

You rarely need to type the full ID. Every command that takes an `<id>` accepts any unique prefix:

```bash
parc show 01JQ7V
parc edit 01JQ7
parc set 01JQ7V status done
```

If a prefix is ambiguous, parc lists the matches and asks you to be more specific.

## Lifecycle

A fragment goes through three states from parc's perspective:

1. **Active** — lives in `<vault>/fragments/`, indexed, searchable.
2. **Archived** — still in `fragments/`, indexed, but excluded from default listings. Use `parc archive <id>` to flag.
3. **Trashed** — moved to `<vault>/trash/`, removed from the index, recoverable until purged. Use `parc delete <id>` to soft-delete.

Every edit creates a snapshot in `<vault>/history/<id>/` automatically — no git required. See [History]({{ '/concepts/history/' | url }}).

## Files first

The Markdown files are the source of truth. The SQLite index in `<vault>/index.db` is fully derivable from them:

```bash
parc reindex
```

This means you can edit fragments by hand, sync the vault with `rsync` or `git`, or restore from backup without losing anything parc cares about.
