---
layout: layouts/doc.njk
title: Configuration
eyebrow: Reference · §03
---

Per-vault settings live in `<vault>/config.yml`. The file is created with sensible defaults by `parc init`. Every field is optional.

## Default config

```yaml
# user: alice            # used in created_by field; defaults to system user
# editor: vim            # defaults to $EDITOR
default_tags: []         # auto-applied to new fragments
date_format: relative    # relative | iso | short
id_display_length: 8     # ULID chars shown in listings
color: auto              # auto | always | never

aliases:
  n: note
  t: todo
  d: decision
  r: risk
  i: idea

server:
  transport: stdio       # stdio | socket
  # socket_path: null    # defaults to <vault>/server.sock

resurfacing:
  stale_days: 30           # cutoff for `parc stale` (and the stale section in `parc review`)
  review_window: this-week # default --since window for `parc review`
  today_section_limit: 10  # max rows per section in `parc today`

plugins:                 # per-plugin configuration, passed at init
  # my-plugin:
  #   setting: value
```

## Field reference

### `user`

String. Used in the `created_by` field on new fragments. If unset, parc falls back to `$USER` (or `$USERNAME` on Windows). Set this when you want fragments to attribute to a name that differs from your system login — useful in shared vaults.

### `editor`

String. Path to the editor parc invokes for `parc new` (without an inline title) and `parc edit`. If unset, parc reads `$EDITOR`, then `$VISUAL`, then falls back to `nano` on Unix and `notepad` on Windows.

### `default_tags`

List of strings. Tags automatically added to every new fragment created in this vault. Useful for tagging an entire local vault by project: `default_tags: [api]` will add `#api` to every fragment automatically.

### `date_format`

How parc renders dates in human output:

| Value | Example |
|-------|---------|
| `relative` (default) | `due in 3d`, `created 2 weeks ago` |
| `iso` | `2026-03-01T10:30:00Z` |
| `short` | `2026-03-01` |

Machine output (`--json`) always uses ISO 8601 regardless of this setting.

### `id_display_length`

Integer. How many characters of the ULID to show in listings. Default `8`. Smaller is more readable, larger reduces ambiguity in vaults with many fragments. Internally parc always works with full IDs.

### `color`

`auto` (default) detects whether stdout is a TTY and respects `NO_COLOR` and `--no-color`. `always` forces colour even when piped. `never` disables colour entirely.

### `aliases`

A map from short name → type. The defaults give you `n / t / d / r / i` for the five built-ins. Add your own:

```yaml
aliases:
  m: meeting
  b: bookmark
  q: question
```

After this, `parc m "1:1 with carla"` is equivalent to `parc new meeting "1:1 with carla"`.

### `server`

JSON-RPC server defaults. See [JSON-RPC server]({{ '/json-rpc/' | url }}).

```yaml
server:
  transport: socket
  socket_path: /tmp/parc.sock
```

### `resurfacing`

Defaults for the resurfacing commands ([CLI reference]({{ '/cli/resurfacing/' | url }})).

| Field | Type | Default | Effect |
|-------|------|---------|--------|
| `stale_days` | integer | `30` | Cutoff used by `parc stale` and the *stale todos* section of `parc review` |
| `review_window` | string | `this-week` | Default `--since` window for `parc review`. Any [date shorthand]({{ '/search-dsl/' | url }}#date-shorthands) or absolute date works |
| `today_section_limit` | integer | `10` | Maximum rows per section in `parc today` |

```yaml
resurfacing:
  stale_days: 14
  review_window: 14-days-ago
  today_section_limit: 5
```

### `plugins`

Per-plugin configuration, keyed by plugin name. parc passes the matching subtree to each plugin's `init` callback at load time. The shape is plugin-specific — see each plugin's manifest.

## Environment variables

These take precedence over `config.yml` for the duration of a single command.

| Variable | Effect |
|----------|--------|
| `PARC_VAULT` | Use this vault path, unless `--vault` or `-g` / `--global` is passed |
| `EDITOR` / `VISUAL` | Editor for `parc new` / `parc edit` |
| `NO_COLOR` | Disable colour, equivalent to `--no-color` |
| `PARC_LOG` | Set log filter, e.g. `PARC_LOG=debug` |

`parc update check` is the only built-in CLI command that makes a network request; it queries the latest GitHub release when run explicitly.

## Schema overrides

You can also override the built-in schemas by dropping a file with the same name in `<vault>/schemas/`. parc resolves schemas with vault-local overrides taking precedence over built-ins. Use this carefully — fragments created with one schema and read against another can fail validation.
