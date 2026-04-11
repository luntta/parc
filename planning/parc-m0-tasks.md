# parc M0 — Implementation Task Breakdown

**Goal:** Working CLI that can create, list, show, search, edit, and set metadata on fragments in a global vault, with five built-in types and inline hashtag extraction.

**Estimated effort:** ~2–3 weeks for an experienced Rust developer.

---

## Phase 0: Project Scaffolding

### T0.1 — Initialize Rust workspace

Set up the Cargo workspace with two crates.

```
parc/
├── Cargo.toml              # [workspace] members = ["parc-core", "parc-cli"]
├── parc-core/
│   ├── Cargo.toml
│   └── src/lib.rs
├── parc-cli/
│   ├── Cargo.toml          # depends on parc-core
│   └── src/main.rs
├── CLAUDE.md               # Project context for Claude Code
├── README.md
├── .gitignore
└── LICENSE
```

**Dependencies to add immediately:**

parc-core:
- `serde`, `serde_yaml`, `serde_json` (with derive)
- `rusqlite` (features: `bundled`, `fts5`)  (bundled compiles SQLite from source — no system dep)
- `ulid`
- `chrono`
- `thiserror`
- `comrak` (Markdown parsing — needed for hashtag extraction)

parc-cli:
- `clap` (features: `derive`)
- `anyhow`
- `termimad` (terminal Markdown rendering)

**Acceptance criteria:**
- `cargo build` succeeds for both crates.
- `cargo run -p parc-cli` prints a placeholder help message.
- CI runs `cargo test`, `cargo clippy`, `cargo fmt --check`.

**Estimated effort:** 1 hour.

---

### T0.2 — Define core error types

Create `parc-core/src/error.rs` with a `ParcError` enum covering initial failure modes.

```rust
#[derive(Debug, thiserror::Error)]
pub enum ParcError {
    #[error("vault not found: {0}")]
    VaultNotFound(PathBuf),

    #[error("vault already exists: {0}")]
    VaultAlreadyExists(PathBuf),

    #[error("fragment not found: {0}")]
    FragmentNotFound(String),

    #[error("ambiguous ID prefix '{0}': matches {1} fragments")]
    AmbiguousId(String, usize),

    #[error("schema not found for type: {0}")]
    SchemaNotFound(String),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("index error: {0}")]
    IndexError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
```

**Estimated effort:** 30 minutes.

---

## Phase 1: Vault & Schema Foundation

### T1.1 — Vault module (`vault.rs`)

The vault module handles vault discovery and initialization.

**Functions:**

```rust
/// Returns the path to the active vault.
/// Walks up from CWD looking for `.parc/`, falls back to `~/.parc`.
pub fn discover_vault() -> Result<PathBuf, ParcError>

/// Creates a new vault at the given path with the default directory structure
/// and built-in schema files.
pub fn init_vault(path: &Path) -> Result<(), ParcError>

/// Returns true if a valid vault exists at the given path.
pub fn is_vault(path: &Path) -> bool
```

**`init_vault` creates:**
```
.parc/
├── config.yml          (default config)
├── schemas/            (5 built-in schema files)
├── templates/          (5 built-in template files)
├── fragments/          (empty)
├── attachments/        (empty, forward-compat with M5)
├── history/            (empty, forward-compat with M5)
├── trash/              (empty)
└── index.db            (empty, initialized with schema)
```

Note: `attachments/` and `history/` directories are created during init for forward-compatibility with M5, even though they are unused in M0.

**Acceptance criteria:**
- `init_vault` creates the full directory structure.
- `init_vault` on an existing vault returns `VaultAlreadyExists`.
- `discover_vault` walks up from CWD looking for `.parc/`, falls back to `~/.parc`.
- If both local and global vaults exist, local takes precedence.
- Built-in schema YAML files are written correctly (embed them in the binary with `include_str!`).

**Estimated effort:** 2–3 hours.

---

### T1.2 — Schema module (`schema.rs`)

Loads and validates fragment type definitions from YAML files.

**Schema YAML format:**

