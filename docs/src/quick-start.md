---
layout: layouts/doc.njk
title: Quick start
eyebrow: Getting started · §02
---

Five minutes from zero to a working vault with a few captured thoughts.

## 1. Create a vault

A vault is just a `.parc/` directory. You can have one global vault for personal notes and per-project vaults that travel with the code.

```bash
# Project-local vault — created in the current directory
parc init

# Global vault — created at ~/.parc/
parc init --global
```

parc finds your vault by walking up from the current directory, then falling back to `~/.parc/`. Inside a project-local vault, use `parc -g` or `parc --global` to write to the global vault instead.

## 2. Capture a thought

The shortest command in parc captures a note in one keystroke:

```bash
parc + "Look into connection pooling for the read replicas"
```

`+` is the alias for `parc capture`: a single-line input becomes the title; multi-line input puts the first line in the title and the rest in the body. It always creates a `note` and never opens an editor.

For typed creation with editor + schema validation use the type aliases:

```bash
parc n "Look into connection pooling"   # = parc new note
parc --global n "Book personal tax appointment"
```

`n` is a built-in alias for `new note`. The same shorthand works for the other types: `t` for todo, `d` for decision, `r` for risk, `i` for idea. Any captured note can be promoted later — `parc promote 01JQ7V todo --priority high` rewrites it as a todo while keeping its body, tags, and links.

## 3. Add structured fragments

```bash
# Todo with priority, due date, and a tag
parc t "Upgrade auth library" --priority high --due friday --tag security

# Decision with a tag
parc d "Use Postgres for the event store" --tag infrastructure

# Risk with likelihood and impact
parc r "Token leak through logs" --likelihood medium --impact high
```

## 4. List and search

```bash
# Open todos
parc list todo --status open

# Anything tagged with backend that's due this week
parc search '#backend due:this-week'

# Open high-priority todos
parc search 'type:todo status:open priority:high'
```

## 5. Show and edit

Every fragment has a ULID identifier. You can refer to it by any unique prefix.

```bash
parc show 01JQ7V          # show a fragment
parc edit 01JQ7V          # open it in $EDITOR
parc set 01JQ7V status done
```

## 6. Bring it back to the surface

`parc today` prints a daily digest — what you've touched, what's due, what's open and high priority. Bare `parc` in a terminal opens the [TUI]({{ '/cli/tui/' | url }}); piped or redirected, it falls back to the same digest.

```bash
parc today                # daily digest
parc due overdue          # missed due dates
parc stale --days 14      # open work that's gone quiet
parc review               # weekly multi-section recap
```

See [Resurfacing]({{ '/cli/resurfacing/' | url }}) for the full set.

## Where to next

- [Concepts](#) — fragments, vaults, types, tags, links
- [Search DSL]({{ '/search-dsl/' | url }}) — the full filter language
- [CLI overview]({{ '/cli/' | url }}) — every command in one place
