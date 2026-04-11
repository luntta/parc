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

parc finds your vault by walking up from the current directory, then falling back to `~/.parc/`.

## 2. Capture a thought

The shortest command in parc creates a note from a one-liner:

```bash
parc n "Look into connection pooling for the read replicas"
```

`n` is a built-in alias for `new note`. The same shorthand works for the other types: `t` for todo, `d` for decision, `r` for risk, `i` for idea.

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

## Where to next

- [Concepts](#) — fragments, vaults, types, tags, links
- [Search DSL]({{ '/search-dsl/' | url }}) — the full filter language
- [CLI overview]({{ '/cli/' | url }}) — every command in one place
