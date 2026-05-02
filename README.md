# parc

A local-first CLI for capturing, organizing, and retrieving structured fragments of thought. Notes, todos, decisions, risks, ideas — stored as plain Markdown in a `.parc` vault, indexed with SQLite FTS5, and searchable with a composable query DSL.

No accounts. No network unless you explicitly check for updates. No sync service. Just files you own.

## Install

```bash
# Latest published CLI release
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/luntta/parc/releases/latest/download/parc-cli-installer.sh | sh

# Optional: standalone JSON-RPC server binary
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/luntta/parc/releases/latest/download/parc-server-installer.sh | sh
```

Windows PowerShell installers are published as `parc-cli-installer.ps1` and `parc-server-installer.ps1` on each GitHub release. Release archives include `.sha256` checksums.

From a local checkout:

```bash
cargo install --path parc-cli
cargo install --path parc-server  # optional: standalone JSON-RPC server binary
cargo install --path parc-cli --features wasm-plugins
```

Source builds require Rust 1.70+. SQLite is bundled — no system dependencies. WASM plugins require the `wasm-plugins` feature (adds wasmtime).

## Quick start

```bash
# Create a vault
parc init

# Quick capture — first line becomes the title, rest becomes the body
parc + "Look into connection pooling for the read replicas"

# Or a full new note via the alias
parc n "Look into connection pooling for the read replicas"

# Create a todo with metadata
parc t "Upgrade auth library" --priority high --due friday --tag security

# Log a decision
parc d "Use Postgres for the event store" --tag infrastructure

# Promote that quick capture into a structured todo
parc promote 01JQ7V todo --priority high --due friday

# Today's resurfacing digest
parc today

# List open todos
parc list todo --status open

# Search across everything
parc search 'type:todo status:open #backend due:this-week'

# View a fragment (prefix-match on ID)
parc show 01JQ7V
```

## Concepts

**Fragment** — the atomic unit. A Markdown file with YAML frontmatter containing a common envelope (id, type, title, tags, links, timestamps) plus type-specific fields. Each fragment gets a ULID filename under `.parc/fragments/`.

**Type** — defines what fields a fragment carries. Five built-in types ship with parc; add your own with a YAML schema file.

**Vault** — the `.parc/` directory that holds everything. Can be **global** (`~/.parc/`, your personal archive) or **local** (`.parc/` in a project directory, discovered by walking up from CWD).

**Tags** — merged from the frontmatter `tags:` list and inline `#hashtags` in the body. Case-insensitive, searchable.

**Links** — `[[id-prefix]]` or `[[Fragment title]]` wiki-links between fragments. Bidirectional at query time — link A→B and parc knows B←A.

## Built-in types

| Type | Alias | Key fields |
|------|-------|-----------|
| **note** | `n` | — |
| **todo** | `t` | `status` (open/in-progress/done/cancelled), `priority` (low/medium/high/critical), `due`, `assignee` |
| **decision** | `d` | `status` (proposed/accepted/superseded/deprecated), `deciders` |
| **risk** | `r` | `status` (identified/mitigating/accepted/resolved), `likelihood`, `impact`, `mitigation` |
| **idea** | `i` | `status` (raw/exploring/promoted/parked/discarded) |

Define custom types by dropping a YAML schema into your vault:

```bash
parc schema add my-type.yml
```

## Fragment format

Fragments are plain Markdown files — readable, grep-able, and version-controllable:

```markdown
---
id: 01JQ7V3XKP5GQZ2N8R6T1WBMVH
type: todo
title: Upgrade auth library
tags: [security, backend]
links: [01JQ7V4Y]
status: open
priority: high
due: 2026-02-28
created_at: 2026-02-21T10:30:00Z
updated_at: 2026-02-21T10:30:00Z
---

The current JWT library has a known timing vulnerability.
See #cve-2026-1234 for details.

Related: [[01JQ7V4Y|auth service refactor]]
```

## Search DSL

A single query string combines full-text search with structured filters. All terms are AND-ed.

```bash
# Full-text
parc search "connection pooling"
parc search '"exact phrase"'

# Filter by type, status, priority
parc search 'type:todo status:open priority:high'

# Tags (two equivalent syntaxes)
parc search 'tag:backend'
parc search '#backend'

# Dates — absolute or relative shorthands
parc search 'due:today'
parc search 'due:this-week'
parc search 'due:overdue'
parc search 'created:>2026-01-01'

# By author
parc search 'by:alice'

# Presence checks
parc search 'has:attachments'
parc search 'has:links'
parc search 'has:due'

# Link graph
parc search 'linked:01JQ7V3X'

# Negation with !
parc search 'status:!done'
parc search 'type:!note'

# Combine freely
parc search 'type:todo status:open #backend priority:>=medium due:this-week API'
```