```yaml
# schemas/todo.yml
name: todo
alias: t
editor_skip: false
fields:
  - name: status
    type: enum
    values: [open, in-progress, done, cancelled]
    required: true
    default: open
  - name: due
    type: date
    required: false
  - name: priority
    type: enum
    values: [low, medium, high, critical]
    required: false
    default: medium
  - name: assignee
    type: string
    required: false
```

`editor_skip` (default `false`): When `true` AND `--title` is provided, `parc new` creates the fragment without opening `$EDITOR`. When `false`, `$EDITOR` always opens even with `--title` (title is pre-filled in the template).

**Data structures:**

```rust
pub struct Schema {
    pub name: String,
    pub alias: Option<String>,
    pub editor_skip: bool,  // default false
    pub fields: Vec<FieldDef>,
}

pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub default: Option<String>,
}

pub enum FieldType {
    String,
    Date,
    Enum(Vec<String>),
    ListOfStrings,
}
```

**Functions:**

```rust
/// Load all schemas from the vault's schemas/ directory.
pub fn load_schemas(vault_path: &Path) -> Result<SchemaRegistry, ParcError>

/// The SchemaRegistry provides lookup by name or alias.
pub struct SchemaRegistry { ... }
impl SchemaRegistry {
    pub fn get_by_name(&self, name: &str) -> Option<&Schema>;
    pub fn get_by_alias(&self, alias: &str) -> Option<&Schema>;
    pub fn resolve(&self, name_or_alias: &str) -> Option<&Schema>;
    pub fn list(&self) -> Vec<&Schema>;
}
```

**Acceptance criteria:**
- All five built-in schemas load without error.
- `resolve("t")` and `resolve("todo")` both return the todo schema.
- Invalid YAML produces a clear error.

**Estimated effort:** 3–4 hours.

---

### T1.3 — Write built-in schemas and templates

Create the five YAML schema files and five Markdown template files.

**Schemas:** `note.yml`, `todo.yml`, `decision.yml`, `risk.yml`, `idea.yml`
(Following the field definitions from the PRD §5. All built-in schemas set `editor_skip: false`.)

**Templates** (example for decision):
```markdown
---
# Fields will be filled automatically. Edit title and tags.
title:
tags: []
---

## Context

<!-- What is the context or background? -->

## Decision

<!-- What was decided? -->

## Consequences

<!-- What are the implications? -->
```

**Acceptance criteria:**
- Each schema matches the PRD spec (correct fields, types, defaults).
- Each template is valid Markdown with YAML frontmatter.
- Templates include helpful comments where appropriate.

**Estimated effort:** 1–2 hours.

---

### T1.4 — Configuration loading (`config.rs`)

Load `config.yml` from the vault. Needed early because `parc new` uses `created_by`, `default_tags`, and `editor`, and `parc list` uses `id_display_length` and `date_format`.

```rust
pub struct Config {
    pub user: Option<String>,
    pub editor: Option<String>,
    pub default_tags: Vec<String>,
    pub date_format: DateFormat,
    pub id_display_length: usize,
    pub color: ColorMode,
    pub aliases: BTreeMap<String, String>,
}
```

**Functions:**

```rust
/// Load config from the vault's config.yml. Missing file → defaults.
pub fn load_config(vault: &Path) -> Result<Config, ParcError>
```

**Acceptance criteria:**
- Missing config file → use defaults.
- Partial config → merge with defaults.
- `editor` field respected (falls back to `$EDITOR`, then `vim`).
- `default_tags` applied to new fragments.

**Estimated effort:** 1–2 hours.

---

## Phase 2: Fragment Engine

### T2.1 — Fragment data model (`fragment.rs`)

Define the core Fragment struct and parsing/serialization logic.

**Data structures:**

```rust
pub struct Fragment {
    pub id: String,                     // ULID
    pub fragment_type: String,          // "note", "todo", etc.
    pub title: String,
    pub tags: Vec<String>,             // frontmatter tags only (inline merged at index time)
    pub links: Vec<String>,            // explicit link IDs from frontmatter
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<String>,
    pub extra_fields: BTreeMap<String, serde_json::Value>,  // type-specific fields (serde_json::Value to avoid leaking serde_yaml into core's public API)
    pub body: String,                  // Markdown content below frontmatter
}
```

**Functions:**

