---
layout: layouts/doc.njk
title: Custom types
eyebrow: Reference · §04
---

Define your own fragment type by dropping a YAML schema into `<vault>/schemas/`. parc validates fragments against the schema on save and on reindex; the search DSL knows about every registered type automatically.

## Schema format

A schema describes a single type. The file name (without extension) is the type name.

```yaml
# .parc/schemas/meeting.yml
name: meeting
title: Meeting
description: A scheduled or completed meeting

fields:
  attendees:
    type: list
    item_type: string
    required: false

  scheduled_at:
    type: datetime
    required: true

  duration_minutes:
    type: integer
    required: false
    min: 5
    max: 480

  status:
    type: enum
    values: [scheduled, completed, cancelled]
    default: scheduled

  outcome:
    type: text
    required: false

template: |
  ## Agenda

  - 

  ## Notes

  
  ## Action items

  - [ ] 
```

## Field types

| Type | YAML notes | DSL filter |
|------|------------|------------|
| `string` | Single-line text | `field:value` |
| `text` | Multi-line text | Free-text only |
| `integer` | Whole number, supports `min`/`max` | `field:>=N` |
| `float` | Decimal number, supports `min`/`max` | `field:>=N.M` |
| `boolean` | `true` / `false` | `field:true` |
| `date` | ISO date (no time) | `field:>=2026-03-01` |
| `datetime` | ISO 8601 with time | `field:>=2026-03-01T10:00:00Z` |
| `enum` | One of `values:` | `field:value` |
| `list` | Array of `item_type:` | `field:contains:value` |

Every field accepts:

- `required: true|false` (default `false`)
- `default: <value>` — applied when the field is omitted on creation
- `description: <text>` — shown in `parc schema show`

## Templates

The optional `template:` block is the body parc seeds the editor with for `parc new <type>` (without an inline title). Use it to encode your standard structure for that type — agenda, notes, action items.

If you prefer to keep templates in their own files, drop them in `<vault>/templates/<type>.md` instead. The schema-level `template:` and the file template are merged: the schema block applies first, then the file template overrides any common sections.

## Adding a schema

```bash
parc schema add ./meeting.yml
```

`schema add` validates the file against parc's schema-of-schemas, copies it to `<vault>/schemas/meeting.yml`, and refreshes the type registry. After this:

```bash
parc new meeting "Weekly sync"
parc list meeting
parc search 'type:meeting status:scheduled'
```

Add an alias to the vault's `config.yml#aliases` if you want a one-letter shortcut:

```yaml
aliases:
  m: meeting
```

## Removing a schema

```bash
parc schema remove meeting
```

This deletes `<vault>/schemas/meeting.yml` and unregisters the type. Existing fragments of that type stay on disk but become invalid until you either re-add the schema or change their `type:` field by hand.

Built-in schemas (`note`, `todo`, `decision`, `risk`, `idea`) cannot be removed — they live inside the parc binary. You *can* override them by dropping a file with the same name into `<vault>/schemas/`, but this is rarely a good idea.

## A worked custom type

```yaml
# .parc/schemas/bookmark.yml
name: bookmark
title: Bookmark
description: A URL worth keeping

fields:
  url:
    type: string
    required: true
    description: Canonical URL

  source:
    type: enum
    values: [hn, lobsters, reddit, twitter, mastodon, manual]
    default: manual

  read:
    type: boolean
    default: false

  rating:
    type: integer
    min: 1
    max: 5
    required: false

template: |
  > URL goes here

  ## Why I saved it

  

  ## Notes

  
```

After `parc schema add bookmark.yml`:

```bash
parc new bookmark "Local-first software" \
  --field url=https://www.inkandswitch.com/local-first/ \
  --field source=hn

parc search 'type:bookmark read:false rating:>=4'
```
