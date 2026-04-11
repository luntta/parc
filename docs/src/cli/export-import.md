---
layout: layouts/doc.njk
title: Export & import
eyebrow: CLI · §08
---

Move fragments in and out of the vault as JSON, CSV, or self-contained HTML.

## export

```bash
parc export --format <format>
            [--output <file>]
            [--all]
            [<query>]
```

Exports fragments matching `<query>` (or every active fragment if omitted) in the chosen format. Without `--output`, the result is written to stdout.

| Format | Description |
|--------|-------------|
| `json` | Array of full fragment objects, including frontmatter and body |
| `csv` | Flat table of frontmatter fields. Body is omitted; list fields are joined with `;` |
| `html` | Self-contained HTML file with every fragment rendered, an inline stylesheet, and an index |

### Examples

```bash
# Everything as JSON, to stdout
parc export --format json

# Open todos as CSV
parc export --format csv --output todos.csv 'type:todo status:open'

# Static HTML archive of every decision
parc export --format html --output decisions.html 'type:decision'

# Pipe to jq
parc export --format json | jq '[.[] | select(.priority == "high")]'
```

`--all` includes archived fragments. Trashed fragments are never exported.

## import

```bash
parc import <file>
            [--dry-run]
            [--update-existing]
            [--strategy <strategy>]
```

Reads a JSON file produced by `parc export --format json` (or any matching shape) and creates fragments in the vault. Existing fragments are detected by ID.

| Strategy | Behaviour on existing IDs |
|----------|---------------------------|
| `skip` (default) | Leaves existing fragments alone |
| `update` | Overwrites existing fragments — snapshots the previous state into history first |
| `error` | Aborts the import on the first collision |

`--dry-run` parses and validates the input but doesn't write anything to the vault. Use it to check that an import will succeed before committing.

```bash
# Validate first
parc import fragments.json --dry-run

# Then apply
parc import fragments.json --strategy update
```

## Import shape

A minimal importable JSON file is an array of fragment objects:

```json
[
  {
    "id": "01JQ7V3XKP5GQZ2N8R6T1WBMVH",
    "type": "todo",
    "title": "Imported task",
    "tags": ["legacy"],
    "status": "open",
    "body": "Body content goes here.\n"
  }
]
```

Fields not in the schema are dropped. Required fields missing from the input cause that fragment to be rejected — `parc import` reports every rejection but does not abort the rest of the import unless `--strategy error` is set.
