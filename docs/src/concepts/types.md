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
parc types               # list all registered types in the active vault
parc schema show todo    # print a type's schema
```
