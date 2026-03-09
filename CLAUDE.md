# parc

**Personal Archive** — a local-first tool for capturing, organizing, and retrieving structured fragments of thought (notes, todos, decisions, risks, ideas, and user-defined types). Everything is plain Markdown in a `.parc` vault.

## Architecture

Library-first design: `parc-core` (library) + thin consumer binaries.

```
parc/
├── parc-core/       # Library crate — no terminal I/O, returns Result<T, ParcError>
├── parc-cli/        # CLI binary — handles terminal I/O, $EDITOR, formatting
├── parc-server/     # JSON-RPC server binary (stdio / Unix socket)
└── parc-gui/        # Tauri desktop app (TypeScript + web components)
```

Rules for `parc-core`:
- No `println!`, no TTY assumptions
- All operations return structured `Result<T, ParcError>`
- Takes `VaultPath` as input, never assumes a location

## Key Dependencies

Rust. Key crates: `clap` (derive), `serde`/`serde_yaml`/`serde_json`, `rusqlite` (bundled, FTS5), `ulid`, `comrak`, `termimad`, `chrono`, `thiserror`/`anyhow`, `similar` (diffing), `regex`, `toml` (plugin manifests), `wasmtime` (optional, feature-gated `wasm-plugins`), `tokio` (CLI + server), `assert_cmd`/`tempfile` (testing).

## Core Concepts

- **Fragment**: Markdown file with YAML frontmatter (common envelope + type-specific fields)
- **Fragment types**: defined by YAML schemas in `<vault>/schemas/`. Built-in: note, todo, decision, risk, idea
- **Vault**: `.parc/` directory. Global (`~/.parc/`) or local (`.parc/` in project, discovered by walking up from CWD)
- **IDs**: ULIDs. Prefix-matching supported everywhere
- **Links**: `[[id-prefix]]` wiki-links, bidirectional at query time
- **Tags**: merged from frontmatter `tags:` list + inline `#hashtags` in body. Case-insensitive
- **Attachments**: stored in `<vault>/attachments/<fragment-id>/`, referenced via `![[attach:filename]]`
- **History**: snapshot-on-save to `<vault>/history/<fragment-id>/`
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

Filters: `type:`, `status:`, `priority:`, `tag:` / `#`, `due:`, `created:`, `updated:`, `by:`, `has:`, `linked:`, `is:`. Negation with `!` prefix on values. Date shorthands: `today`, `this-week`, `overdue`, etc.

Parsed in `parc-core` into `SearchQuery` AST, compiled to FTS5 MATCH + SQL WHERE clauses.

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
