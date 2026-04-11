---
layout: layouts/doc.njk
title: CLI overview
eyebrow: CLI · §01
---

The `parc` command is the primary way to interact with a vault. It speaks JSON when you ask it to, respects `$EDITOR`, returns meaningful exit codes, and pipes well.

## Command map

| Group | Commands |
|-------|----------|
| [Fragments]({{ '/cli/fragments/' | url }}) | `new`, `list`, `show`, `edit`, `set`, `delete` |
| [Search]({{ '/cli/search/' | url }}) | `search` |
| [Links]({{ '/cli/links/' | url }}) | `link`, `unlink`, `backlinks` |
| [Attachments]({{ '/cli/attachments/' | url }}) | `attach`, `detach`, `attachments` |
| [History]({{ '/cli/history/' | url }}) | `history` |
| [Organization]({{ '/cli/organization/' | url }}) | `tags`, `archive`, `trash` |
| [Export & import]({{ '/cli/export-import/' | url }}) | `export`, `import` |
| [Vault management]({{ '/cli/vault/' | url }}) | `init`, `vault` |
| [Maintenance]({{ '/cli/maintenance/' | url }}) | `reindex`, `doctor`, `git-hooks`, `types`, `schema`, `completions` |

A few commands are also dispatched through this CLI but live in their own pages:

- [`parc server`]({{ '/json-rpc/' | url }}) — JSON-RPC 2.0 server
- [`parc plugin`]({{ '/plugins/' | url }}) — WASM plugin management

## Type aliases

The five built-in types have single-letter aliases that work everywhere a type is expected:

| Alias | Expands to |
|-------|------------|
| `n` | `new note` |
| `t` | `new todo` |
| `d` | `new decision` |
| `r` | `new risk` |
| `i` | `new idea` |

```bash
parc n "Quick capture"            # = parc new note "Quick capture"
parc t "Buy bread" --priority low # = parc new todo "Buy bread" --priority low
parc list t                        # = parc list todo
```

You can change or extend the alias map in `<vault>/config.yml#aliases`.

## Global flags

| Flag | Description |
|------|-------------|
| `--vault <path>` | Use a specific vault. Also via the `PARC_VAULT` env var. |
| `--json` | Machine-readable JSON output. Use this when scripting. |
| `--no-color` | Suppress ANSI colour. Implied when stdout is not a TTY. |
| `--quiet` | Suppress non-error output. |
| `-h`, `--help` | Per-command help. |
| `-V`, `--version` | Print version and exit. |

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Generic failure |
| `2` | Usage error (bad flag, missing arg) |
| `3` | Vault not found |
| `4` | Fragment not found / ambiguous prefix |
| `5` | Validation error (schema or DSL) |

`--json` output always includes a top-level `"ok": true|false` field so you can branch on it without checking exit codes.

## Where output goes

- Human output → stdout, formatted with [`termimad`](https://crates.io/crates/termimad)
- Machine output (`--json`) → stdout as a single newline-terminated JSON value
- Errors → stderr, never mixed into JSON output
- Editor invocations → take over the terminal, then resume parc when the editor exits
