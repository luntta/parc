# M6 — Quality of Life

## Features

1. **`--json` on all commands** — Structured JSON output for every command that currently lacks it
2. **`parc tags` aggregation** — List all tags (frontmatter + inline) with usage counts
3. **`parc archive`** — Soft-archive fragments (exclude from default listing/search)
4. **`parc trash` lifecycle** — List trashed fragments, purge old trash
5. **`parc export`** — Export fragments to JSON, CSV, or HTML
6. **`parc import`** — Import fragments from JSON
7. **`parc git-hooks install`** — Generate post-merge hook for auto-reindex
8. **`has:attachments` search fix** — Wire up the currently no-op attachment filter

**Already done (skip):** `--json` is already on `list`, `show`, `search`, `backlinks`, `doctor`, `vault`.

---

## Feature 1: `--json` on All Commands

### Files
- `parc-cli/src/main.rs` — Add `--json` flag to `New`, `Edit`, `Set`, `Delete`, `Link`, `Unlink`, `History`, `Attach`, `Detach`, `Attachments`, `Reindex`, `Types`
- `parc-cli/src/commands/new.rs` — Return JSON with created fragment info
- `parc-cli/src/commands/edit.rs` — Return JSON with edited fragment info
- `parc-cli/src/commands/set.rs` — Return JSON with updated fragment info
- `parc-cli/src/commands/delete.rs` — Return JSON with deleted fragment ID
- `parc-cli/src/commands/link.rs` — Return JSON with linked IDs
- `parc-cli/src/commands/unlink.rs` — Return JSON with unlinked IDs
- `parc-cli/src/commands/history.rs` — Return JSON for list/show/diff/restore modes
- `parc-cli/src/commands/attach.rs` — Return JSON for attach/detach/attachments
- `parc-cli/src/commands/reindex.rs` — Return JSON with reindex stats
- `parc-cli/src/commands/types.rs` — Return JSON with type list

### Tasks
- [ ] Add `#[arg(long)] json: bool` to each command variant in `main.rs`
- [ ] `new.rs`: when `--json`, print `{"id": ..., "type": ..., "title": ...}` instead of plain text
- [ ] `edit.rs`: when `--json`, print `{"id": ..., "title": ..., "updated": true}`
- [ ] `set.rs`: when `--json`, print `{"id": ..., "field": ..., "value": ..., "updated": true}`
- [ ] `delete.rs`: when `--json`, print `{"id": ..., "deleted": true}`
- [ ] `link.rs` / `unlink.rs`: when `--json`, print `{"id_a": ..., "id_b": ..., "linked": true/false}`
- [ ] `history.rs`: when `--json`, output version list as JSON array, show/diff/restore as JSON objects
- [ ] `attach.rs`: when `--json`, output attach/detach result and attachments list as JSON
- [ ] `reindex.rs`: when `--json`, print `{"fragments_indexed": count}`
- [ ] `types.rs`: when `--json`, print array of type objects with name and field info
- [ ] Thread `json` parameter through all `run()` signatures
- [ ] Integration tests: spot-check `--json` on 3-4 commands

---

## Feature 2: `parc tags` Aggregation

### Files
- `parc-core/src/tag.rs` — `aggregate_tags(vault) -> Result<Vec<TagCount>>` (new function)
- `parc-cli/src/commands/tags.rs` — **New.** Command handler
- `parc-cli/src/commands/mod.rs` — `pub mod tags;`
- `parc-cli/src/main.rs` — `Tags` command variant

### Design

Query the `fragment_tags` table with `GROUP BY tag ORDER BY count DESC` to get all tags with counts. This includes both frontmatter tags and inline `#hashtags` since the indexer already merges them.

### Tasks
- [ ] Add `aggregate_tags(conn) -> Result<Vec<TagCount>>` to `parc-core/src/tag.rs`
  - `TagCount { tag: String, count: usize }`
  - SQL: `SELECT tag, COUNT(*) as count FROM fragment_tags GROUP BY tag ORDER BY count DESC`
- [ ] Add `Tags` command to `main.rs`:
  ```
  Tags {
      #[arg(long)] json: bool,
  }
  ```
- [ ] Create `tags.rs` handler:
  - Text mode: table with TAG and COUNT columns, sorted by count descending
  - JSON mode: `[{"tag": "...", "count": N}, ...]`
- [ ] Register module in `mod.rs`
- [ ] Integration tests: create fragments with tags, verify `parc tags` lists them with correct counts

---

## Feature 3: `parc archive`