## Commands

### Fragments

```bash
parc + "<text>"                  # quick capture into a note
parc capture "<text>" [--tag t] [--link id]
parc new <type> [--title "..."] [--tag t] [--link id] [--due date]
                [--priority p] [--status s] [--assignee name]
parc promote <id> <new-type> [--priority p] [--due date] [--status s] ...
parc list [type] [--status s] [--tag t] [--limit N]
parc show <id>
parc edit <id>
parc set <id> <field> <value>
parc delete <id>
```

`parc +` (alias for `parc capture`) is the shortest possible path from a thought to a saved note: a single line becomes the title, multi-line input puts the first line in the title and the rest in the body. `parc promote` rewrites a fragment as a different type (note → todo, idea → decision) while preserving its content, links, and timestamps.

Type aliases work everywhere — `parc n` is `parc new note`, `parc t` is `parc new todo`.

### Search

```bash
parc search <query> [--sort order] [--limit N]
```

### Links

```bash
parc link <id-a> <id-b>        # Create bidirectional link
parc unlink <id-a> <id-b>      # Remove link
parc backlinks <id>             # Show what links here
```

### Attachments

```bash
parc attach <id> <file> [--mv]  # Copy (or move) file into vault
parc detach <id> <filename>     # Remove attachment
parc attachments <id>           # List attachments
```

### History

Every edit creates a snapshot automatically — no git required.

```bash
parc history <id>                        # List versions
parc history <id> --show <timestamp>     # View a version
parc history <id> --diff [timestamp]     # Diff against previous
parc history <id> --restore <timestamp>  # Restore a version
```

### Resurfacing

Surface what matters today, what's slipping, and what you've been working on lately. These commands compose existing search filters, so the output mirrors `parc search` semantics.

```bash
parc today                         # touched today + due / overdue + open & high priority
parc due [bucket]                  # bucket: today | overdue | this-week (default: this-week)
parc stale [--days N] [--types t,t]# open work with no updates in the window
parc random [--type t]             # one (or --limit N) random fragments — for serendipity
parc review [--since window]       # multi-section weekly digest
```

Defaults for `stale_days`, the `review` window, and the `today` section limit live under `resurfacing:` in `<vault>/config.yml`.

### Organization

```bash
parc tags                          # All tags with counts
parc archive <id>                  # Exclude from default listings
parc archive <id> --undo           # Unarchive
parc trash                         # List trashed fragments
parc trash --restore <id>          # Recover
parc trash --purge                 # Permanently delete
```

### Export & Import

```bash
parc export --format json [--output file] [query]
parc export --format csv --output report.csv type:todo
parc export --format html --output archive.html
parc import fragments.json [--dry-run]
```

### Vault management

```bash
parc init                # Local vault in .parc/
parc init --global       # Global vault in ~/.parc/
parc --global n "Personal note" # Use ~/.parc/ inside a local vault
parc vault               # Show active vault
parc vault list          # List known vaults
```

### JSON-RPC Server

Run parc as a JSON-RPC 2.0 server for programmatic access from any language:

```bash
parc server                    # stdio transport (for child process / LSP-style)
parc server --socket           # Unix domain socket (persistent server)
parc server --socket-path /tmp/parc.sock
```

Or use the standalone binary:

```bash
parc-server --vault /path/to/.parc
parc-server --vault /path/to/.parc --socket
```

The Unix socket is bound `0600` (owner-only) — the server has no auth, so the file mode is the access boundary. Keep the socket inside a directory only your user can traverse.

Send newline-delimited JSON-RPC requests, receive responses:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"fragment.create","params":{"type":"todo","title":"Test","priority":"high"}}' \
  | parc server
```

20 methods covering the full core API: `fragment.{create,get,update,delete,list,search}`, `fragment.{link,unlink,backlinks}`, `fragment.{attach,detach,attachments}`, `vault.{info,reindex,doctor}`, `schema.{list,get}`, `tags.list`, `history.{list,get,restore}`.

See the [JSON-RPC reference](https://luntta.github.io/parc/json-rpc/) for the full method reference with examples and integration snippets for Node.js and Python.

### Plugins

Extend parc with WebAssembly plugins. Plugins are `.wasm` binaries with TOML manifests, installed into `<vault>/plugins/`. They can hook into fragment lifecycle events, add custom validation, provide custom rendering, and extend the CLI with new subcommands.

```bash
parc plugin list                              # List installed plugins
parc plugin info <name>                       # Show plugin details
parc plugin install <path> [--manifest file]  # Install a plugin
parc plugin remove <name> [--force]           # Remove a plugin
```

Plugins use a capability-based sandbox — each plugin declares what it can do in its manifest:

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "Does something useful"
wasm = "my-plugin.wasm"

[capabilities]
read_fragments = true
write_fragments = false
hooks = ["post-create", "pre-update"]
validate = ["todo"]
render = ["note"]
extend_cli = ["my-command"]
```

