# parc — Product Requirements Document

**Version:** 0.3
**Date:** 2026-02-21

---

## 1. Vision

**parc** is a local-first CLI productivity tool for capturing, organizing, and retrieving structured fragments of thought — notes, todos, decisions, risks, ideas, and any user-defined type. Everything lives as plain Markdown files in a `.parc` vault, runs on a single extensible engine, and stays out of your way until you need it.

The name "parc" stands for **P**ersonal **Arc**hive — a place where fragments of work accumulate into a searchable, linked body of knowledge.

---

## 2. Core Concepts

### 2.1 Fragment

A **fragment** is the atomic unit of parc. Every artefact — a note, a todo, a decision — is a fragment. All fragments share a common envelope (metadata) but carry type-specific fields defined by a **schema**.

```
┌─────────────────────────────┐
│  Envelope (common to all)   │
│  ─────────────────────────  │
│  id          (ulid)         │
│  type        (note, todo…)  │
│  title       (string)       │
│  tags        (list)         │
│  links       (list of ids)  │
│  attachments (list, opt)    │
│  created_at  (iso-8601)     │
│  updated_at  (iso-8601)     │
│  created_by  (string, opt)  │
│  ─────────────────────────  │
│  Body (type-specific)       │
│  ─────────────────────────  │
│  Freeform Markdown content  │
│  + schema-defined fields    │
│  + inline #hashtags         │
└─────────────────────────────┘
```

### 2.2 Fragment Type & Schema

Each fragment type is declared by a small schema file that defines:

- The type's name and short alias (e.g. `todo` / `t`)
- Extra frontmatter fields beyond the common envelope (e.g. `status`, `due`, `priority` for todos)
- Default template content
- Allowed statuses or enum values
- Lifecycle hooks (optional, see §12 Plugin System)

Schemas live in `<vault>/schemas/` and are plain YAML files, making them easy to version, share, or extend.

### 2.3 Links

Any fragment can reference another by ID. Links are bidirectional at query time — if fragment A links to fragment B, querying B will surface A as a backlink. The syntax inside Markdown body content is `[[<id-prefix>]]` or `[[<id-prefix>|display text]]`, similar to wiki-links.

### 2.4 Tags

Tags come from two sources, merged into a single unified set:

1. **Frontmatter tags** — explicit list in the YAML header. These are the curated, canonical tags.
2. **Inline hashtags** — `#word` tokens in the Markdown body. These are extracted automatically during indexing.

Both sources are treated identically in search, filtering, and `parc tags`. The merged set is stored in the index. Frontmatter remains the source of record for explicitly managed tags; inline hashtags are additive and low-friction.

**Hashtag parsing rules:**

- A hashtag is `#` followed by one or more alphanumeric characters, hyphens, or underscores: `#backend`, `#Q2-planning`, `#rust_wasm`.
- Hashtags inside fenced code blocks, inline code, and URLs are ignored.
- Hashtags are case-insensitive for matching (`#Backend` and `#backend` are the same tag).
- Duplicate tags (appearing in both frontmatter and body) are deduplicated silently.

### 2.5 Vault

A **vault** is a self-contained `.parc` directory holding fragments, schemas, templates, configuration, and a search index. Parc supports two vault scopes:

- **Global vault:** `~/.parc/` — always available as a fallback.
- **Local vault:** `.parc/` in any directory — project-scoped, discovered by walking up from `$CWD` (similar to how git finds `.git/`).

When both exist, the **local vault takes precedence** for all commands. Users can explicitly target a vault with `--vault <path>` or the `PARC_VAULT` environment variable.

Vaults are independent. Cross-vault search and cross-vault links are explicitly **out of scope** in v1 to keep the model simple.

### 2.6 Attachments

Fragments can reference binary files (images, PDFs, diagrams, etc.) as attachments.

**Storage layout:**

```
<vault>/attachments/<fragment-id>/
├── screenshot.png
├── diagram.svg
└── spec.pdf
```

Each fragment with attachments gets a subdirectory under `<vault>/attachments/` named by its full ULID. Files are copied into this directory when attached.

**Referencing attachments:**

In frontmatter, attachments are listed by filename:

```yaml
attachments:
  - screenshot.png
  - spec.pdf
```

In the Markdown body, attachments are referenced with a custom syntax:

```markdown
See the architecture diagram: ![[attach:diagram.svg]]
The full spec is here: ![[attach:spec.pdf|Project Specification]]
```

The `![[attach:...]]` syntax is distinct from wiki-links (`[[...]]`) and standard Markdown images (`![]()`). This makes attachment references unambiguous and parseable.

