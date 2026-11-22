---
layout: layouts/doc.njk
title: Maintenance
eyebrow: CLI · §10
---

Index management, vault diagnostics, schemas, version checks, and shell completions.

## reindex

```bash
parc reindex [--json]
```

Rebuilds the SQLite index from the Markdown files in `<vault>/fragments/`, in one transaction: if it's interrupted, the previous index stays as it was. Prints the number of fragments indexed and every file that couldn't be indexed, with the reason.

You shouldn't need it. parc brings the index up to date every time it opens it: files added, edited or deleted outside parc (an editor, a script, `git pull`, a sync tool) are picked up by the next command, and only the files that changed are read again. An index from another parc version, or a damaged `index.db`, is rebuilt automatically.

When a command finds a file it can't index (broken frontmatter, not UTF-8, or a frontmatter `id` that doesn't match the file name, as in a sync tool's conflict copy), it prints a warning to stderr once:

```
warning: fragments/01JQ7V3XKP5GQZ2N8R6T1WBMVH.md is not indexed: YAML error: …
```

The file stays out of listings and search until it's fixed; `parc doctor` lists every such file.

## doctor

```bash
parc doctor [--json]
```

Walks the vault and reports problems. It changes nothing.

- Files in `fragments/` that can't be read as fragments (and so aren't indexed)
- Broken and ambiguous wiki-links
- Orphans (fragments nothing links to, that link to nothing)
- Schema validation failures
- Attachment mismatches (files missing on disk, or present but not listed)
- Vault size warnings and plugin issues

```bash
parc doctor
parc doctor --json
```

## git-hooks

```bash
parc git-hooks install [--force]
parc git-hooks uninstall
```

Installs a git `post-merge` hook in the surrounding repository that runs `parc reindex` after a successful pull. Optional: parc picks up pulled changes on its next command anyway, so the hook only moves that work to the pull.

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

Checks the latest published GitHub release and compares it with the installed version. This command performs an explicit network request to GitHub. Automatic installation is intentionally not implemented yet; use the listed `dist` release asset, installer, or your package manager.

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