Plugin commands become top-level subcommands:

```bash
parc my-command arg1 arg2   # Dispatched to the plugin
```

Requires the `wasm-plugins` feature. Plugins are compiled to `wasm32-unknown-unknown` and communicate with parc through a host API (`parc_host` namespace) for logging, reading/writing fragments, and producing output.

### Maintenance

```bash
parc reindex             # Rebuild index from files
parc doctor              # Check vault health
parc git-hooks install   # Add post-merge reindex hook
parc types               # List registered types
parc version             # Print installed version
parc update check        # Check latest GitHub release
parc schema show <type>  # Print schema definition
parc schema add <file>   # Register a custom type
parc completions <shell> # bash, zsh, fish, elvish
```

### Global flags

| Flag | Description |
|------|-------------|
| `--vault <path>` | Use a specific vault (also: `PARC_VAULT` env var) |
| `-g`, `--global` | Use the global `~/.parc/` vault, ignoring local discovery and `PARC_VAULT` |
| `--json` | Machine-readable JSON output |

## Vault discovery

parc finds your vault by checking, in order:

1. `--vault` flag, or `-g` / `--global` for the global `~/.parc/` vault
2. `PARC_VAULT` environment variable
3. `.parc/` in the current directory, then each parent up to `/`
4. `~/.parc/` (global vault)

This means project-local vaults shadow the global one — keep project fragments with the project, personal fragments in `~/.parc/`.

## Configuration

`<vault>/config.yml`:

```yaml
# user: alice            # Used in created_by field
# editor: vim            # Defaults to $EDITOR
default_tags: []         # Auto-applied to new fragments
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
  transport: stdio      # stdio | socket
  # socket_path: null   # defaults to <vault>/server.sock
resurfacing:
  stale_days: 30        # `parc stale` cutoff
  review_window: this-week  # default `parc review --since` window
  today_section_limit: 10   # max rows per section in `parc today`
plugins:                 # Per-plugin configuration (passed to plugin init)
  my-plugin:
    setting: value
```

## Collaboration

parc is single-user by design, but vaults are git-friendly:

- One file per fragment with a ULID filename — no merge conflicts on creation
- `parc git-hooks install` adds a post-merge hook that runs `parc reindex` automatically
- The index (`index.db`) and `trash/` are gitignored — they rebuild from source files

```bash
# After pulling changes from a collaborator
parc reindex
```

## Terminal UI

Bare `parc` opens a keyboard-driven terminal UI when stdout is a TTY. In pipes and scripts it falls back to the plain `parc today` digest.

```bash
parc          # TUI in a terminal, today digest when piped
parc tui      # force the TUI
parc --no-tui # force plain today output
```

The TUI provides tabbed Today, List, Stale, and Search views with arrow-key navigation, debounced live search, a markdown-rendered detail pane, and inline actions for editing (`e`), toggling todo status (`t`), archiving (`a`), deleting (`d`), promoting (`p`), and yanking the ID to the clipboard (`y`). Press `?` inside the TUI for the full keymap.

## Architecture

Library-first: `parc-core` is a pure library crate with no terminal I/O. The CLI/TUI and JSON-RPC server are thin consumers.

```
parc/
├── parc-core/     # Library — no println!, no TTY, returns Result<T, ParcError>
├── parc-cli/      # CLI binary — terminal formatting, $EDITOR, clap
└── parc-server/   # JSON-RPC 2.0 server (stdio / Unix socket)
```

Files are the source of truth. The SQLite index is derived and fully rebuildable from the Markdown files at any time with `parc reindex`.

The WASM plugin system is gated behind the `wasm-plugins` cargo feature — default builds have zero wasmtime overhead. Plugin manifest types and `parc plugin list/info` work without the feature; only runtime loading and execution require it.

## Releasing

Releases are tag-driven. Update the crate versions, run the local verification, then push a SemVer tag:

```bash
cargo fmt --check
cargo test --workspace --no-default-features
dist plan
git tag v0.2.0
git push origin v0.2.0
```

The `Release` GitHub Actions workflow uses `dist` to build `parc` and `parc-server` archives, shell/PowerShell installers, and SHA-256 checksums for GitHub Releases. Pull requests run in plan-only mode; only tag pushes publish. `parc update check` reads the latest GitHub release for `luntta/parc`, so published tags are the source of truth for update availability.

## License

MIT
