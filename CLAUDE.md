# parc

**Personal Archive** — a local-first CLI tool for capturing, organizing, and retrieving structured fragments of thought (notes, todos, decisions, risks, ideas, and user-defined types). Everything is plain Markdown in a `.parc` vault.

## Architecture

Library-first design: `parc-core` (library) + thin consumer binaries.

```
parc/
├── parc-core/       # Library crate — no terminal I/O, no $EDITOR, returns Result<T, ParcError>
├── parc-cli/        # CLI binary — handles terminal I/O, $EDITOR, formatting
├── parc-server/     # JSON-RPC server binary (stdio / Unix socket)
└── parc-gui/        # Future: Tauri GUI
```

Rules for `parc-core`:
- No `println!`, no TTY assumptions
- All operations return structured `Result<T, ParcError>`
- Takes `VaultPath` as input, never assumes a location

## Language & Key Dependencies

Rust. Key crates: `clap` (derive), `serde`/`serde_yaml`/`serde_json`, `rusqlite` (bundled, FTS5), `ulid`, `comrak`, `termimad`, `chrono` or `time`, `thiserror`/`anyhow`, `similar` (diffing), `assert_cmd`/`tempfile` (testing).

## Core Concepts

- **Fragment**: atomic unit — Markdown file with YAML frontmatter (common envelope + type-specific fields)
- **Fragment types**: defined by YAML schemas in `<vault>/schemas/`. Built-in: note, todo, decision, risk, idea
- **Vault**: `.parc/` directory. Global (`~/.parc/`) or local (`.parc/` in project, discovered by walking up from CWD)
- **IDs**: ULIDs. Prefix-matching supported everywhere
- **Links**: `[[id-prefix]]` wiki-links, bidirectional at query time
- **Tags**: merged from frontmatter `tags:` list + inline `#hashtags` in body. Case-insensitive
- **Attachments**: stored in `<vault>/attachments/<fragment-id>/`, referenced via `![[attach:filename]]`
- **History**: snapshot-on-save to `<vault>/history/<fragment-id>/`. No git dependency
- **Index**: SQLite + FTS5, derived and rebuildable from Markdown source files

## Vault Layout

```
.parc/
├── config.yml
├── schemas/          # YAML type definitions
├── templates/        # Body templates per type
├── fragments/        # One .md file per fragment (ULID filename)
├── attachments/      # Binary files organized by fragment ID
├── history/          # Version snapshots
├── trash/            # Soft-deleted fragments
├── plugins/          # Plugin scripts/binaries
├── hooks/            # Lifecycle hook scripts
└── index.db          # SQLite index (auto-generated, not tracked in git)
```

## Fragment File Format

```markdown
---
id: 01JQ7V3XKP5GQZ2N8R6T1WBMVH
type: todo
title: Example task
tags: [backend, search]
links: [01JQ7V4Y]
status: open
priority: high
due: 2026-03-01
created_at: 2026-02-21T10:30:00Z
updated_at: 2026-02-21T10:30:00Z
---

Body content with #inline-tags and [[01JQ7V4Y|links]].
```

## Search DSL

Single query string combining full-text + structured filters. All terms AND-ed.

Filters: `type:`, `status:`, `priority:`, `tag:` / `#`, `due:`, `created:`, `updated:`, `by:`, `has:`, `linked:`. Negation with `!` prefix on values. Date shorthands: `today`, `this-week`, `overdue`, etc.

Parsed in `parc-core` into `SearchQuery` AST, compiled to FTS5 MATCH + SQL WHERE clauses.

## Milestones

- **M0 — Skeleton**: Workspace, init, new, list, show, search (basic FTS), edit, set, reindex, 5 built-in types, global vault, hashtag extraction
- **M1 — Links**: Wiki-link parsing, backlinks, `parc doctor`
- **M2 — Multi-Vault**: Local vault discovery, `--vault` flag, `PARC_VAULT` env
- **M3 — Search DSL**: Full DSL parser with all filters
- **M4 — Templates & Hooks**: Templates, schema add, aliases, hook scripts, tab completion
- **M5 — History & Attachments**: Version history, attachment management
- **M6 — QoL**: Tags aggregation, archive/trash, export/import, `--json` everywhere
- **M7 — JSON-RPC Server**: `parc-server` binary
- **M8 — WASM Plugins**: Tier 2 plugin system
- **M9 — GUI**: Tauri desktop app

## Design Principles

1. Files are the source of truth — index is always derivable
2. Library first — core has no I/O assumptions
3. One engine, many types — new type = new YAML schema file
4. Progressive complexity — `parc n "thought"` to full DSL + plugins
5. Offline and local — no network, no accounts
6. Unix-friendly — pipe-able, `--json`, meaningful exit codes, `$EDITOR`
7. Vault-scoped — global for personal, local for project
8. Tags everywhere — frontmatter + inline `#hashtags`, merged and searchable

## Development Notes

- PRD: `docs/parc-prd.md`
- Git-ignored in vault: `index.db`, `trash/`, `server.sock`
- Collaboration model: git as transport, `parc reindex` after pull
