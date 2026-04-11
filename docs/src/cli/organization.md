---
layout: layouts/doc.njk
title: Organization
eyebrow: CLI · §07
---

Tag inventories, archiving, and the trash workflow.

## tags

```bash
parc tags [--json] [--sort <order>]
```

Lists every tag in the vault with its usage count. Tags come from both frontmatter `tags:` lists and inline `#hashtags`, merged and case-folded.

```bash
parc tags
```

```text
backend       42
infra         19
security      14
audit          7
postgres       6
```

Sort orders: `count` (default, descending), `name` (alphabetical).

## archive

```bash
parc archive <id> [--undo]
```

Flags a fragment as archived. Archived fragments stay in `<vault>/fragments/`, stay indexed, but are excluded from default `parc list` and `parc search` results unless you pass `--all`.

```bash
parc archive 01JQ7V                # archive
parc archive 01JQ7V --undo         # bring it back into default listings
```

Use archiving for fragments you want to keep findable but not see in your normal flow — old decisions, completed projects, reference notes you don't actively work with.

## trash

```bash
parc trash [--restore <id>] [--purge] [--json]
```

Without flags, lists every trashed fragment.

```bash
parc trash
```

```text
01JQ7Z    note         Random thought · trashed 2026-02-22
01JQ7Y    todo         Old task · trashed 2026-02-20
```

### Restoring

```bash
parc trash --restore 01JQ7Z
```

Moves the fragment back from `<vault>/trash/` to `<vault>/fragments/` and re-indexes it.

### Purging

```bash
parc trash --purge                 # delete every trashed fragment, with confirmation
parc trash --purge --force         # skip confirmation
```

Purging is permanent — there is no recovery once trash is purged. Attachments and history snapshots for purged fragments are also deleted.

## Why archive vs trash?

| | Archive | Trash |
|---|---------|-------|
| Indexed and searchable | Yes | No |
| Visible in default list | No | No |
| Visible with `--all` | Yes | No |
| Recoverable | Always | Until purged |
| Purpose | "Done with this for now" | "I want to delete this" |
