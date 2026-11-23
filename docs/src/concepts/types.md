---
layout: layouts/doc.njk
title: Types
eyebrow: Concepts · §03
---

A **type** defines what fields a fragment carries. Every fragment has exactly one type. Five built-in types ship with parc; you can add your own with a YAML schema file.

## Built-in types

| Type | Alias | Purpose | Key fields |
|------|-------|---------|-----------|
| **note** | `n` | Captured thoughts, references, anything | — |
| **todo** | `t` | Actionable work | `status` (open / in-progress / done / cancelled), `priority` (low / medium / high / critical), `due`, `assignee` |
| **decision** | `d` | Architectural and design decisions | `status` (proposed / accepted / superseded / deprecated), `deciders` |
| **risk** | `r` | Things that could go wrong | `status` (identified / mitigating / accepted / resolved), `likelihood`, `impact`, `mitigation` |
| **idea** | `i` | Half-formed thoughts worth keeping | `status` (raw / exploring / promoted / parked / discarded) |

The aliases work everywhere a type name is expected: `parc n "..."` is `parc new note "..."`, and `parc list t` is `parc list todo`.

Each vault has its own copy of these definitions in `schemas/`, so a vault can change them: rename `in-progress` to `doing`, add a `blocked` status, drop `assignee`. The desktop app's **Vault settings** do this and move existing fragments to renamed values; by hand, edit the YAML (see [Custom types]({{ '/custom-types/' | url }})).

## What a status means

Status words differ per type, so each value of a `status` field says what it means in one of four **categories**:

| Category | Meaning | Built-in examples |
|----------|---------|-------------------|
| `open` | Waiting | open, proposed, identified, raw |
| `active` | Under way | in-progress, mitigating, exploring |
| `done` | Finished | done, accepted (decision), resolved, promoted |
| `closed` | Dropped | cancelled, superseded, deprecated, accepted (risk), parked, discarded |

Done and closed fragments are *settled*: they leave the open-work lists of `parc today`, `due`, `stale` and `review`, the TUI's matching tabs, the desktop's **Open** filter and its open-work counts. A status a schema doesn't categorize is open. Schemas written before categories existed are read with the built-in meanings of these words.

## Turning types on and off

A vault doesn't have to use every type. `parc init` asks which ones to turn on, and the desktop's **Vault settings** switch them later; both write `types:` in [`config.yml`]({{ '/configuration/' | url }}#types).

A type that is off keeps its schema and its fragments: they still open, list, search, validate and change. What it loses is new fragments: `parc new`, the type's alias, `parc promote` and every other way of creating or converting a fragment refuse it, and capture forms don't offer it. `parc types` shows each type as `on`, `off` or `default`, the type captures start as (`parc +`, the TUI's capture form and the desktop's captures).

## Common envelope

Every type, built-in or custom, carries the same common fields:

| Field | Type | Notes |
|-------|------|-------|
| `id` | ULID | Filename and primary key |
| `type` | string | Must match a registered schema |
| `title` | string | Required |
| `tags` | string[] | Merged with inline `#hashtags` from the body |
| `links` | ULID[] | Outgoing wiki-links |
| `created_at` | ISO 8601 | Set on creation |
| `updated_at` | ISO 8601 | Updated on every save |
| `created_by` | string? | Optional, from `config.yml#user` |

Type-specific fields go alongside these in the same frontmatter block.

## Custom types

Drop a YAML schema into `<vault>/schemas/` to register a new type. See [Custom types]({{ '/custom-types/' | url }}) for the schema format and a worked example.

## Listing registered types

```bash
parc types               # every type in the active vault: alias, on/off/default, fields
parc types --json        # the same, with each status's category
```