```rust
/// Parse a fragment from a Markdown file with YAML frontmatter.
pub fn parse_fragment(content: &str) -> Result<Fragment, ParcError>

/// Serialize a fragment back to Markdown with YAML frontmatter.
pub fn serialize_fragment(fragment: &Fragment) -> String

/// Generate a new ULID.
pub fn new_id() -> String

/// Create a new fragment with defaults from a schema.
pub fn new_fragment(
    fragment_type: &str,
    title: &str,
    schema: &Schema,
    config: &Config,
) -> Fragment
```

**Frontmatter parsing strategy:**
- Split on `---` delimiters.
- Deserialize YAML into a `BTreeMap<String, serde_json::Value>` (YAML → `serde_json::Value` via serde's data model — JSON values are a subset that covers our needs: strings, numbers, bools, lists).
- Extract known envelope fields (id, type, title, tags, links, created_at, updated_at, created_by).
- Remaining keys go into `extra_fields`.
- Everything after the closing `---` is the body.

**Acceptance criteria:**
- Round-trip: `parse(serialize(fragment)) == fragment`.
- YAML frontmatter uses one-item-per-line for lists.
- `new_fragment` populates defaults from schema (e.g. `status: open` for todos).
- Missing required fields in `extra_fields` produce a validation error when checked against schema.

**Estimated effort:** 4–5 hours.

---

### T2.2 — Fragment validation and CRUD operations

Fragment validation and file-system operations for creating, reading, updating, and deleting fragments.

**Validation function (lives in `parc-core`):**

```rust
/// Validate a fragment against its schema.
/// Checks: required fields present, enum values valid, date formats correct.
/// Called by CLI in `new`, `edit`, `set` commands.
pub fn validate_fragment(fragment: &Fragment, schema: &Schema) -> Result<(), ParcError>
```

**CRUD functions:**

```rust
/// Write a new fragment to disk. Returns the fragment ID.
pub fn create_fragment(vault: &Path, fragment: &Fragment) -> Result<String, ParcError>

/// Read a fragment by full ID or unique prefix.
pub fn read_fragment(vault: &Path, id_or_prefix: &str) -> Result<Fragment, ParcError>

/// Overwrite a fragment file (caller is responsible for updating `updated_at`).
pub fn write_fragment(vault: &Path, fragment: &Fragment) -> Result<(), ParcError>

/// Soft-delete: move fragment file to trash/.
pub fn delete_fragment(vault: &Path, id_or_prefix: &str) -> Result<(), ParcError>

/// List all fragment files in the vault (returns IDs).
pub fn list_fragment_ids(vault: &Path) -> Result<Vec<String>, ParcError>

/// Resolve an ID prefix to a full ID.
/// Returns error if ambiguous (matches > 1) or not found.
pub fn resolve_id(vault: &Path, prefix: &str) -> Result<String, ParcError>
```

**File naming:** `fragments/<full-ulid>.md`

**ID prefix resolution:**
- Scan filenames in `fragments/`.
- Filter to those starting with the prefix.
- If exactly one match → return full ID.
- If zero → `FragmentNotFound`.
- If multiple → `AmbiguousId`.

**Acceptance criteria:**
- Create writes file with correct name and content.
- Read with prefix works (e.g. first 8 chars of ULID).
- Delete moves file to `trash/`.
- Ambiguous prefix returns clear error with match count.

**Estimated effort:** 3–4 hours.

---

### T2.3 — Tag extraction (`tag.rs`)

Extract inline hashtags from Markdown body and merge with frontmatter tags.

**Functions:**

```rust
/// Extract hashtags from Markdown body, ignoring code blocks and inline code.
pub fn extract_inline_tags(markdown: &str) -> Vec<String>

/// Merge frontmatter tags and inline tags, deduplicated, case-insensitive.
pub fn merge_tags(frontmatter_tags: &[String], inline_tags: &[String]) -> Vec<String>
```

**Parsing approach:**

Use `comrak` to parse the Markdown AST. Walk text nodes only (skip code blocks, inline code, and URLs). Apply a regex `#([a-zA-Z0-9][a-zA-Z0-9_-]*)` to text content.

**Edge cases:**
- `#123` (pure numeric) — include or exclude? **Decision: exclude.** Likely a heading reference, not a tag.
- `##heading` — not a tag (Markdown heading syntax). Only match `#` preceded by whitespace or start-of-line.
- `#tag` inside `[link](#tag)` — exclude (URL fragment).
- `#tag` inside `` `code` `` — exclude.

**Acceptance criteria:**
- Extracts `#backend` from prose.
- Ignores `#backend` inside fenced code blocks.
- Ignores `#backend` inside inline code.
- Ignores `#backend` in URLs.
- Ignores `#123` (pure numeric).
- Case-insensitive dedup: `#Backend` and `#backend` → `["backend"]`.
- Merges with frontmatter tags without duplicates.

**Estimated effort:** 3–4 hours.

---

## Phase 3: Search Index

### T3.1 — SQLite index schema (`index.rs`)

Create and manage the SQLite database with FTS5 for full-text search.

**Tables:**

```sql
-- Main fragment metadata table
CREATE TABLE fragments (
    id          TEXT PRIMARY KEY,
    type        TEXT NOT NULL,
    title       TEXT NOT NULL,
    status      TEXT,
    priority    TEXT,
    due         TEXT,
    assignee    TEXT,
    created_by  TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    body        TEXT NOT NULL,
    extra_json  TEXT            -- remaining extra_fields as JSON
);

-- Tags table (normalized, one row per fragment-tag pair)
CREATE TABLE fragment_tags (
    fragment_id TEXT NOT NULL REFERENCES fragments(id),
    tag         TEXT NOT NULL,
    PRIMARY KEY (fragment_id, tag)
);

-- Full-text search index (standalone FTS5 — manages its own storage, simpler for M0, index is rebuildable anyway)
CREATE VIRTUAL TABLE fragments_fts USING fts5(
    id UNINDEXED,
    title,
    body,
    tags
);

-- Links table
CREATE TABLE fragment_links (
    source_id TEXT NOT NULL REFERENCES fragments(id),
    target_id TEXT NOT NULL,
    PRIMARY KEY (source_id, target_id)
);

CREATE INDEX idx_fragments_type ON fragments(type);
CREATE INDEX idx_fragments_status ON fragments(status);
CREATE INDEX idx_fragments_due ON fragments(due);
CREATE INDEX idx_fragment_tags_tag ON fragment_tags(tag);
```

**Functions:**

```rust
/// Initialize the database schema (create tables if not exist).
pub fn init_index(vault: &Path) -> Result<Connection, ParcError>

/// Open an existing index.
pub fn open_index(vault: &Path) -> Result<Connection, ParcError>

/// Index a single fragment (upsert into all tables).
pub fn index_fragment(conn: &Connection, fragment: &Fragment, merged_tags: &[String]) -> Result<(), ParcError>

/// Remove a fragment from the index.
pub fn remove_from_index(conn: &Connection, id: &str) -> Result<(), ParcError>

/// Rebuild the entire index from fragment files.
pub fn reindex(vault: &Path) -> Result<usize, ParcError>
```

**Acceptance criteria:**
- `init_index` is idempotent (safe to call on existing DB).
- `index_fragment` correctly populates all tables including FTS.
- `reindex` scans all files, clears index, rebuilds. Returns count.
- FTS5 search for a word in body or title returns matching fragment IDs.

**Estimated effort:** 4–5 hours.

---

### T3.2 — Search module (`search.rs`)

For M0, implement basic full-text search with simple structured filters. (Full DSL comes in M3.)

**Functions:**

```rust
pub struct SearchParams {
    pub query: Option<String>,          // full-text search string
    pub type_filter: Option<String>,    // --type
    pub status_filter: Option<String>,  // --status
    pub tag_filter: Vec<String>,        // --tag (AND)
    pub sort: SortOrder,                // --sort
    pub limit: Option<usize>,          // --limit
}

pub enum SortOrder {
    UpdatedDesc,  // default
    UpdatedAsc,
    CreatedDesc,
    CreatedAsc,
}

pub struct SearchResult {
    pub id: String,
    pub fragment_type: String,
    pub title: String,
    pub status: Option<String>,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub snippet: Option<String>,        // FTS5 snippet for full-text matches
}

/// Execute a search query against the index.
pub fn search(conn: &Connection, params: &SearchParams) -> Result<Vec<SearchResult>, ParcError>
```

**Query building:**
- Start with `SELECT` from `fragments`.
- If `query` is set, `JOIN fragments_fts` with `MATCH`.
- Add `WHERE` clauses for type, status.
- For tags, `JOIN fragment_tags` with `GROUP BY` + `HAVING COUNT = len(tag_filter)` for AND semantics.
- Apply `ORDER BY` and `LIMIT`.

**Acceptance criteria:**
- Full-text search finds fragments by word in title or body.
- `--type todo` filters to todos only.
- `--status open` filters by status.
- `--tag a --tag b` returns only fragments with BOTH tags.
- Default sort is most recently updated first.
- Results include a text snippet for FTS matches.

**Estimated effort:** 4–5 hours.

---

## Phase 4: CLI Commands

### T4.1 — CLI scaffolding (`main.rs` + `commands/`)

Set up `clap` with the command structure.

```rust
#[derive(Parser)]
#[command(name = "parc", about = "Personal Archive")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        #[arg(long)]
        global: bool,
    },
    New {
        type_name: String,
        /// Positional title — mutually exclusive with --title (positional takes precedence).
        /// Enables: `parc new note "quick thought"` or `parc n "quick thought"`
        title: Option<String>,
        #[arg(long, name = "title")]
        title_flag: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        link: Vec<String>,
        // type-specific flags:
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        priority: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        assignee: Option<String>,
    },
    List {
        type_name: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        limit: Option<usize>,
    },
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    Edit {
        id: String,
    },
    Set {
        id: String,
        field: String,
        value: String,
    },
    Search {
        query: Vec<String>,            // collected into a single string
        #[arg(long, name = "type")]
        type_filter: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        tag: Vec<String>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        sort: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Delete {
        id: String,
    },
    Reindex,
    Types,
}
```

**Acceptance criteria:**
- `parc --help` shows all commands.
- `parc new --help` shows type-specific flags.
- Unknown commands produce clear error.

**Estimated effort:** 2–3 hours.

---

### T4.2 — `parc init`

Calls `vault::init_vault`. Prints the created path.

- `parc init` (without `--global`) creates a **local vault** in `$CWD/.parc`.
- `parc init --global` creates `~/.parc`.
- If both exist, vault discovery prefers local (handled by `discover_vault` in T1.1).

```
$ parc init
Initialized local vault at /home/alice/project/.parc

$ parc init --global
Initialized global vault at /home/alice/.parc
```

**Estimated effort:** 30 minutes.

---

### T4.3 — `parc new`

Creates a new fragment. Title can be provided as a positional argument (`parc new note "thought"`) or via `--title`.

**Editor behavior (schema-driven):**
- If `editor_skip` is `true` in the schema AND a title is provided → create the fragment directly, print the ID, skip `$EDITOR`.
- If `editor_skip` is `false` (default for all built-in types) AND a title is provided → open `$EDITOR` with the title pre-filled in the template.
- If no title is provided → always open `$EDITOR`.

**Editor flow:**
- Write a temp file with the template content (title pre-filled if provided).
- Spawn `$EDITOR` (or config editor, or `vim` fallback) on the temp file.
- Wait for editor to exit.
- Read the temp file, parse it as a fragment.
- Validate against schema.
- **If validation fails:** display the error and re-open the editor with the invalid content (don't delete the temp file). Loop until valid or user saves empty/unchanged content (abort signal).
- Write to `fragments/`, update index.
- Delete temp file.

**Acceptance criteria:**
- `parc new note "Test"` (positional) opens editor with title pre-filled (since built-in schemas have `editor_skip: false`).
- `parc new todo --title "Task" --due 2026-03-01 --priority high` opens editor with fields pre-filled.
- `--tag` flags are added to frontmatter.
- Without title, opens editor with empty template. Fragment created on save.
- If editor exits without changes (or empty title), abort with message.
- Invalid frontmatter re-opens editor with error message displayed.
- Fragment is indexed immediately after creation.

**Estimated effort:** 3–4 hours.

---

### T4.4 — `parc list`

Lists fragments from the index.

**Output (default table):**
```
ID        TYPE      STATUS    TITLE                           TAGS
01JQ7V3X  decision  accepted  Use SQLite for the search idx   architecture, search
01JQ7V4Y  note      —         Initial architecture thoughts   architecture
```

**Output (`--json`):**
```json
[
  {
    "id": "01JQ7V3XKP5GQZ2N8R6T1WBMVH",
    "type": "decision",
    "status": "accepted",
    "title": "Use SQLite for the search index",
    "tags": ["architecture", "search"]
  }
]
```

**Acceptance criteria:**
- Default output is a formatted table.
- `parc list todo` filters to todos.
- `parc list todo --status open` filters further.
- `parc list --tag arch` filters by tag.
- `--json` outputs valid JSON array.
- `--limit N` limits results.
- IDs are shown truncated to `config.id_display_length` (default 8).

**Estimated effort:** 2–3 hours.

---

### T4.5 — `parc show`

Renders a single fragment to the terminal.

- Uses `termimad` to render the Markdown body with syntax highlighting.
- Prepends a metadata header block (type, status, tags, dates, links).
- Accepts ID prefix.
- `--json` outputs the full fragment as JSON.

**Acceptance criteria:**
- Shows metadata header + rendered Markdown body.
- Tags section shows merged tags (frontmatter + inline).
- ID prefix resolution works.
- `--json` outputs complete fragment data.

**Estimated effort:** 2–3 hours.

---

### T4.6 — `parc edit`

Opens a fragment in `$EDITOR`.

**Flow:**
1. Resolve ID prefix → read fragment file.
2. Copy current content to a temp file.
3. Open `$EDITOR` on the temp file.
4. On save, parse the temp file.
5. Validate against schema.
6. Update `updated_at` timestamp.
7. Write back to `fragments/<id>.md`.
8. Re-index the fragment.
9. Delete temp file.

**Acceptance criteria:**
- Opens correct fragment in editor.
- Changes are persisted after editor closes.
- `updated_at` is refreshed.
- Index is updated.
- Invalid edits (broken frontmatter) display the error and re-open the editor with the invalid content. Loop until valid or user saves empty/unchanged content (abort signal).

**Estimated effort:** 2–3 hours.

---

### T4.7 — `parc set`

Updates a single metadata field without opening an editor.

```bash
parc set 01JQ7V3X status done
parc set 01JQ7V3X priority critical
parc set 01JQ7V3X title "New title"
```

**Flow:**
1. Resolve ID prefix → read fragment.
2. Validate the field name exists in envelope or schema.
3. Validate the value (e.g. enum check).
4. Update the field, set `updated_at`.
5. Write back, re-index.

**Acceptance criteria:**
- Setting a valid enum field works.
- Setting an invalid enum value produces a clear error.
- Setting `title` works.
- Setting `tags` works (comma-separated? Or require `--tag` on `new`?). **Decision: `set` handles single-value fields only. Use `edit` for tags and lists.** Tag management (`parc tag add/remove`) is deferred to a later milestone.
- `updated_at` is refreshed.
- Index is updated.

**Estimated effort:** 2 hours.

---

### T4.8 — `parc search`

Exposes the search module via CLI.

```bash
parc search "SQLite index"
parc search "SQLite" --type decision
parc search --tag architecture --status accepted
```

Uses the same table output format as `parc list`, with an additional snippet column for full-text matches.

**Acceptance criteria:**
- Full-text search works.
- Filters combinable.
- `--json` outputs results as JSON.
- No results produces a clean "No fragments found." message.

**Estimated effort:** 1–2 hours (mostly wiring, logic is in T3.2).

---

### T4.9 — `parc delete`

Soft-deletes a fragment.

**Flow:**
1. Resolve ID prefix.
2. Move `fragments/<id>.md` to `trash/<id>.md`.
3. Remove from index.

**Acceptance criteria:**
- File moves to trash.
- No longer appears in list/search.
- Deleting a non-existent ID produces a clear error.

**Estimated effort:** 1 hour.

---

### T4.10 — `parc reindex`

Rebuilds the entire index from files.

```
$ parc reindex
Reindexed 42 fragments.
```

**Acceptance criteria:**
- Clears and rebuilds all index tables.
- Handles corrupt/unparseable files gracefully (warns, skips).
- Prints count of indexed fragments.

**Estimated effort:** 1 hour (logic is in T3.1).

---

### T4.11 — `parc types`

Lists registered fragment types.

```
NAME      ALIAS  FIELDS
note      n      (none)
todo      t      status, due, priority, assignee
decision  d      status, deciders
risk      r      status, likelihood, impact, mitigation
idea      i      status
```

**Estimated effort:** 30 minutes.

---

## Phase 5: Integration & Testing

### T5.1 — Integration tests

End-to-end tests using `assert_cmd` and `tempfile`:

- `parc init --global` → creates vault structure.
- `parc new note --title "Test"` → creates fragment, prints ID.
- `parc list` → shows the fragment.
- `parc show <id>` → renders it.
- `parc set <id> title "Updated"` → changes title.
- `parc search "Test"` → finds it.
- `parc delete <id>` → moves to trash.
- `parc reindex` → rebuilds cleanly.
- Inline hashtag: create fragment with body containing `#myTag`, verify it appears in `parc list --tag myTag`.
- `parc search --tag myTag` finds it.

**Acceptance criteria:**
- All integration tests pass.
- Tests use isolated temp directories (not the real `~/.parc`).

**Estimated effort:** 4–5 hours.

---

## Suggested Implementation Order

| Order | Task | Depends on | Est. Hours |
|-------|------|------------|------------|
| 1 | T0.1 — Workspace setup | — | 1 |
| 2 | T0.2 — Error types | T0.1 | 0.5 |
| 3 | T1.1 — Vault module | T0.2 | 2.5 |
| 4 | T1.2 — Schema module | T0.2 | 3.5 |
| 5 | T1.3 — Built-in schemas | T1.2 | 1.5 |
| 6 | T1.4 — Config loading | T1.1 | 1.5 |
| 7 | T2.1 — Fragment model | T0.2 | 4.5 |
| 8 | T2.2 — Fragment validation + CRUD | T2.1 | 3.5 |
| 9 | T2.3 — Tag extraction | T2.1 | 3.5 |
| 10 | T3.1 — Index schema | T0.2 | 4.5 |
| 11 | T3.2 — Search module | T3.1 | 4.5 |
| 12 | T4.1 — CLI scaffolding | T0.1 | 2.5 |
| 13 | T4.2 — `init` command | T1.1, T4.1 | 0.5 |
| 14 | T4.3 — `new` command | T2.*, T1.2, T1.4, T3.1, T4.1 | 3.5 |
| 15 | T4.11 — `types` command | T1.2, T4.1 | 0.5 |
| 16 | T4.4 — `list` command | T3.2, T1.4, T4.1 | 2.5 |
| 17 | T4.5 — `show` command | T2.2, T2.3, T4.1 | 2.5 |
| 18 | T4.8 — `search` command | T3.2, T4.1 | 1.5 |
| 19 | T4.6 — `edit` command | T2.2, T3.1, T4.1 | 2.5 |
| 20 | T4.7 — `set` command | T2.2, T1.2, T3.1, T4.1 | 2 |
| 21 | T4.9 — `delete` command | T2.2, T3.1, T4.1 | 1 |
| 22 | T4.10 — `reindex` command | T3.1, T4.1 | 1 |
| 23 | T5.1 — Integration tests | all T4.* | 4.5 |
|    | **TOTAL** | | **~52 hours** |

---

## Definition of Done (M0)

- [ ] All commands functional: `init`, `new`, `list`, `show`, `edit`, `set`, `search`, `delete`, `reindex`, `types`
- [ ] Five built-in types with correct schemas and templates
- [ ] Full-text search works across all fragment types
- [ ] Inline `#hashtags` extracted and searchable
- [ ] ID prefix resolution works everywhere
- [ ] `--json` flag on `list`, `show`, `search`
- [ ] `$EDITOR` integration for `new` and `edit`
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] `cargo clippy` clean
- [ ] README with installation and basic usage
