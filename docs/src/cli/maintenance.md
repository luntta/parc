---
layout: layouts/doc.njk
title: Maintenance
eyebrow: CLI · §10
---

Index management, vault diagnostics, schemas, version checks, and shell completions.

## reindex

```bash
parc reindex [--full]
```

Rebuilds the SQLite index from the Markdown files in `<vault>/fragments/`. parc keeps the index in sync automatically on every write, so you only need `reindex` after:

- Pulling vault changes from a collaborator (or after `git checkout`)
- Editing fragments outside parc (e.g. with vim or a script)
- Recovering from a corrupted `index.db` — delete the file, then `parc reindex`

`--full` drops the existing index entirely and rebuilds from scratch. Without it, parc does an incremental sync (faster, but assumes the existing index is structurally valid).

## doctor

```bash
parc doctor [--fix] [--json]
```

Walks the vault and checks for problems:

- Fragments referenced by the index but missing from disk
- Fragments on disk that aren't in the index
- Broken wiki-links (target ID doesn't exist)
- Schema validation failures
- Orphaned attachments (folder exists, fragment doesn't)
- Stale history snapshots

By default, `doctor` only reports. `--fix` applies safe automatic remediations: re-indexing missing fragments, removing orphaned index rows, and pruning stale history. It will not delete fragments or attachments.

```bash
parc doctor
parc doctor --fix
```

## git-hooks

```bash
parc git-hooks install [--force]
parc git-hooks uninstall
```

Installs a git `post-merge` hook in the surrounding repository that runs `parc reindex` automatically after a successful pull. Useful for vaults that live alongside source code in a shared repo.

```bash
cd ~/work/api
parc git-hooks install
```

`uninstall` removes the hook. `install --force` overwrites an existing hook with parc's version (otherwise parc errors out to avoid clobbering hooks you wrote yourself).

## types

```bash
parc types [--json]
```

Lists every type registered in the active vault, including built-ins and any custom types you've added.

```bash
parc types
```

```text
note       (built-in)
todo       (built-in)
decision   (built-in)
risk       (built-in)
idea       (built-in)
meeting    (custom: schemas/meeting.yml)
```

## version

```bash
parc version [--json]
parc --version
```

Prints the installed CLI version. The JSON form includes the release repository used by update checks.

## update

```bash
parc update check [--json]
parc update [--json]
```

Checks the latest published GitHub release and compares it with the installed version. This command performs an explicit network request to GitHub. Automatic installation is intentionally not implemented yet; use the listed release asset or your package manager.

## schema

```bash
parc schema show <type>           # print a type's schema
parc schema add <file>            # register a custom type from a YAML file
parc schema remove <type>         # remove a custom type (built-ins are read-only)
```

```bash
parc schema show todo             # see what fields a todo carries
parc schema add ./meeting.yml     # register a new type
```

`schema add` validates the file against parc's schema-of-schemas before installing it under `<vault>/schemas/`. See [Custom types]({{ '/custom-types/' | url }}) for the schema format.

## completions

```bash
parc completions <shell>
```

Prints a shell completion script to stdout. Supported shells: `bash`, `zsh`, `fish`, `elvish`.

```bash
# bash
parc completions bash > ~/.local/share/bash-completion/completions/parc

# zsh
parc completions zsh > "${fpath[1]}/_parc"

# fish
parc completions fish > ~/.config/fish/completions/parc.fish
```
