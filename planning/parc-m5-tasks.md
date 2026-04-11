# M5 — History & Attachments

## Features

1. **Fragment version history** — Snapshot-on-save before edits, stored in `history/<id>/`
2. **`parc history`** — List, show, diff, and restore previous versions
3. **Attachment management** — Store, reference, and remove binary attachments
4. **`parc attach` / `parc detach` / `parc attachments`** — CLI for attachment operations
5. **`![[attach:...]]` syntax** — Attachment references in Markdown body
6. **`parc doctor` extensions** — Attachment mismatches, vault size warnings

**Already done (skip):** Vault `init_vault()` already creates `history/` and `attachments/` directories.

---

## Feature 1: Fragment Version History (Core)

### Files
- `parc-core/src/history.rs` — **New.** `save_snapshot()`, `list_versions()`, `read_version()`, `restore_version()`
- `parc-core/src/lib.rs` — `pub mod history;`
- `parc-core/src/fragment.rs` — Call `save_snapshot()` before overwrite in `write_fragment()`
- `parc-core/src/config.rs` — Read `history.enabled` from config (default: true)

### Design

Before `write_fragment()` overwrites a file, the current file is copied to `history/<fragment-id>/<updated_at>.md`. The timestamp in the filename is the `updated_at` of the version being superseded. Each history file is a full snapshot (frontmatter + body).

History is opt-out via `config.yml`:
```yaml
history:
  enabled: true
```

### Tasks
- [ ] Create `history.rs` module with `save_snapshot(vault, fragment_id) -> Result<()>`
  - Read the current fragment file from `fragments/<id>.md`
  - Parse its `updated_at` for the snapshot filename
  - Copy to `history/<id>/<updated_at>.md`, creating the subdirectory if needed
  - If the fragment file doesn't exist yet (create path), skip silently
- [ ] Add `list_versions(vault, fragment_id) -> Result<Vec<VersionEntry>>` returning timestamps sorted newest-first
  - `VersionEntry { timestamp: String, path: PathBuf, size: u64 }`
- [ ] Add `read_version(vault, fragment_id, timestamp) -> Result<Fragment>` to parse a specific snapshot
- [ ] Add `restore_version(vault, fragment_id, timestamp) -> Result<Fragment>` that:
  - Saves a snapshot of the current version first (so restore is itself reversible)
  - Reads the target version, sets `updated_at` to now
  - Writes it as the current fragment via `write_fragment()`
  - Re-indexes
- [ ] Wire `save_snapshot()` into `write_fragment()` — call it before the `fs::write` overwrite
  - Check `config.history.enabled` (default true) before saving
- [ ] Register `pub mod history;` in `lib.rs`
- [ ] Unit tests: snapshot creation, list versions, read version, restore version

---

## Feature 2: `parc history` CLI Command

### Files
- `parc-cli/src/commands/history.rs` — **New.** Command handler
- `parc-cli/src/commands/mod.rs` — `pub mod history;`
- `parc-cli/src/main.rs` — `History` command variant with subcommand flags
- `parc-core/Cargo.toml` — Add `similar` dependency (for diffing)
- `parc-core/src/history.rs` — `diff_versions()` function

### CLI Interface

```
parc history <id>                        # list versions with timestamps
parc history <id> --show <timestamp>     # display a specific version
parc history <id> --diff [timestamp]     # diff current vs. previous (or specific)
parc history <id> --restore <timestamp>  # restore a previous version
```

### Tasks
- [ ] Add `similar` crate to `parc-core/Cargo.toml`
- [ ] Add `diff_versions(vault, fragment_id, timestamp) -> Result<String>` to `history.rs`
  - If timestamp provided: diff current vs that version
  - If no timestamp: diff current vs most recent snapshot
  - Use `similar` crate for unified diff output
- [ ] Add `History` command to clap in `main.rs`:
  ```
  History {
      id: String,
      #[arg(long)] show: Option<String>,
      #[arg(long)] diff: Option<Option<String>>,
      #[arg(long)] restore: Option<String>,
  }
  ```