**CLI for attachments:**

```bash
parc attach <fragment-id> <file-path>       # copy file into attachment dir
parc attach <fragment-id> <file-path> --mv  # move instead of copy
parc attachments <fragment-id>              # list attachments
parc detach <fragment-id> <filename>        # remove an attachment
```

**Constraints:**

- Attachments are not indexed for full-text search (binary files).
- Attachment filenames must be unique within a fragment's attachment directory.
- No size limit enforced by parc, but `parc doctor` can warn about vaults exceeding a configurable threshold.

---

## 3. Architecture

### 3.1 Library-First Design

parc is built as a **core library + thin consumer binaries**, not a monolithic CLI application. This separation is the foundation for CLI, GUI, and third-party application support.

```
┌──────────────────────────────────────────────────────────────┐
│                       Consumers                              │
│                                                              │
│  ┌──────────┐  ┌──────────┐  ┌─────────────┐  ┌──────────┐ │
│  │ CLI bin  │  │ GUI app  │  │ parc-server  │  │ Plugins  │ │
│  │ (thin)   │  │ (Tauri)  │  │ (JSON-RPC)   │  │ (WASM)   │ │
│  └────┬─────┘  └────┬─────┘  └──────┬───────┘  └────┬─────┘ │
│       │              │               │               │       │
│  ─────┴──────────────┴───────────────┴───────────────┴────── │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │                  parc-core library                     │  │
│  │                                                        │  │
│  │  ┌──────────┐ ┌──────────┐ ┌───────────────────────┐  │  │
│  │  │ Fragment │ │  Index   │ │   Schema Validation   │  │  │
│  │  │  CRUD    │ │ (SQLite  │ │   & Registry          │  │  │
│  │  │          │ │  + FTS5) │ │                       │  │  │
│  │  └──────────┘ └──────────┘ └───────────────────────┘  │  │
│  │  ┌──────────┐ ┌──────────┐ ┌───────────────────────┐  │  │
│  │  │   Link   │ │ Template │ │   Plugin Manager      │  │  │
│  │  │ Resolver │ │  Engine  │ │                       │  │  │
│  │  └──────────┘ └──────────┘ └───────────────────────┘  │  │
│  │  ┌──────────┐ ┌──────────┐ ┌───────────────────────┐  │  │
│  │  │  Search  │ │ History  │ │   Attachment Manager  │  │  │
│  │  │  (DSL)   │ │ Tracker  │ │                       │  │  │
│  │  └──────────┘ └──────────┘ └───────────────────────┘  │  │
│  │  ┌──────────────────────────────────────────────────┐  │  │
│  │  │           Vault Manager (multi-vault)            │  │  │
│  │  └──────────────────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌────────────────────────────────────────────────────────┐  │
│  │              File System + SQLite                      │  │
│  │    (Markdown files + attachments + index.db)           │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

**Rules for the core library:**

- No direct terminal I/O (no `println!`, no TTY assumptions).
- No `$EDITOR` invocation — that belongs in the CLI layer.
- All operations return structured data (`Result<T, ParcError>`).
- The library takes a `VaultPath` as input, never assumes a location.

### 3.2 Integration Layers

| Consumer | Integration | Notes |
|----------|-------------|-------|
| **parc-cli** | Direct Rust crate dependency | Thin CLI layer. Handles terminal I/O, `$EDITOR`, formatting. |
| **parc-gui** (Tauri) | Direct Rust crate dependency | Tauri backend imports `parc-core`. Web frontend via Tauri IPC commands. Zero-overhead. |
| **parc-server** | Direct Rust crate dependency, exposes JSON-RPC | Standalone binary. Bridges `parc-core` to non-Rust consumers. |
| **Electron / 3rd-party apps** | JSON-RPC client → `parc-server` | Language-agnostic. Any app that speaks JSON-RPC over stdio or Unix socket. |
| **Scripts / automation** | `parc-cli --json` | Pipe-friendly. All commands support `--json` output. |

### 3.3 parc-server (JSON-RPC Bridge)

`parc-server` is a thin binary that wraps `parc-core` in a JSON-RPC 2.0 interface. It enables any non-Rust application to interact with parc programmatically.

**Transport options:**

- **stdio** (default) — launched as a child process, communicates via stdin/stdout. Same model as LSP. Ideal for Electron or VS Code extensions.
- **Unix domain socket** — for persistent server scenarios. The socket lives at `<vault>/.parc/server.sock`.

**Protocol:**

```json
// Request
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "fragment.create",
  "params": {
    "type": "todo",
    "title": "Review PRD",
    "tags": ["project"],
    "body": "Review and provide feedback on v0.3"
  }
}

