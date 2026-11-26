# parc

**Personal Archive** — a local-first tool for capturing, organizing, and retrieving structured fragments of thought (notes, todos, decisions, risks, ideas, and user-defined types). Everything is plain Markdown in a `.parc` vault.

## Architecture

Library-first design: `parc-core` (library) + thin consumer binaries.

```
parc/
├── parc-core/       # Library crate — no terminal I/O, returns Result<T, ParcError>
├── parc-cli/        # CLI binary + terminal UI (`parc tui`) — terminal I/O, $EDITOR, formatting
└── parc-server/     # JSON-RPC server binary (stdio / Unix socket); `mcp.rs` is the MCP server (`parc mcp`), a thin layer over the RPC methods
```

Rules for `parc-core`:
- No `println!`, no TTY assumptions
- All operations return structured `Result<T, ParcError>`
- Takes `VaultPath` as input, never assumes a location
- A vault's `config.yml` travels with the vault (git is the transport), so treat it as untrusted: it must not choose a program parc runs. `editor:` is honoured only from the global vault (`config::get_editor`)
- Every fragment write goes through `ops::Writer` (validate → pre-hooks → write + history → index → post-hooks). Frontends never call `create_fragment`/`write_fragment` + index themselves, or hooks drift between CLI, TUI and server
- The meaning model (`embed`) is read only from the per-user models folder (`embed::model_dir`: `$PARC_MODELS_DIR` or the platform data folder), never from a path a vault names. Its vectors live in `index.db` outside `SCHEMA_SQL` (keyed by file revision, so an index rebuild keeps them) and are filled by `embed::fill`, never by the sync in `open_index`. Tests whose results must not depend on an installed model set `PARC_MODELS_DIR` to a missing folder, or call `embed::set_enabled(false)`

## Key Dependencies

Rust. Key crates: `clap` (derive), `ratatui`/`crossterm` (TUI), `serde`/`serde_yaml_ng`/`serde_json`, `rusqlite` (bundled, FTS5), `ulid`, `comrak`, `termimad`, `chrono`, `thiserror`/`anyhow`, `similar` (diffing), `regex`, `toml` (plugin manifests), `wasmtime` (optional, feature-gated `wasm-plugins`), `tokio` (CLI + server), `tokenizers`/`gemm`/`memmap2`/`rayon` (the meaning model, run in plain Rust), `assert_cmd`/`tempfile` (testing).

## Core Concepts

- **Fragment**: Markdown file with YAML frontmatter (common envelope + type-specific fields)
- **Fragment types**: defined by YAML schemas in `<vault>/schemas/`. Built-in: note, todo, decision, risk, idea (copied into each vault by `parc init`, so a vault can edit them)
- **Vault settings**: `types:` in `config.yml` (`default` capture type, `disabled` types). `ops::Writer` refuses creating or converting into a disabled type; existing fragments of it stay editable. Save it with `config::save_types`, which rewrites only that block
- **Status categories**: each status value is open, active, done or closed (`categories:` on the status field, `Schema::status_category`, `SchemaRegistry::settled_statuses`). Never hard-code status words for "finished" or "open work"; read the categories. Schema edits that rename or remove values go through `Writer::save_schema`, which moves fragments and refuses to strand them
- **Vault**: `.parc/` directory. Global (`~/.parc/`) or local (`.parc/` in project, discovered by walking up from CWD)
- **IDs**: ULIDs. Prefix-matching supported everywhere
- **Links**: `[[id-prefix]]` wiki-links, bidirectional at query time
- **Tags**: merged from frontmatter `tags:` list + inline `#hashtags` in body. Case-insensitive
- **Attachments**: stored in `<vault>/attachments/<fragment-id>/`, referenced via `![[attach:filename]]`
- **History**: snapshot-on-save to `<vault>/history/<fragment-id>/`
- **Index**: SQLite + FTS5, a cache of the Markdown files. `index::open_index` syncs it with `fragments/` first (mtime/size per file, content hash for racy entries); an index whose stamp (hash of `SCHEMA_SQL` + `INDEX_VERSION`) differs is dropped and rebuilt, so bump `INDEX_VERSION` only when stored data changes without the schema text changing

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
5. Offline and local — no network, no accounts. The desktop app's exceptions are opt-in: speech and meaning model downloads (pinned, hash-checked), and the Agent (ChatGPT or GitHub Copilot sign-in), which sends only what a question needs, only when asked
6. Unix-friendly — pipe-able, `--json`, meaningful exit codes, `$EDITOR`
7. Vault-scoped — global for personal, local for project
8. Tags everywhere — frontmatter + inline `#hashtags`, merged and searchable

## Development Notes

- PRD: `planning/parc-prd.md`
- Git-ignored in vault: `index.db`, `trash/`, `server.sock`
- Collaboration model: git as transport. The index syncs itself with `fragments/` on every `index::open_index` (only changed files are re-read), so no reindex after pull