- [ ] Implement list mode (default): table with timestamp, size, age
- [ ] Implement `--show`: render the historical version with `termimad`
- [ ] Implement `--diff`: display unified diff with `+`/`-` coloring
- [ ] Implement `--restore`: call `restore_version()`, print confirmation
- [ ] Register module in `mod.rs`
- [ ] Integration tests:
  - Create fragment, edit it, verify `parc history <id>` lists versions
  - Verify `--show` displays old content
  - Verify `--diff` shows changes
  - Verify `--restore` brings back old version and creates new snapshot

---

## Feature 3: Attachment Management (Core)

### Files
- `parc-core/src/attachment.rs` — **New.** `attach_file()`, `detach_file()`, `list_attachments()`
- `parc-core/src/lib.rs` — `pub mod attachment;`
- `parc-core/src/fragment.rs` — Add `attachments: Vec<String>` field to `Fragment`, handle serialization
- `parc-core/src/index.rs` — Index `attachments` field (for `has:attachments` filter)

### Design

Attachments are stored in `attachments/<fragment-id>/`. The fragment's frontmatter lists attachment filenames:
```yaml
attachments:
  - screenshot.png
  - spec.pdf
```

The `Fragment` struct gains `pub attachments: Vec<String>` (default empty vec, skip serializing if empty).