// Response
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "id": "01JQ7V3XKP5GQZ2N8R6T1WBMVH",
    "type": "todo",
    "title": "Review PRD",
    "status": "open",
    "created_at": "2026-02-21T10:30:00Z"
  }
}
```

**Available methods (mirror the core API):**

- `fragment.create`, `fragment.get`, `fragment.update`, `fragment.delete`
- `fragment.list`, `fragment.search`
- `fragment.link`, `fragment.unlink`, `fragment.backlinks`
- `fragment.attach`, `fragment.detach`
- `vault.info`, `vault.reindex`, `vault.doctor`
- `schema.list`, `schema.get`
- `tags.list`
- `history.list`, `history.get`

### 3.4 Workspace Layout (Rust)

```
parc/
├── Cargo.toml              # Workspace root
├── parc-core/              # Library crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── fragment.rs     # Fragment CRUD, parsing, serialization
│       ├── schema.rs       # Schema loading, validation, registry
│       ├── index.rs        # SQLite + FTS5 indexing
│       ├── search.rs       # Search DSL parser and execution
│       ├── link.rs         # Link resolution, backlink computation
│       ├── tag.rs          # Tag extraction (frontmatter + inline hashtags)
│       ├── template.rs     # Template loading and rendering
│       ├── attachment.rs   # Attachment management
│       ├── history.rs      # Fragment version history
│       ├── vault.rs        # Vault discovery, multi-vault logic
│       ├── plugin.rs       # Plugin manager and hook dispatch
│       └── error.rs        # Error types
├── parc-cli/               # CLI binary crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── commands/       # One module per CLI command
│       └── render.rs       # Terminal formatting, Markdown rendering
├── parc-server/            # JSON-RPC server binary crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── rpc.rs          # JSON-RPC method dispatch
│       └── transport.rs    # stdio / Unix socket transport
└── parc-gui/               # Future: Tauri GUI binary crate
    └── ...
```

---

## 4. Vault Structure

```
.parc/
├── config.yml                  # Vault configuration
├── schemas/                    # Fragment type definitions
│   ├── note.yml
│   ├── todo.yml
│   ├── decision.yml
│   ├── risk.yml
│   └── idea.yml
├── templates/                  # Optional body templates per type
│   ├── note.md
│   ├── decision.md
│   └── ...
├── plugins/                    # Plugin scripts/binaries
│   └── ...
├── hooks/                      # Lifecycle hook scripts
│   └── ...
├── fragments/                  # All fragments stored here
│   ├── 01JQ7V3X….md           # One file per fragment (ULID filename)
│   └── ...
├── attachments/                # Binary attachments, organized by fragment ID
│   ├── 01JQ7V3X…/
│   │   ├── screenshot.png
│   │   └── spec.pdf
│   └── ...
├── history/                    # Fragment version history
│   ├── 01JQ7V3X…/
│   │   ├── 2026-02-21T10:30:00Z.md
│   │   └── 2026-02-21T14:15:00Z.md
│   └── ...
├── trash/                      # Soft-deleted fragments
│   └── ...
└── index.db                    # SQLite search/query index (auto-generated)
```

### Fragment File Format

```markdown
---
id: 01JQ7V3XKP5GQZ2N8R6T1WBMVH
type: decision
title: Use SQLite for the search index
tags:
  - architecture
  - search
links:
  - 01JQ7V4Y
attachments:
  - benchmark-results.png
status: accepted
created_at: 2026-02-21T10:30:00Z
updated_at: 2026-02-21T10:30:00Z
created_by: alice
---

## Context

We need fast full-text search across all fragments without
external dependencies. This is critical for the #developer-experience
of the tool.

## Decision

Use SQLite with FTS5 for indexing. The index is derived and rebuildable
from the Markdown source files. See benchmarks: ![[attach:benchmark-results.png]]

Related: [[01JQ7V4Y|Initial architecture notes]]

## Consequences

- No daemon or server process required.
- Portable across machines by copying the fragments directory.
- Index rebuild is O(n) but expected to be fast for <100k fragments.
- Aligns with our #local-first philosophy.
```

In this example, the fragment has:
- Two explicit frontmatter tags: `architecture`, `search`
- Two inline hashtags: `#developer-experience`, `#local-first`
- Indexed tag set (merged): `architecture`, `search`, `developer-experience`, `local-first`
- One wiki-link to fragment `01JQ7V4Y`
- One attachment reference

