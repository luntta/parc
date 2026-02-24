# parc

A local-first CLI for capturing, organizing, and retrieving structured fragments of thought. Notes, todos, decisions, risks, ideas — stored as plain Markdown in a `.parc` vault, indexed with SQLite FTS5, and searchable with a composable query DSL.

No accounts. No network. No sync service. Just files you own.

## Install

```bash
cargo install --path parc-cli
cargo install --path parc-server  # optional: standalone JSON-RPC server binary
cargo install --path parc-gui     # optional: Tauri desktop GUI

# With WASM plugin support
cargo install --path parc-cli --features wasm-plugins
```

Requires Rust 1.70+. SQLite is bundled — no system dependencies. WASM plugins require the `wasm-plugins` feature (adds wasmtime). The GUI requires system WebKit (`webkit2gtk-4.1` on Linux).

## Quick start

```bash
# Create a vault
parc init

# Capture a thought
parc n "Look into connection pooling for the read replicas"

# Create a todo with metadata
parc t "Upgrade auth library" --priority high --due friday --tag security

# Log a decision
parc d "Use Postgres for the event store" --tag infrastructure

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

**Links** — `[[id-prefix]]` wiki-links between fragments. Bidirectional at query time — link A→B and parc knows B←A.

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
parc new <type> [--title "..."] [--tag t] [--link id] [--due date]
                [--priority p] [--status s] [--assignee name]
parc list [type] [--status s] [--tag t] [--limit N]
parc show <id>
parc edit <id>
parc set <id> <field> <value>
parc delete <id>
```

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

Send newline-delimited JSON-RPC requests, receive responses:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"fragment.create","params":{"type":"todo","title":"Test","priority":"high"}}' \
  | parc server
```

20 methods covering the full core API: `fragment.{create,get,update,delete,list,search}`, `fragment.{link,unlink,backlinks}`, `fragment.{attach,detach,attachments}`, `vault.{info,reindex,doctor}`, `schema.{list,get}`, `tags.list`, `history.{list,get,restore}`.

See [`docs/json-rpc.md`](docs/json-rpc.md) for the full method reference with examples and integration snippets for Node.js and Python.

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
parc schema show <type>  # Print schema definition
parc schema add <file>   # Register a custom type
parc completions <shell> # bash, zsh, fish, elvish
```

### Global flags

| Flag | Description |
|------|-------------|
| `--vault <path>` | Use a specific vault (also: `PARC_VAULT` env var) |
| `--json` | Machine-readable JSON output |

## Vault discovery

parc finds your vault by checking, in order:

1. `--vault` flag
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

## Desktop GUI

parc ships a Tauri-based desktop app with a full graphical interface:

```bash
# Build the frontend first
cd parc-gui/ui && npm install && npm run build && cd ../..

# Install the binary
cargo install --path parc-gui

# Run it
parc-gui
```

For development with hot-reload:

```bash
cd parc-gui/ui && npx tauri dev
```

The GUI provides:

- **Fragment management** — list, create, edit, and delete fragments with a schema-driven form editor and live Markdown preview
- **Search** — full DSL search with autocomplete suggestions and filter chips
- **Graph view** — interactive Canvas 2D force-directed backlink graph with zoom, pan, and drag
- **Tag browser** — cloud and list views with proportional sizing
- **History viewer** — version timeline with side-by-side diff and restore
- **Attachment management** — drag-and-drop file uploads
- **Vault switcher** — multi-vault support with reindex and doctor actions
- **Command palette** — Ctrl+K quick access to search and navigation
- **Keyboard-driven** — full shortcut map (Ctrl+? to view), vim-style j/k list navigation
- **Dark mode** — system-following or manual light/dark theme toggle

Requires `webkit2gtk-4.1` on Linux (`sudo pacman -S webkit2gtk-4.1` on Arch, `sudo apt install libwebkit2gtk-4.1-dev` on Debian/Ubuntu).

## Architecture

Library-first: `parc-core` is a pure library crate with no terminal I/O. The CLI, JSON-RPC server, and GUI are thin consumers.

```
parc/
├── parc-core/     # Library — no println!, no TTY, returns Result<T, ParcError>
├── parc-cli/      # CLI binary — terminal formatting, $EDITOR, clap
├── parc-server/   # JSON-RPC 2.0 server (stdio / Unix socket)
└── parc-gui/      # Tauri v2 desktop app — vanilla TypeScript web components, zero npm runtime deps
```

Files are the source of truth. The SQLite index is derived and fully rebuildable from the Markdown files at any time with `parc reindex`.

The WASM plugin system is gated behind the `wasm-plugins` cargo feature — default builds have zero wasmtime overhead. Plugin manifest types and `parc plugin list/info` work without the feature; only runtime loading and execution require it.

## License

MIT
