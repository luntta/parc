---
layout: layouts/doc.njk
title: Fragment format
eyebrow: Reference · §02
---

Every fragment is a Markdown file with YAML frontmatter. The frontmatter is a strict envelope; the body is freeform with two parc-specific extensions.

## File location

```
<vault>/fragments/<ULID>.md
```

The filename is the fragment's ULID. parc never renames files — change the title and the filename stays the same.

## Frontmatter

The frontmatter is delimited by `---` lines and parsed as YAML. Every fragment carries the **common envelope**; type-specific fields go alongside it.

### Common envelope

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `id` | ULID string | yes | Must match the filename |
| `type` | string | yes | Must match a registered schema |
| `title` | string | yes | Plain text, single line |
| `tags` | string[] | no | Merged with inline `#hashtags` at index time |
| `links` | ULID[] | no | Outgoing wiki-links — merged with inline `[[...]]` at index time |
| `created_at` | ISO 8601 timestamp | yes | Set on creation |
| `updated_at` | ISO 8601 timestamp | yes | Bumped on every save |
| `created_by` | string | no | From `config.yml#user` if set |
| `archived` | boolean | no | Default `false` |

### Built-in type fields

| Type | Field | Values |
|------|-------|--------|
| `todo` | `status` | `open` / `in-progress` / `done` / `cancelled` |
| `todo` | `priority` | `low` / `medium` / `high` / `critical` |
| `todo` | `due` | ISO date (`2026-03-01`) or datetime |
| `todo` | `assignee` | string |
| `decision` | `status` | `proposed` / `accepted` / `superseded` / `deprecated` |
| `decision` | `deciders` | string[] |
| `risk` | `status` | `identified` / `mitigating` / `accepted` / `resolved` |
| `risk` | `likelihood` | `low` / `medium` / `high` |
| `risk` | `impact` | `low` / `medium` / `high` / `critical` |
| `risk` | `mitigation` | string |
| `idea` | `status` | `raw` / `exploring` / `promoted` / `parked` / `discarded` |

The `note` type carries no extra fields beyond the common envelope.

## Body

The body is everything after the closing `---` line. parc treats it as standard Markdown (CommonMark via [`comrak`](https://crates.io/crates/comrak)) with two extensions.

### Inline tags

`#hashtag` mentions become tags at index time. Tags from the body are merged with the frontmatter `tags:` list — duplicates are coalesced, case is folded.

```markdown
Need to back-fill #postgres rows from the existing #mysql table
before flipping #backend traffic.
```

A `#` only counts as a tag when it's at a word boundary and followed by an ASCII letter — `#123` is not a tag, `# heading` is not a tag.

### Wiki-links

`[[id-prefix]]` becomes a link to the fragment with that ID prefix. The link target is resolved at render time, not at write time, so prefixes survive ID growth in the vault.

```markdown
Related: [[01JQ7V4Y]]
See also: [[01JQ7V4Y|the auth service refactor]]
```

The pipe form supplies a custom label. Without a label, parc renders the linked fragment's title.

A special `attach:` scheme references files in the current fragment's attachment folder:

```markdown
![[attach:flowchart.png]]      # image
[[attach:postmortem.pdf|PDF]]  # link
```

## Worked example

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
assignee: alice
created_at: 2026-02-21T10:30:00Z
updated_at: 2026-02-21T10:30:00Z
created_by: alice
---

The current JWT library has a known timing vulnerability. See
#cve-2026-1234 for details. Fix is to switch to the constant-time
verifier in the new release.

Tracking: [[01JQ7V4Y|the auth service refactor]]
```

After indexing, this fragment is reachable via:

- `tag:security`, `tag:backend`, `tag:cve-2026-1234`
- `linked:01JQ7V4Y` (and `01JQ7V4Y` shows it as a backlink)
- `type:todo status:open priority:high due:overdue`
- Free text: `auth`, `JWT`, `vulnerability`

## Validation

parc validates every fragment against its type's schema on save and on `parc reindex`. Invalid fragments are surfaced by `parc doctor` and excluded from search results until fixed.
