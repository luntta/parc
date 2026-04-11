---
layout: layouts/doc.njk
title: Link commands
eyebrow: CLI · §04
---

Manage relationships between fragments. parc tracks links bidirectionally even though you only declare them in one direction.

## link

```bash
parc link <id-a> <id-b>
```

Creates a link from `<id-a>` to `<id-b>`. Modifies `<id-a>`'s frontmatter `links:` list and snapshots its previous state into history. The reverse direction (`<id-b>` ← `<id-a>`) becomes visible in `parc backlinks` immediately, no second command needed.

```bash
parc link 01JQ7V 01JQ7V4Y
```

Linking is idempotent — running it twice does nothing the second time and exits 0.

## unlink

```bash
parc unlink <id-a> <id-b>
```

Removes the link from `<id-a>` to `<id-b>`. Snapshots the previous state, no error if the link did not exist.

```bash
parc unlink 01JQ7V 01JQ7V4Y
```

## backlinks

```bash
parc backlinks <id> [--json]
```

Lists every fragment that links to `<id>`. Backlinks are computed at query time from the index, not stored — they always reflect the current state of the vault.

```bash
parc backlinks 01JQ7V4Y
```

```text
01JQ7V    todo       Upgrade auth library
01JQ8M    decision   Use bcrypt for password hashing
01JQ91    risk       Token leak through logs
```

`--json`:

```json
{
  "ok": true,
  "id": "01JQ7V4YKP5GQZ2N8R6T1WBMVH",
  "backlinks": [
    { "id": "01JQ7V3XKP5GQZ2N8R6T1WBMVH", "type": "todo", "title": "Upgrade auth library" }
  ]
}
```

## Inline links in body content

You don't have to use `parc link` — wiki-link syntax in the body works too:

```markdown
See also: [[01JQ7V4Y]]
Related: [[01JQ7V4Y|the auth service refactor]]
```

When parc indexes the fragment, inline links are merged with the frontmatter `links:` list. If you want to remove an inline link, edit the body; if you want to manage links without touching prose, use `parc link` / `parc unlink`.
