---
layout: layouts/doc.njk
title: Search DSL
eyebrow: Reference · §01
---

A single query string combines full-text search with structured filters. parc parses the query into a `SearchQuery` AST in `parc-core` and compiles it to an FTS5 `MATCH` plus a SQL `WHERE` clause. All terms are AND-ed; filters live next to free text without quoting.

## Free text

Bare words become full-text terms against the body and title:

```bash
parc search "connection pooling"           # both words must appear
parc search '"connection pool"'            # exact phrase
parc search 'pool*'                         # prefix match
```

Phrase quoting and prefix wildcards pass through to FTS5. The relevance score (BM25) is used to sort results when no other `--sort` is given.

## Filters

A filter is a `key:value` token. Whitespace separates filters from each other and from free text.

| Filter | Values | Example |
|--------|--------|---------|
| `type:` | type name (built-in or custom) | `type:todo` |
| `status:` | type-specific value | `status:open` |
| `priority:` | `low` / `medium` / `high` / `critical`, with `>=` etc. | `priority:>=high` |
| `tag:` | tag name | `tag:backend` |
| `#` | tag name (shorthand for `tag:`) | `#backend` |
| `due:` | date or shorthand | `due:this-week` |
| `created:` | date with optional comparator | `created:>2026-01-01` |
| `updated:` | date with optional comparator | `updated:>=yesterday` |
| `by:` | author name from `created_by` | `by:alice` |
| `has:` | `attachments` / `links` / `due` / `body` | `has:attachments` |
| `linked:` | id prefix — fragments linking to or from | `linked:01JQ7V` |
| `is:` | `archived` / `orphan` / `pinned` | `is:archived` |

### Negation

Prefix any value with `!` to invert it:

```bash
parc search 'status:!done'
parc search 'type:!note'
parc search '#!archived'
```

Negation can be combined with comparators:

```bash
parc search 'priority:!low'
parc search 'created:!>2026-01-01'
```

### Comparators

Numeric and date filters accept comparators inline:

```bash
priority:>=medium
created:>2026-01-01
updated:<=2026-03-01
due:>=tomorrow
```

| Comparator | Meaning |
|------------|---------|
| `=` (default) | Exact match |
| `>` | Strictly greater |
| `>=` | Greater or equal |
| `<` | Strictly less |
| `<=` | Less or equal |

## Date shorthands

parc resolves these against the local clock at query time:

| Shorthand | Resolves to |
|-----------|-------------|
| `today` | The current calendar day |
| `yesterday` | One day ago |
| `tomorrow` | One day ahead |
| `this-week` | Mon–Sun of the current ISO week |
| `last-week` | Previous ISO week |
| `next-week` | Next ISO week |
| `this-month` | First → last of the current month |
| `overdue` | Strictly before today (typically with `due:`) |

Absolute dates accept ISO 8601 (`2026-03-01`, `2026-03-01T10:00:00Z`) or just a year-month (`2026-03`).

## Combining everything

Filters and free text mix freely. All terms are AND-ed. Free-text relevance scores still determine the default sort.

```bash
parc search 'type:todo status:open #backend priority:>=medium due:this-week API'
parc search '#security has:attachments updated:>2026-02-01'
parc search 'type:decision linked:01JQ7V #infra'
```

## What the parser produces

For the curious — `parc search ... --json --explain` (when supported by your build) prints the AST that the parser produced and the SQL query it compiled to. Useful when a query is not returning what you expect.

## Limits and pitfalls

- FTS5 tokenisation is Unicode-aware but does not stem. `pool` and `pools` are different terms — use a wildcard (`pool*`) if you want both.
- Unknown filter keys fail loudly with exit code 5 — there is no silent fall-through to free text.
- A bare colon (`pool:`) is parsed as a filter with an empty value, not a free-text token. Wrap such terms in quotes if you really mean them as text.
- Tag and type names are folded to lowercase before matching; everything else is case-sensitive.