---

## 5. Built-in Fragment Types

### 5.1 Note

General-purpose capture. No extra required fields.

| Field | Type | Required |
|-------|------|----------|
| *(envelope only)* | — | — |

### 5.2 Todo

Actionable item with lifecycle tracking.

| Field | Type | Required |
|-------|------|----------|
| status | enum: `open`, `in-progress`, `done`, `cancelled` | yes (default: `open`) |
| due | date (iso-8601) | no |
| priority | enum: `low`, `medium`, `high`, `critical` | no (default: `medium`) |
| assignee | string | no |

### 5.3 Decision

A recorded choice with context and consequences.

| Field | Type | Required |
|-------|------|----------|
| status | enum: `proposed`, `accepted`, `superseded`, `deprecated` | yes (default: `proposed`) |
| deciders | list of strings | no |

### 5.4 Risk

Something that might go wrong.

| Field | Type | Required |
|-------|------|----------|
| status | enum: `identified`, `mitigating`, `accepted`, `resolved` | yes (default: `identified`) |
| likelihood | enum: `low`, `medium`, `high` | no |
| impact | enum: `low`, `medium`, `high` | no |
| mitigation | string | no |

### 5.5 Idea

A seed worth capturing but not yet actionable.

| Field | Type | Required |
|-------|------|----------|
| status | enum: `raw`, `exploring`, `promoted`, `parked`, `discarded` | yes (default: `raw`) |

---

## 6. Search DSL

### 6.1 Design Goals

A single query string that combines full-text search with structured field filters. It should feel natural to type, require no quoting for simple cases, and compose predictably.

### 6.2 Grammar

```
query       = term+
term        = filter | hashtag | phrase | word
filter      = field_name ":" value
field_name  = "type" | "status" | "priority" | "due" | "created"
              | "updated" | "by" | "tag" | "has" | "linked"
value       = comparison | word | quoted_string
comparison  = ("<" | ">" | "<=" | ">=" | "=") word
hashtag     = "#" word
phrase      = '"' [^"]+ '"'
word        = [^\s"#:]+
```

### 6.3 Filter Reference

| Filter | Description | Examples |
|--------|-------------|---------|
| `type:` | Fragment type | `type:todo`, `type:decision` |
| `status:` | Status field value | `status:open`, `status:!done` (negation) |
| `priority:` | Priority level | `priority:high`, `priority:>=medium` |
| `tag:` | Matches frontmatter OR inline tags | `tag:backend`, `tag:Q2-planning` |
| `#` | Shorthand for `tag:` | `#backend` (equivalent to `tag:backend`) |
| `due:` | Due date comparison | `due:<2026-03-01`, `due:today`, `due:this-week`, `due:overdue` |
| `created:` | Creation date | `created:>2026-01-01`, `created:today` |
| `updated:` | Last modified date | `updated:>2026-02-01` |
| `by:` | Created-by identity | `by:alice` |
| `has:` | Existence check | `has:attachments`, `has:links`, `has:due` |
| `linked:` | Linked to fragment ID | `linked:01JQ7V3X` |
| `!` prefix | Negation (on any filter) | `status:!cancelled`, `type:!note` |

### 6.4 Combining Terms

All terms are **AND**-ed by default. Unfiltered words and phrases are matched against the full-text index (title + body).

```bash
# All open high-priority todos due this week mentioning "API"
parc search 'type:todo status:open priority:high due:this-week API'

# All fragments tagged "backend" that mention "authentication"
parc search '#backend authentication'

# Decisions made by alice in the last month
parc search 'type:decision by:alice created:>2026-01-21'

# Anything with attachments that links to a specific fragment
parc search 'has:attachments linked:01JQ7V3X'

# Full-text phrase search across all types
parc search '"database migration"'

# Open todos NOT tagged infrastructure
parc search 'type:todo status:open tag:!infrastructure'
```

### 6.5 Date Shorthands

The DSL supports relative date expressions for `due:`, `created:`, and `updated:` filters:

| Shorthand | Meaning |
|-----------|---------|
| `today` | Today's date |
| `yesterday` | Yesterday's date |
| `tomorrow` | Tomorrow's date |
| `this-week` | Monday through Sunday of the current week |
| `last-week` | Previous week |
| `this-month` | Current calendar month |
| `last-month` | Previous calendar month |
| `overdue` | Due date is in the past (only for `due:`) |
| `N-days-ago` | N days before today (e.g. `30-days-ago`) |

### 6.6 Implementation