### Files
- `parc-core/src/fragment.rs` — Handle `archived` as a recognized envelope field (not extra_fields)
- `parc-core/src/index.rs` — Index `archived` field; exclude archived from default queries
- `parc-core/src/search.rs` — Add `is:archived` / `is:active` filter; default to excluding archived
- `parc-cli/src/commands/archive.rs` — **New.** Archive/unarchive handler
- `parc-cli/src/commands/mod.rs` — `pub mod archive;`
- `parc-cli/src/main.rs` — `Archive` command variant

### Design

Archiving sets an `archived: true` field in frontmatter. Archived fragments are excluded from `parc list` and `parc search` by default. Use `is:archived` filter to find them, or `is:all` to include everything.

The `archived` field is kept as a standard extra_field (not a dedicated struct field) since it's a simple boolean that only some fragments will have. The search compiler checks for it in the SQL query.

### Tasks
- [ ] Add `archived` column (`INTEGER DEFAULT 0`) to `fragments` table in index schema
- [ ] Update `index_fragment()` to read `archived` from extra_fields and store in column
- [ ] Update default search to add `AND archived = 0` unless `is:archived` or `is:all` is present
- [ ] Add `is:archived` and `is:all` filter parsing to search DSL
- [ ] Update `list` command's default query to exclude archived
- [ ] Create `archive.rs` CLI command:
  - `parc archive <id>` — set `archived: true` in extra_fields, write_fragment, reindex
  - `parc archive <id> --undo` — remove `archived` from extra_fields
  - `--json` support
- [ ] Register in `mod.rs` and `main.rs`
- [ ] Integration tests: archive a fragment, verify hidden from list, visible with `is:archived`

---

## Feature 4: `parc trash` Lifecycle

### Files
- `parc-cli/src/commands/trash.rs` — **New.** List and purge trashed fragments
- `parc-cli/src/commands/mod.rs` — `pub mod trash;`
- `parc-cli/src/main.rs` — `Trash` command variant

### Design

`parc delete` already moves fragments to `trash/`. This feature adds visibility into trash:
- `parc trash` — list trashed fragments (parse files in `trash/`)
- `parc trash --purge` — permanently delete all trashed fragments
- `parc trash --purge <id>` — permanently delete a specific trashed fragment
- `parc trash --restore <id>` — move fragment back from trash to fragments/

### Tasks
- [ ] Create `trash.rs` CLI command:
  - List mode (default): parse all `.md` files in `trash/`, display table with ID, title, deleted date
  - `--purge`: delete all files from `trash/`
  - `--purge <id>`: delete specific file from `trash/`
  - `--restore <id>`: move file back to `fragments/`, reindex
  - `--json` support for all modes
- [ ] Add optional `list_trash(vault)` and `restore_trash(vault, id)` functions to `parc-core`
- [ ] Register in `mod.rs` and `main.rs`
- [ ] Integration tests: delete a fragment, verify in `parc trash`, restore it, verify back in list

---

## Feature 5: `parc export`

### Files
- `parc-core/src/export.rs` — **New.** Export logic for JSON, CSV, HTML formats
- `parc-core/src/lib.rs` — `pub mod export;`
- `parc-cli/src/commands/export.rs` — **New.** Command handler
- `parc-cli/src/commands/mod.rs` — `pub mod export;`
- `parc-cli/src/main.rs` — `Export` command variant

### CLI Interface

```
parc export --format json [--output file.json]    # export all fragments as JSON
parc export --format csv [--output file.csv]      # export as CSV (metadata only, no body)
parc export --format html [--output dir/]         # export as rendered HTML files
parc export --format json type:todo status:open   # export with search filter
```

### Tasks
- [ ] Create `export.rs` in `parc-core`:
  - `export_json(fragments: &[Fragment]) -> Result<String>` — JSON array of fragments
  - `export_csv(fragments: &[Fragment]) -> Result<String>` — CSV with columns: id, type, title, tags, status, priority, due, created_at, updated_at
  - `export_html(fragments: &[Fragment]) -> Result<Vec<(String, String)>>` — Vec of (filename, html_content), use `comrak` for Markdown rendering
- [ ] Create `export.rs` CLI command:
  - Parse optional search query to filter which fragments to export
  - `--format` flag (json, csv, html; default: json)
  - `--output` flag (file path or directory for HTML; default: stdout)
  - For HTML: create output directory, write one `.html` per fragment plus `index.html`
- [ ] Register in `mod.rs`, `main.rs`, `lib.rs`
- [ ] Integration tests: export as JSON, verify parseable; export as CSV, verify header and rows

---

## Feature 6: `parc import`

### Files
- `parc-core/src/import.rs` — **New.** Import logic
- `parc-core/src/lib.rs` — `pub mod import;`
- `parc-cli/src/commands/import.rs` — **New.** Command handler
- `parc-cli/src/commands/mod.rs` — `pub mod import;`
- `parc-cli/src/main.rs` — `Import` command variant

