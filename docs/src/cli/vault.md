---
layout: layouts/doc.njk
title: Vault management
eyebrow: CLI · §09
---

Create vaults, list them, and switch between them.

## init

```bash
parc init [--global] [-y | --yes]
```

Creates a new vault. Without flags, creates `.parc/` in the current directory. With `--global`, creates `~/.parc/`.

Like `npm init`, it asks a few questions first: which of the built-in types (note, todo, decision, risk, idea) new fragments can use, and which type captures start as. Press Enter to keep the answer in parentheses; at the end it shows the `types:` block it will write to `config.yml` and asks *Is this OK?*

```text
$ parc init
This walks you through creating a parc vault.
Press Enter to keep the answer in parentheses, or ^C to quit.

Vault: /home/alice/work/api/.parc

Which types should new fragments use? You can turn types on or off later.
note      free-form thoughts and captures (Y/n)
todo      tasks with a status, due date and priority (Y/n)
decision  choices and the reasons for them (Y/n)
risk      what could go wrong, and what is being done about it (Y/n) n
idea      possibilities to explore later (Y/n) n
Type new captures start as: (note) todo

About to write to /home/alice/work/api/.parc/config.yml:

types:
  default: todo
  disabled:
  - risk
  - idea

Is this OK? (yes)
Initialized local vault at /home/alice/work/api/.parc
```

`-y` / `--yes` skips the questions and takes every default: all five types on, notes by default. So does running `init` without a terminal (in a script, or with input piped in).

```bash
parc init                # local vault in $PWD/.parc/, asks first
parc init -y             # the same, with the defaults
parc init --global       # global vault in ~/.parc/
```

`init` writes the standard layout (`config.yml`, `schemas/`, `templates/`, `fragments/`, etc.) and seeds `schemas/` with all five built-in types, including the ones you turned off, so turning a type on later needs no other step. See [Types]({{ '/concepts/types/' | url }}#turning-types-on-and-off). `init` refuses to touch a directory that is already a vault.

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

You can override the discovery order four ways:

```bash
# Per-command flag
parc --vault ~/work/api/.parc list todo

# Global vault shortcut
parc -g list todo
parc --global n "Personal note"

# Environment variable (whole shell session)
export PARC_VAULT=~/work/api/.parc

# Just `cd` into a directory under the vault — discovery does the rest
cd ~/work/api/src && parc list todo
```

The `--vault` flag selects an explicit vault. The `-g` / `--global` flag selects the global `~/.parc/` vault directly and is mutually exclusive with `--vault`. Without either flag, `PARC_VAULT` wins, then walk-up discovery, then the global `~/.parc/`.

## Project + global

A common pattern: keep `~/.parc/` for personal notes and todos, and use project-local vaults for project-scoped fragments. Local vaults shadow the global one whenever you're inside the project — no flags needed.

```bash
cd ~/work/api && parc list todo     # uses ~/work/api/.parc
cd ~ && parc list todo               # uses ~/.parc
```