The DSL is parsed in `parc-core` into a `SearchQuery` AST, which is then compiled to a combination of:

- SQLite FTS5 `MATCH` clause for full-text terms
- SQL `WHERE` clauses for structured field filters
- Post-query filtering for complex conditions (e.g. `has:attachments`)

The parser is a standalone module with no I/O dependencies, making it testable and reusable by all consumers (CLI, GUI, JSON-RPC).

```rust
pub struct SearchQuery {
    pub text_terms: Vec<TextTerm>,     // words and phrases → FTS5
    pub filters: Vec<Filter>,          // field:value → SQL WHERE
    pub sort: SortOrder,               // from --sort flag
    pub limit: Option<usize>,          // from --limit flag
}

pub enum TextTerm {
    Word(String),
    Phrase(String),
}

pub enum Filter {
    Type(String),
    Status { value: String, negated: bool },
    Priority { op: CompareOp, value: String },
    Tag { value: String, negated: bool },
    Due(DateFilter),
    Created(DateFilter),
    Updated(DateFilter),
    CreatedBy(String),
    Has(HasCondition),
    Linked(String),
}
```

---

## 7. Fragment Version History

### 7.1 Rationale

`parc init` does not initialize a git repository — version control is the user's choice. To ensure edits are never lost, parc tracks its own lightweight edit history.

### 7.2 Mechanism

When a fragment is modified (via `parc edit`, `parc set`, or the core API), the **previous version** is copied to the history directory before the new version is written.

```
history/<fragment-id>/
├── 2026-02-21T10:30:00Z.md     # version before first edit
├── 2026-02-21T14:15:00Z.md     # version before second edit
└── 2026-02-21T16:45:00Z.md     # version before third edit
```

Each history file is a complete snapshot of the fragment (frontmatter + body) at that point in time. The filename is the ISO-8601 timestamp of when the snapshot was taken (i.e. the `updated_at` of the version being superseded).

### 7.3 CLI

```bash
parc history <id>                        # list versions with timestamps
parc history <id> --show <timestamp>     # display a specific version
parc history <id> --diff [timestamp]     # diff current vs. previous (or specific) version
parc history <id> --restore <timestamp>  # restore a previous version (creates a new edit)
```

### 7.4 Design Decisions

| Concern | Decision |
|---------|----------|
| **Storage format** | Full file snapshot (not diffs). Simple, readable, self-contained. |
| **Granularity** | One snapshot per save operation. Intermediate edits within `$EDITOR` are not captured. |
| **Retention** | Keep all history by default. Pruning policy (e.g. keep last N versions, or compress after 30 days) deferred to a later milestone. |
| **Index** | History files are NOT indexed for search. Only the current version is searchable. |
| **Attachments** | History does not track attachment changes. Attachment history is out of scope for v1. |

---

## 8. CLI Interface

The top-level command is `parc`. Subcommands follow a `verb [type]` or `verb [query]` pattern.

### 8.1 Vault Management

```bash
parc init                                # create a local vault in $CWD/.parc
parc init --global                       # create the global vault in ~/.parc
parc vault                               # show active vault path and scope
parc vault list                          # list known vaults
```

### 8.2 Creating Fragments

```bash
parc new <type> [--title "..."] [--tag foo --tag bar] [--link <id>]
parc new todo --title "Write PRD" --tag project --due 2026-03-01
parc new note                            # opens $EDITOR with template
```

- If `--title` is omitted, opens `$EDITOR` with the template pre-filled.
- The fragment file is created, the index is updated, and the ID is printed to stdout.

**Alias:** Each type's short alias works as a top-level command for quick capture:

```bash
parc t "Buy groceries" --due tomorrow    # shorthand for: parc new todo
parc n "Meeting notes from standup"      # shorthand for: parc new note
```

### 8.3 Searching

```bash
parc search <dsl-query>                  # unified search using the DSL
parc search 'type:todo status:open'      # structured filters
parc search '#backend authentication'    # tag + full-text
parc search '"exact phrase"'             # phrase search
```

Output is a compact table by default:

```
ID (short)  TYPE      STATUS    TITLE                           TAGS
01JQ7V3X    decision  accepted  Use SQLite for the search idx   architecture, search, developer-experience, local-first
01JQ7V4Y    note      —         Initial architecture thoughts   architecture
```

Options: `--json`, `--ids-only`, `--verbose`, `--limit N`, `--sort updated`.

### 8.4 Viewing & Editing

```bash
parc show <id-prefix>                    # render fragment to terminal
parc edit <id-prefix>                    # open in $EDITOR (saves history snapshot)
parc open <id-prefix>                    # alias for edit
```

