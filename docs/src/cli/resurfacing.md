---
layout: layouts/doc.njk
title: Resurfacing
eyebrow: CLI · §07
---

The resurfacing commands surface fragments you'd otherwise forget — things touched today, things due soon, things that have gone quiet, and things picked at random. Each one is a thin wrapper around the search engine, so output matches what `parc search` would produce with the equivalent filters.

Defaults come from the `resurfacing:` section of `<vault>/config.yml`. See [Configuration]({{ '/configuration/' | url }}#resurfacing).

## today

```bash
parc today [--json]
```

A three-section digest for "what should I look at right now":

1. **Touched today** — fragments created or updated today
2. **Due today / overdue** — open todos due on or before today
3. **Open & high priority** — open todos with priority `high` or `critical`

Bare `parc` in a TTY opens the [TUI]({{ '/cli/tui/' | url }}); when piped or redirected, bare `parc` falls back to `parc today`, which means the digest is a good fit for shell startup files or scripts.

```bash
parc today
parc today --json | jq '.due_today_overdue'
```

Section size is capped by `resurfacing.today_section_limit` (default `10`).

## due

```bash
parc due [bucket] [--json]
```

Open todos by due-date bucket. The default bucket is `this-week`.

| Bucket | Filter |
|--------|--------|
| `today` | `due:today` |
| `overdue` | `due:<today` |
| `this-week` (default) | `due:<=` today + 7 days |

Always restricted to `type:todo` and an unfinished status. Sorted oldest-due first.

```bash
parc due today
parc due overdue
parc due this-week
```

## stale

```bash
parc stale [--days N] [--types <type,type,...>] [--limit N] [--json]
```

Open work that hasn't been updated recently. Without flags, this returns todos, decisions, and risks not updated in `resurfacing.stale_days` days (default `30`).

```bash
parc stale                              # default cutoff
parc stale --days 14                    # past two weeks
parc stale --types todo                 # just todos
parc stale --types todo,risk --limit 5  # narrow + cap
```

Sorted oldest-update first so the most-neglected fragments appear at the top.

## random

```bash
parc random [--limit N] [--type <type>] [--include-done] [--json]
```

Returns one (or `--limit N`) random fragments — useful for serendipitous review. By default, excludes todos (since "random open todo" usually isn't what you want). Pass `--type <type>` to scope to a single type, and `--include-done` to widen the status filter back to "all" for that type.

```bash
parc random                          # one random non-todo fragment
parc random --limit 3                # three of them
parc random --type idea              # any unfinished idea
parc random --type decision --include-done
```

## review

```bash
parc review [--since <window>] [--json]
```

A multi-section weekly digest for retrospectives or end-of-week review. The default window is `resurfacing.review_window` from your config (default `this-week`); pass `--since` to override with any [date shorthand]({{ '/search-dsl/' | url }}#date-shorthands) or absolute date.

Sections, in order:

1. **Edited** — fragments updated in the window
2. **Created** — fragments created in the window
3. **Decisions accepted** — `type:decision status:accepted` updated in the window
4. **Risks identified** — `type:risk` created in the window
5. **Open todos due soon** — open todos due in the next 7 days
6. **Stale todos** — open todos not updated in `stale_days`

```bash
parc review
parc review --since 14-days-ago
parc review --since 2026-04-01
parc review --json | jq '.decisions_accepted'
```

## Composing with search

The resurfacing commands are convenience wrappers — anything they show, you can build with `parc search`. Example: a three-week stale window for just decisions:

```bash
parc search 'type:decision updated:<21-days-ago status:!resolved'
```

Reach for the resurfacing commands when you want a digest; reach for `search` when you want a custom query.
