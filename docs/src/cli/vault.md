---
layout: layouts/doc.njk
title: Vault management
eyebrow: CLI · §09
---

Create vaults, list them, and switch between them.

## init

```bash
parc init [--global] [--force]
```

Creates a new vault. Without flags, creates `.parc/` in the current directory. With `--global`, creates `~/.parc/`.

```bash
parc init                # local vault in $PWD/.parc/
parc init --global       # global vault in ~/.parc/
```

`init` writes the standard layout (`config.yml`, `schemas/`, `templates/`, `fragments/`, etc.), seeds the schemas directory with the five built-in types, and registers the vault in the known-vaults list.

`--force` overwrites an existing `.parc/` after a confirmation prompt — it does **not** delete fragments, it only re-creates `config.yml` and re-seeds schemas/templates if they're missing.

## vault

```bash
parc vault [list] [--json]
```

Without arguments, prints the active vault path and how parc found it:

```text
Vault: /home/alice/work/api/.parc
Source: walked up from /home/alice/work/api/src
Type: local
```

`vault list` shows every vault parc has seen:

```bash
parc vault list
```

```text
* /home/alice/work/api/.parc          local      (active)
  /home/alice/work/web/.parc          local
  /home/alice/.parc                   global
```

The `*` marker is the currently active vault. The list is just a registry — removing a vault from the registry does not delete the directory, and parc always re-discovers vaults the next time it walks up from CWD.

## Selecting a vault

You can override the discovery order three ways:

```bash
# Per-command flag
parc --vault ~/work/api/.parc list todo

# Environment variable (whole shell session)
export PARC_VAULT=~/work/api/.parc

# Just `cd` into a directory under the vault — discovery does the rest
cd ~/work/api/src && parc list todo
```

The `--vault` flag wins, then `PARC_VAULT`, then walk-up discovery, then the global `~/.parc/`.

## Project + global

A common pattern: keep `~/.parc/` for personal notes and todos, and use project-local vaults for project-scoped fragments. Local vaults shadow the global one whenever you're inside the project — no flags needed.

```bash
cd ~/work/api && parc list todo     # uses ~/work/api/.parc
cd ~ && parc list todo               # uses ~/.parc
```