### Tasks
- [ ] Add `pub attachments: Vec<String>` to `Fragment` struct
  - Default to empty vec
  - `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
- [ ] Update `serialize_fragment()` and `parse_fragment()` to handle `attachments` field
- [ ] Create `attachment.rs` module with:
  - `attach_file(vault, fragment_id, source_path, move_file: bool) -> Result<String>`
    - Resolve fragment by prefix
    - Create `attachments/<full-id>/` directory
    - Copy (or move if `move_file`) the file into it
    - Add filename to fragment's `attachments` list (save snapshot first, then write)
    - Re-index
    - Return the filename
  - `detach_file(vault, fragment_id, filename) -> Result<()>`
    - Remove file from `attachments/<id>/`
    - Remove filename from fragment's `attachments` list (save snapshot, write)
    - Re-index
    - Remove empty attachment directory
  - `list_attachments(vault, fragment_id) -> Result<Vec<AttachmentInfo>>`
    - `AttachmentInfo { filename: String, size: u64, path: PathBuf }`
    - List files in `attachments/<id>/`
- [ ] Validate filename uniqueness within a fragment's attachment directory
- [ ] Register `pub mod attachment;` in `lib.rs`
- [ ] Update index schema to store attachment count or `has_attachments` flag for `has:attachments` filter
- [ ] Unit tests: attach, detach, list, duplicate filename rejection, move mode

---

## Feature 4: Attachment CLI Commands

### Files
- `parc-cli/src/commands/attach.rs` — **New.** Handles `attach`, `detach`, `attachments`
- `parc-cli/src/commands/mod.rs` — `pub mod attach;`
- `parc-cli/src/main.rs` — `Attach`, `Detach`, `Attachments` command variants

### CLI Interface

```
parc attach <id> <file-path>             # copy file into attachment dir
parc attach <id> <file-path> --mv        # move instead of copy
parc attachments <id>                    # list attachments for a fragment
parc detach <id> <filename>              # remove an attachment
```

### Tasks
- [ ] Add `Attach`, `Detach`, `Attachments` commands to clap in `main.rs`:
  ```
  Attach { id: String, file: PathBuf, #[arg(long)] mv: bool }
  Detach { id: String, filename: String }
  Attachments { id: String }
  ```
- [ ] Implement `attach` handler: validate file exists, call `attach_file()`, print result
- [ ] Implement `detach` handler: call `detach_file()`, print confirmation
- [ ] Implement `attachments` handler: call `list_attachments()`, display table (filename, size)
- [ ] Register module in `mod.rs`
- [ ] Integration tests:
  - Attach a file, verify it appears in `parc attachments <id>`
  - Attach with `--mv`, verify source is removed
  - Detach, verify file is removed and frontmatter updated
  - Attach duplicate filename, verify error

---

## Feature 5: `![[attach:...]]` Syntax Parsing

### Files
- `parc-core/src/attachment.rs` — `parse_attachment_refs(body) -> Vec<AttachmentRef>`
- `parc-core/src/doctor.rs` — Use parsed refs for validation

### Design

Body text can reference attachments with `![[attach:filename]]` or `![[attach:filename|display text]]`. This is parsed for doctor validation (not for rendering — rendering is display-layer concern).

### Tasks
- [ ] Add `parse_attachment_refs(body: &str) -> Vec<AttachmentRef>` to `attachment.rs`
  - `AttachmentRef { filename: String, display_text: Option<String> }`
  - Regex: `!\[\[attach:([^\]|]+)(?:\|([^\]]+))?\]\]`
  - Ignore refs inside fenced code blocks and inline code
- [ ] Unit tests: parse various `![[attach:...]]` patterns, ignore code blocks

---

## Feature 6: `parc doctor` Extensions

### Files
- `parc-core/src/doctor.rs` — New checks: `check_attachments()`, `check_vault_size()`

### Design

Two new doctor checks:
1. **Attachment mismatches**: files on disk not in frontmatter, frontmatter entries with no file, `![[attach:]]` refs with no corresponding file
2. **Vault size warning**: total vault size exceeds configurable threshold (default: warn at 500MB)

### Tasks
- [ ] Add `DoctorFinding::AttachmentMismatch { fragment_id, detail: String }` variant
- [ ] Add `DoctorFinding::VaultSizeWarning { total_bytes: u64 }` variant
- [ ] Implement `check_attachments(vault, fragments)`:
  - For each fragment with `attachments` field: verify each filename exists in `attachments/<id>/`
  - For each fragment: parse body for `![[attach:]]` refs, verify those files exist
  - Scan `attachments/` directory for files not referenced by any fragment
- [ ] Implement `check_vault_size(vault)`:
  - Walk vault directory, sum file sizes
  - Warn if total exceeds threshold (500MB default, configurable in config.yml)
- [ ] Wire new checks into `run_doctor()`
- [ ] Unit tests: missing attachment file, unreferenced attachment, body ref mismatch

---

## Feature 7: `parc show` — Display Attachments

### Files
- `parc-cli/src/commands/show.rs` — Append attachments section to output

### Tasks
- [ ] After backlinks section, if fragment has attachments, render an "Attachments" section listing filenames with sizes
- [ ] Integration test: show fragment with attachments includes attachment listing

---

## Implementation Order

1. **Feature 1** (history core) — foundation, modifies `write_fragment` flow
2. **Feature 2** (history CLI) — depends on Feature 1
3. **Feature 3** (attachment core) — independent of history, adds `attachments` to `Fragment`
4. **Feature 5** (`![[attach:]]` parsing) — small, needed by Feature 6
5. **Feature 4** (attachment CLI) — depends on Feature 3
6. **Feature 7** (show attachments) — depends on Feature 3
7. **Feature 6** (doctor extensions) — depends on Features 3 and 5

---

## Verification

```bash
cargo build
cargo test -p parc-core
cargo test -p parc-cli

# History
parc new note --title "History test"
# note the ID
parc edit <id>    # make changes
parc history <id>                    # should list 1 version
parc history <id> --show <ts>        # should show old content
parc history <id> --diff             # should show diff
parc history <id> --restore <ts>     # should restore, creating new snapshot
parc history <id>                    # should now list 2 versions

# Attachments
parc new note --title "Attach test"
parc attach <id> /path/to/file.png
parc attachments <id>                # should list file.png
parc show <id>                       # should show Attachments section
parc detach <id> file.png
parc attachments <id>                # should be empty

# Attach with move
cp /tmp/test.txt /tmp/move-test.txt
parc attach <id> /tmp/move-test.txt --mv
ls /tmp/move-test.txt                # should not exist

# Doctor
parc doctor                          # should check attachment mismatches
```
