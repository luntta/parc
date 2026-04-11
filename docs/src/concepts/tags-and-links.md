---
layout: layouts/doc.njk
title: Tags & links
eyebrow: Concepts · §04
---

Two complementary ways to weave fragments together. Tags group; links connect.

## Tags

Tags are merged from two sources:

1. The `tags:` list in the frontmatter
2. Inline `#hashtag` mentions in the body

Both produce the same set at index time. Tags are **case-insensitive** — `#Backend` and `#backend` are the same tag.

```markdown
---
title: Migrate audit log to Postgres
tags: [infrastructure, audit]
---

Need to back-fill #postgres rows from the existing #mysql table
before flipping #backend traffic.
```

This fragment is reachable via `tag:infrastructure`, `tag:audit`, `tag:postgres`, `tag:mysql`, or `tag:backend`.

### Searching by tag

```bash
parc search 'tag:backend'        # by name
parc search '#backend'            # shorthand
parc search '#backend #postgres'  # AND
parc search 'tag:!archived'       # negation
```

### Listing all tags

```bash
parc tags        # show every tag and its usage count
```

## Links

Links are wiki-style references between fragments using ID prefixes.

```markdown
Related: [[01JQ7V4Y]]
See also: [[01JQ7V4Y|the auth service refactor]]
```

The first form renders the linked fragment's title at display time. The second uses your label.

### Bidirectional at query time

You only declare links in one direction (A → B), but parc tracks them both ways. From either fragment you can ask "what links here?":

```bash
parc backlinks 01JQ7V4Y
```

Or use the search filter:

```bash
parc search 'linked:01JQ7V4Y'
```

### Managing links explicitly

You can also create or remove links without touching the body:

```bash
parc link 01JQ7V 01JQ7V4Y       # create A → B
parc unlink 01JQ7V 01JQ7V4Y     # remove A → B
```

This rewrites the source fragment's frontmatter `links:` list.

## When to use which

- **Tags** for ad-hoc grouping. Cheap to add, easy to filter on, no need to commit to a hierarchy.
- **Links** for explicit relationships. Use them when one fragment is *about* another, *supersedes* another, or *blocks* another.

You can use both at once. The convention that works for most people: tag by domain (`#auth`, `#billing`, `#infra`), link by relationship (decision → risk → todo).
