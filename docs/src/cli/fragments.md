---
layout: layouts/doc.njk
title: Fragment commands
eyebrow: CLI · §02
---

Create, view, edit, and delete fragments. The shapes are the same regardless of type — the type just decides which extra fields are accepted.

## new

```bash
parc new <type> [title]
                [--title <text>]
                [--tag <tag>]...
                [--link <id>]...
                [--due <date>]
                [--priority <level>]
                [--status <state>]
                [--assignee <name>]
                [--from <template>]
                [--no-edit]
```

Creates a fragment. If neither a positional title nor `--title` is given, parc opens `$EDITOR` with a templated body and parses the result on save.

```bash
# One-liner
parc n "Look into batched writes"

# Todo with metadata
parc t "Upgrade tokio to 1.40" --priority high --due 2026-04-01 --tag deps

# Decision linked to an existing fragment
parc d "Use Postgres for the event store" --link 01JQ7V --tag infra

# Open the editor with the default todo template
parc t --tag backend
```

`--no-edit` skips the editor even if the title is empty — useful in scripts.

## list

```bash
parc list [type]
          [--status <state>]
          [--tag <tag>]...
          [--limit <n>]
          [--sort <order>]
          [--all]
```

Lists fragments. Without a type argument, lists every type. `--all` includes archived fragments (excluded by default).

```bash
parc list                            # everything, newest first
parc list todo --status open         # open todos
parc list decision --tag infra       # decisions tagged infra
parc list note --limit 5             # five most recent notes
```

Sort orders: `created` (default, newest first), `updated`, `due`, `priority`, `title`.

## show

```bash
parc show <id>
```

Displays a fragment. The body is rendered as Markdown in the terminal; the frontmatter is shown as a header table.

```bash
parc show 01JQ7V
parc show 01JQ7V --json    # raw fragment as JSON
```

## edit

```bash
parc edit <id>
```

Opens the fragment in `$EDITOR`. parc validates the result on save, snapshots the previous version into `<vault>/history/<id>/`, and updates `updated_at`.

If validation fails, parc prints the errors and re-opens the editor — your edits are not lost.

## set

```bash
parc set <id> <field> <value>
```

Updates a single frontmatter field without opening the editor. The field must exist in the type's schema.

```bash
parc set 01JQ7V status done
parc set 01JQ7V priority critical
parc set 01JQ7V due 2026-04-15
parc set 01JQ7V tags backend,security    # comma-separated for list fields
```

Use `parc set <id> <field> ""` to clear an optional field.

## delete

```bash
parc delete <id> [--force]
```

Soft-deletes the fragment by moving it to `<vault>/trash/`. The fragment is removed from the index but stays recoverable until you `parc trash --purge`.

`--force` skips the confirmation prompt.

```bash
parc delete 01JQ7V
parc trash                            # list trashed fragments
parc trash --restore 01JQ7V           # bring it back
parc trash --purge                    # permanently delete every trashed fragment
```