### CLI Interface

```
parc import <file.json>                 # import fragments from JSON export
parc import <file.json> --dry-run       # show what would be imported
```

### Tasks
- [ ] Create `import.rs` in `parc-core`:
  - `import_json(vault, json_str) -> Result<Vec<ImportResult>>`
  - Parse JSON array of fragment objects
  - For each: validate schema, generate new ULID if ID conflicts, create fragment file
  - `ImportResult { id: String, title: String, status: ImportStatus }` where status is Created, Skipped, or Error
- [ ] Create `import.rs` CLI command:
  - Read file, call `import_json()`
  - Print summary: N created, N skipped, N errors
  - `--dry-run`: parse and validate without writing
  - `--json` support
- [ ] Register in `mod.rs`, `main.rs`, `lib.rs`
- [ ] Integration tests: export then import roundtrip, verify fragments match

---

## Feature 7: `parc git-hooks install`

### Files
- `parc-cli/src/commands/git_hooks.rs` — **New.** Git hook installation handler
- `parc-cli/src/commands/mod.rs` — `pub mod git_hooks;`
- `parc-cli/src/main.rs` — `GitHooks` command variant with `Install` subcommand

### CLI Interface

```
parc git-hooks install     # install post-merge hook that runs `parc reindex`
```

### Design

Find the nearest `.git/` directory relative to the vault, write a `post-merge` hook script that calls `parc reindex`. If hook already exists, append to it (don't overwrite). Make the hook executable.

### Tasks
- [ ] Create `git_hooks.rs` CLI command:
  - Find `.git/hooks/` directory (walk up from vault path)
  - Write `post-merge` hook: `#!/bin/sh\nparc reindex\n`
  - If hook exists and already contains `parc reindex`, skip
  - If hook exists without `parc reindex`, append the command
  - `chmod +x` the hook file
  - Print confirmation message
- [ ] Register in `mod.rs` and `main.rs`
- [ ] Integration test: init git repo, install hooks, verify hook file exists with correct content

---

## Feature 8: `has:attachments` Search Fix

### Files
- `parc-core/src/index.rs` — Add `attachment_count` column to index
- `parc-core/src/search.rs` — Replace no-op `has_attachments_post_filter` with SQL condition

### Tasks
- [ ] Add `attachment_count INTEGER DEFAULT 0` column to `fragments` table
- [ ] Update `index_fragment()` to store `fragment.attachments.len()` as `attachment_count`
- [ ] In `compile_query()`, replace the `has_attachments_post_filter` no-op (line ~501) with:
  ```rust
  Filter::Has(HasCondition::Attachments) => {
      conditions.push("f.attachment_count > 0".to_string());
  }
  ```
- [ ] Integration test: create fragment with attachment, verify `has:attachments` finds it

---

## Implementation Order

1. **Feature 8** (`has:attachments` fix) — small, removes a known TODO
2. **Feature 2** (`parc tags`) — small, self-contained, uses existing index
3. **Feature 1** (`--json` everywhere) — broad but mechanical, touches many files
4. **Feature 3** (`parc archive`) — requires index schema change, search modification
5. **Feature 4** (`parc trash`) — depends on existing delete behavior, builds on archive patterns
6. **Feature 5** (`parc export`) — independent, new modules
7. **Feature 6** (`parc import`) — depends on export format decisions
8. **Feature 7** (`parc git-hooks install`) — independent, small

---

## Verification

```bash
cargo build
cargo test -p parc-core
cargo test -p parc-cli

# Tags
parc new note --title "Tag test" --tag alpha --tag beta
parc new todo --title "Another" --tag alpha
parc tags                              # alpha: 2, beta: 1

# Archive
parc archive <id>
parc list                              # should not show archived
parc search "is:archived"              # should show it
parc archive <id> --undo
parc list                              # should show again

# Trash
parc delete <id>
parc trash                             # should list trashed fragment
parc trash --restore <id>
parc list                              # should show restored fragment

# Export / Import
parc export --format json --output /tmp/export.json
parc export --format csv --output /tmp/export.csv
parc import /tmp/export.json --dry-run
parc import /tmp/export.json

# Git hooks
cd my-git-repo && parc git-hooks install
cat .git/hooks/post-merge              # should contain parc reindex

# JSON output
parc new note --title "JSON test" --json
parc set <id> status open --json
parc delete <id> --json
parc tags --json
parc types --json

# has:attachments search
parc new note --title "With file"
parc attach <id> /path/to/file.txt
parc search "has:attachments"          # should find it
```
