---
layout: layouts/doc.njk
title: Search
eyebrow: CLI · §03
---

A single command, a rich query language. The full DSL is documented on its own page — this is the command surface.

## search

```bash
parc search <query>
            [--sort <order>]
            [--limit <n>]
            [--offset <n>]
            [--json]
```

Runs a query against the vault's FTS5 index and returns matching fragments.

```bash
# Full-text
parc search "connection pooling"
parc search '"exact phrase"'

# Filters
parc search 'type:todo status:open priority:high'

# Tag shorthand
parc search '#backend'

# Combined
parc search 'type:todo #backend due:this-week status:open'
```

## Sort orders

| Order | Field |
|-------|-------|
| `relevance` (default for full-text queries) | FTS5 BM25 rank |
| `created` (default for filter-only queries) | `created_at` desc |
| `updated` | `updated_at` desc |
| `due` | `due` asc, nulls last |
| `priority` | `priority` desc |
| `title` | `title` asc |

```bash
parc search '#backend' --sort updated
parc search 'type:todo' --sort due --limit 20
```

## Output formats

By default, results render as a compact list with the eyebrow, title, type, and a snippet of matched text:

```text
01JQ7V  todo  open    Upgrade auth library
        #security #backend · due in 3d
        > current JWT library has a known timing **vulnerability**

01JQ7Z  todo  open    Migrate to bcrypt
        #security · due tomorrow
```

`--json` returns an array of fragment objects, sorted by the same order:

```json
{
  "ok": true,
  "results": [
    {
      "id": "01JQ7V3XKP5GQZ2N8R6T1WBMVH",
      "type": "todo",
      "title": "Upgrade auth library",
      "tags": ["security", "backend"],
      "status": "open",
      "priority": "high",
      "due": "2026-02-28",
      "snippet": "current JWT library has a known timing vulnerability"
    }
  ],
  "total": 1
}
```

## Saved queries

You can pipe a query through `parc search` from a shell function or alias for repeat use. parc itself does not store named queries — keep them in your shell config or as a small script in `<vault>/scripts/`.

```bash
# ~/.zshrc
alias todos-this-week='parc search "type:todo status:open due:this-week"'
alias inbox='parc search "type:note created:today"'
```

## See also

- [Search DSL]({{ '/search-dsl/' | url }}) — the full query language
- [Tags & links]({{ '/concepts/tags-and-links/' | url }}) — how tags merge from frontmatter and inline `#hashtags`

## recall

```bash
parc recall [<id>] [--text <text> | --text -]
            [--tag <tag>]...
            [--limit <n>]
            [--json]
```

Finds the fragments related to a saved fragment, or to text that isn't saved yet, however old they are. Each comes with why it matched (shared words, tags, a link) and up to two lines of its body, verbatim, with their line numbers.

```bash
# Related to a saved fragment
parc recall 01JQ7V3X

# Related to a draft; - reads it from stdin
parc recall --text "the index is stale after a pull"
pbpaste | parc recall --text -

# Unsaved edits to a saved fragment: that fragment is left out
parc recall 01JQ7V3X --text "$(cat draft.md)"
```

A fragment qualifies by a link with the seed, a shared tag, or at least two shared words; common English and Finnish words don't count. Archived fragments are included. "Nothing in the vault is clearly related" is an answer, not an error. `--json` prints the same result as the server's [`recall.context`]({{ '/json-rpc/' | url }}#recallcontext).