- `show` renders Markdown in the terminal and appends **backlinks** and **attachments** sections.
- ID prefixes work anywhere — you only need enough characters to be unique.

### 8.5 Linking

```bash
parc link <id-a> <id-b>                  # create bidirectional link
parc unlink <id-a> <id-b>               # remove link
parc backlinks <id>                      # list all fragments linking here
```

### 8.6 Attachments

```bash
parc attach <id> <file-path>             # copy file into attachment dir
parc attach <id> <file-path> --mv        # move instead of copy
parc attachments <id>                    # list attachments for a fragment
parc detach <id> <filename>              # remove an attachment
```

### 8.7 Fragment History

```bash
parc history <id>                        # list versions with timestamps
parc history <id> --show <timestamp>     # display a specific version
parc history <id> --diff [timestamp]     # diff current vs. version
parc history <id> --restore <timestamp>  # restore previous version
```

### 8.8 Managing Fragments

```bash
parc list [type]                         # list fragments, optionally filtered
parc list todo --status open             # list open todos
parc tags                                # list all tags (frontmatter + inline) with counts
parc set <id> <field> <value>            # update a metadata field (saves history)
parc delete <id>                         # move to .parc/trash/ (soft delete)
parc archive <id>                        # set archived flag
```

### 8.9 Index & Health

```bash
parc reindex                             # rebuild index.db from fragment files
parc doctor                              # broken links, orphans, schema violations,
                                         # attachment mismatches, vault size warnings
```

### 8.10 Schema & Template Management

```bash
parc types                               # list registered fragment types
parc schema show <type>                  # print schema definition
parc schema add <path-to-yaml>           # register a new fragment type
```

### 8.11 Server

```bash
parc server                              # start JSON-RPC server on stdio
parc server --socket                     # start on Unix domain socket
```

---

## 9. Configuration

`<vault>/config.yml`:

```yaml
# Identity (used in created_by, relevant for collaboration)
user: alice

# Editor
editor: $EDITOR                # fallback: vim

# Defaults
default_tags: []               # tags auto-applied to every new fragment
date_format: relative          # "relative" | "iso" | "short"
id_display_length: 8           # characters of ULID shown in listings
color: auto                    # "auto" | "always" | "never"

# Type aliases for quick capture
aliases:
  n: note
  t: todo
  d: decision
  r: risk
  i: idea

# History
history:
  enabled: true                # set false to disable version tracking
  # retention: {}              # future: pruning policies

# Server
server:
  transport: stdio             # "stdio" | "socket"
  socket_path: null            # defaults to <vault>/server.sock

# Plugin configuration
plugins: {}
```

---

## 10. Language Decision: Rust

### 10.1 Rationale (Resolved)

Rust is the implementation language. Key factors:

1. **Schema modeling** — Enums + serde handle the "common envelope + variable type-specific fields" pattern with compile-time guarantees.
2. **WASM plugins** — First-class support for both compiling to and hosting WASM.
3. **GUI path** — Tauri shares the core library crate directly. The JSON-RPC server enables Electron and other non-Rust consumers.
4. **Correctness** — For a tool managing a personal knowledge base, data integrity matters. The type system catches invalid states at compile time.

### 10.2 Go Tradeoffs Considered

Go would have reached MVP ~1.5x faster and offers a superior TUI ecosystem (`bubbletea`, `glamour`). Cross-compilation is trivial. The weaker type system and CGo dependency for SQLite were the deciding factors against it.

### 10.3 Key Rust Dependencies

| Concern | Crate |
|---------|-------|
| CLI argument parsing | `clap` (derive) |
| YAML frontmatter | `serde`, `serde_yaml` |
| JSON output / JSON-RPC | `serde_json` |
| SQLite + FTS5 | `rusqlite` (bundled feature) |
| ULID generation | `ulid` |
| Markdown parsing | `comrak` |
| Terminal Markdown rendering | `termimad` |
| Date/time | `chrono` or `time` |
| File watching (future) | `notify` |
| WASM plugin hosting (future) | `wasmtime` |
| JSON-RPC protocol | `jsonrpc-core` or hand-rolled (simple) |
| Error handling | `thiserror`, `anyhow` |
| Testing | `assert_cmd`, `tempfile` |
| Diffing (history) | `similar` |

---

## 11. Multi-User Collaboration

### 11.1 Model

parc is local-first, not collaborative-first. Multi-user collaboration is supported via **git** (or any file-sync tool) as the transport layer. parc does not implement its own sync.

