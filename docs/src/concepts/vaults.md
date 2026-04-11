---
layout: layouts/doc.njk
title: Vaults
eyebrow: Concepts · §02
---

A **vault** is a `.parc/` directory that holds everything: fragments, schemas, templates, attachments, history snapshots, and the SQLite index. parc supports two flavours.

## Global vs local

| | Global | Local |
|---|--------|-------|
| Location | `~/.parc/` | `.parc/` in any directory |
| Use for | Personal notes that travel with you | Project-scoped fragments that travel with the repo |
| Shared with collaborators | No | Yes (via git) |
| Init | `parc init --global` | `parc init` |

A local vault always shadows the global one when you're inside it. Step outside and parc falls back to `~/.parc/` (or whatever the closest ancestor vault is).

## Discovery order

When you run `parc` without `--vault`, it finds your vault by checking, in order:

1. The `--vault <path>` flag
2. The `PARC_VAULT` environment variable
3. `.parc/` in the current directory, then each parent up to `/`
4. `~/.parc/` (the global vault)

This means a project-local vault automatically shadows your global one whenever you're inside the project — no configuration required.

## Layout

```
.parc/
├── config.yml          # vault settings
├── schemas/            # YAML type definitions
├── templates/          # body templates per type
├── fragments/          # one .md file per fragment (ULID filename)
├── attachments/        # binary files organized by fragment ID
├── history/            # version snapshots
├── trash/              # soft-deleted fragments
├── plugins/            # plugin scripts/binaries
├── hooks/              # lifecycle hook scripts
└── index.db            # SQLite index (auto-generated)
```

## What's tracked in git

Local vaults are designed to live alongside source code in a git repository. parc's vault layout assumes the following are git-ignored:

- `index.db` — derivable from `fragments/`, rebuilds with `parc reindex`
- `trash/` — local soft-delete state
- `server.sock` — Unix socket for the long-running JSON-RPC server

`parc init` writes a `.gitignore` inside the vault that excludes these by default.

## Switching vaults

```bash
parc vault           # show the active vault
parc vault list      # list known vaults

# Use a specific vault for one command
parc --vault ~/work/.parc list todo

# Or set an env var for the session
export PARC_VAULT=~/work/.parc
```

See [Vault management]({{ '/cli/vault/' | url }}) for the full command list.