### 11.2 Why the Architecture Already Works

- **One file per fragment with unique ULIDs** — no filename collisions.
- **Immutable IDs** — renames don't break links.
- **Derived index** — `index.db` is rebuilt locally after pull. A git post-merge hook calling `parc reindex` automates this.

### 11.3 Architectural Provisions

| Concern | Provision |
|---------|-----------|
| **Identity** | `created_by` field in envelope, from `config.yml → user`. |
| **Merge-friendly formatting** | YAML frontmatter uses one-item-per-line for lists. |
| **Conflict detection** | `parc doctor` detects duplicate IDs, broken links, schema violations. |
| **Index after merge** | `parc reindex` rebuilds index. Git hook: `post-merge` → `parc reindex`. |
| **History** | Fragment history survives merges (append-only directory of snapshots). |

### 11.4 Git Workflow

```
.parc/
├── .gitignore          # Contains: index.db, trash/, server.sock
├── fragments/          # Tracked
├── attachments/        # Tracked (consider Git LFS for large files)
├── history/            # Tracked
├── schemas/            # Tracked
├── templates/          # Tracked
└── config.yml          # Tracked (user field may need local override)
```

---

## 12. Plugin System

### 12.1 Goals

- User-defined fragment types with custom validation, rendering, and lifecycle behavior.
- Hooks on fragment lifecycle events.
- Custom CLI commands.
- Sandboxed execution for safety.

### 12.2 Two Tiers

**Tier 1: Hook Scripts (simple, any language, unsandboxed)**

```
hooks/
├── pre-create          # Runs before any fragment is created
├── post-create.todo    # Runs after a todo is created (type-specific)
└── ...
```

Hooks receive the fragment as JSON on stdin and can modify it (pre-hooks) or perform side effects (post-hooks).

**Tier 2: WASM Plugins (powerful, sandboxed)**

```
plugins/
├── my-plugin.wasm
└── my-plugin.toml      # Manifest: name, version, capabilities
```

Declared capabilities: `read-fragments`, `write-fragments`, `extend-cli`, `hook:<event>`, `render:<type>`.

### 12.3 Plugin API (Draft)

```rust
trait ParcPlugin {
    fn name(&self) -> String;
    fn on_event(&mut self, event: LifecycleEvent, fragment: Fragment) -> PluginResult;
    fn validate(&self, fragment: &Fragment) -> ValidationResult;
    fn render(&self, fragment: &Fragment) -> Option<String>;
    fn commands(&self) -> Vec<CommandSpec>;
    fn execute_command(&mut self, cmd: &str, args: &[String]) -> CommandResult;
}
```

---

## 13. GUI Readiness

### 13.1 Native Rust Consumers (Tauri)

Tauri imports `parc-core` as a direct crate dependency. The Rust backend exposes Tauri commands that call core library functions. The web frontend (React, Svelte, etc.) calls these via Tauri's IPC bridge. This is zero-overhead and type-safe.

### 13.2 Non-Rust Consumers (Electron, etc.)

Electron or any other framework can integrate via `parc-server`:

1. Spawn `parc-server` as a child process (stdio transport).
2. Send JSON-RPC requests, receive structured responses.
3. No Rust toolchain needed — the server is a prebuilt binary.

This is the same pattern used by LSP-based editor integrations. The performance overhead of JSON serialization is negligible for the operation volumes parc handles.

### 13.3 Comparison

| Aspect | Tauri (direct) | Electron (JSON-RPC) |
|--------|----------------|---------------------|
| Performance | Native, zero-overhead | JSON serialization (~1ms per call) |
| Binary size | ~5–10 MB | ~100+ MB (Chromium) |
| Type safety | Compile-time via shared types | Runtime via JSON schema |
| Setup complexity | Rust toolchain required | Prebuilt `parc-server` binary |
| Suitability | Primary GUI | 3rd-party integrations, VS Code extensions |

---

## 14. Design Principles

1. **Files are the source of truth.** The SQLite index is always derivable. You can `grep`, `git`, or `rsync` your fragments.
2. **Library first.** The core engine is a library with no I/O assumptions. CLI, GUI, and JSON-RPC server are equal consumers.
3. **One engine, many types.** Adding a fragment type means adding one YAML schema file. No code changes needed for basic types.
4. **Progressive complexity.** `parc n "thought"` is the low end. Schemas, templates, plugins, DSL, and links are there when you need them.
5. **Offline and local.** No network calls, no accounts, no sync service. Pair with git or Syncthing yourself.
6. **Unix-friendly.** Output is pipe-able. `--json` everywhere. Exit codes are meaningful. `$EDITOR` is respected.
7. **Vault-scoped.** Global vault for personal capture, local vaults for project-specific work.
8. **Tags everywhere.** Frontmatter and inline `#hashtags` are first-class, merged, and searchable.

---

## 15. Milestones

### M0 — Skeleton (MVP)

- Rust workspace: `parc-core` + `parc-cli`
- `parc init` — creates vault directory structure with built-in schemas
- `parc new <type>` — create fragments via `$EDITOR`
- `parc list` / `parc show` — basic listing and viewing
- `parc search` — full-text search via SQLite FTS5 (basic, pre-DSL)
- `parc edit` / `parc set` — modify fragments
- `parc reindex` — rebuild index
- Five built-in types: note, todo, decision, risk, idea
- Global vault (`~/.parc`) only
- Inline hashtag extraction and indexing

### M1 — Links & Navigation

- `[[id]]` wiki-link parsing and backlink index
- `parc link` / `parc unlink` / `parc backlinks`
- `show` renders backlinks section
- `parc doctor` — broken links, orphans, schema violations

### M2 — Multi-Vault

- Local vault discovery (walk up from `$CWD`)
- `parc init` for local vaults
- `parc vault` / `parc vault list`
- `--vault` flag and `PARC_VAULT` env var

### M3 — Search DSL

- Full DSL parser in `parc-core`
- Field filters: `type:`, `status:`, `priority:`, `tag:`, `#`, `due:`, `by:`, `has:`, `linked:`
- Date shorthands: `today`, `this-week`, `overdue`, etc.
- Negation with `!`
- Phrase search with quotes
- Replaces basic FTS-only search from M0

### M4 — Templates, Aliases & Hooks

- Template files in `<vault>/templates/`
- `parc schema add` for user-defined types with validation
- Short aliases (`parc t`, `parc n`)
- Relative date parsing for `--due` flag
- Tier 1 plugin system: lifecycle hook scripts
- Tab completion (bash, zsh, fish)

### M5 — History & Attachments

- Fragment version history (snapshot on edit)
- `parc history` — list, show, diff, restore
- Attachment storage and management
- `parc attach` / `parc detach` / `parc attachments`
- `![[attach:...]]` syntax in Markdown body
- `parc doctor` extended: attachment mismatches, vault size

### M6 — Quality of Life

- `parc tags` aggregation with counts (frontmatter + inline)
- `parc archive` / `parc trash` lifecycle
- `parc export` (JSON, CSV, HTML)
- `parc import` from common formats
- Git post-merge hook generation (`parc git-hooks install`)
- `--json` output on all commands

### M7 — JSON-RPC Server

- `parc-server` binary crate
- stdio transport
- Unix socket transport
- Full method coverage mirroring core API
- Documentation for third-party integration

### M8 — WASM Plugin System

- Tier 2: WASM plugin hosting via wasmtime
- Plugin manifest format
- Sandboxed capability model
- Custom CLI subcommands from plugins
- Plugin install/remove commands

### M9 — GUI

- Tauri-based desktop app (`parc-gui` crate)
- Fragment browser, editor, search with DSL
- Visual backlink graph
- Attachment preview
- Reuses `parc-core` directly

---

## 16. Resolved Decisions

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | **Rust** | Schema modeling, WASM plugins, Tauri GUI. See §10. |
| 2 | **Multi-vault** | Local `.parc/` for projects, `~/.parc` global fallback. See §2.5. |
| 3 | **Markdown only** | Universal, renders everywhere. Alt formats via plugins. |
| 4 | **Git-based collaboration** | File-per-fragment + ULID = merge-friendly. No built-in sync. See §11. |
| 5 | **Two-tier plugins** | Hook scripts (M4) + WASM (M8). See §12. |
| 6 | **Library-first** | Core as library crate; CLI, GUI, server are consumers. See §3. |
| 7 | **Tauri for GUI** | Direct crate sharing. Electron via JSON-RPC server. See §13. |
| 8 | **Built-in edit history** | Snapshot-on-save, no git dependency. See §7. |
| 9 | **Custom search DSL** | Composable filters + full-text in one query string. See §6. |
| 10 | **Attachment support** | Stored per-fragment, referenced via `![[attach:]]` syntax. See §2.6. |
| 11 | **Inline hashtags** | `#tags` in body merged with frontmatter tags. See §2.4. |

---

*All open questions from v0.2 are now resolved. This PRD is ready for implementation planning.*
