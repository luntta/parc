# parc M1 — Implementation Task Breakdown

**Goal:** Wiki-link parsing, bidirectional link management, backlink queries, `show` with backlinks section, and `parc doctor` for vault health checks.

**Prerequisite:** M0 complete (vault, fragments, index, search, all CLI commands).

---

## Phase 0: Wiki-Link Parsing

### T0.1 — Wiki-link parser (`link.rs`)

Create `parc-core/src/link.rs` — a module that extracts wiki-links from Markdown body content.

**Data structures:**

```rust
pub struct WikiLink {
    pub target: String,               // ID or ID prefix
    pub display_text: Option<String>, // text after `|`, if present
}
```

**Functions:**

```rust
/// Parse wiki-links from Markdown body, ignoring code blocks and inline code.
/// Supports `[[id-prefix]]` and `[[id-prefix|display text]]`.
pub fn parse_wiki_links(body: &str) -> Vec<WikiLink>
```

**Parsing approach:**

Use `comrak` to parse the Markdown AST. Walk text nodes only (skip fenced code blocks, inline code). Apply regex `\[\[([^\]\|]+)(?:\|([^\]]+))?\]\]` to extract target and optional display text from text content.

**Edge cases:**
- `[[01JQ7V4Y]]` — simple link, no display text.
- `[[01JQ7V4Y|Decision about SQLite]]` — link with display text.
- `[[01JQ7V4Y]]` inside a fenced code block — ignored.
- `[[01JQ7V4Y]]` inside inline code — ignored.
- `[[]]` — empty target — ignored.
- Multiple links in the same text node — all extracted.
- Nested brackets `[[[foo]]]` — extract `[foo]` as target (match innermost valid pair).

**Acceptance criteria:**
- Extracts simple `[[id]]` links.
- Extracts `[[id|text]]` links with display text.
- Ignores links inside fenced code blocks.
- Ignores links inside inline code.
- Ignores empty `[[]]`.
- Returns deduplicated links (same target appearing twice → one entry).

**Estimated effort:** 2–3 hours.

---

## Phase 1: Link Indexing

### T1.1 — Integrate wiki-link extraction into indexing

Update `index_fragment()` to merge frontmatter `links:` with body `[[...]]` wiki-links before inserting into `fragment_links`.

**Changes to `index.rs`:**

```rust
/// Index a single fragment (upsert into all tables).
/// Now also extracts wiki-links from body and merges with frontmatter links.
pub fn index_fragment(
    conn: &Connection,
    fragment: &Fragment,
    merged_tags: &[String],
    all_ids: &[String],       // all known fragment IDs, for prefix resolution
) -> Result<(), ParcError>
```

**Merge logic:**
1. Start with `fragment.links` (frontmatter — already full ULIDs).
2. Call `parse_wiki_links(&fragment.body)` to get body links.
3. For each body link target, resolve the prefix against `all_ids` (best-effort: if ambiguous or not found, skip silently — `parc doctor` will catch it later).
4. Deduplicate the combined set.
5. Insert into `fragment_links`.

**Changes to `reindex()`:**
- Load all fragment IDs first (single pass over `fragments/` directory).
- Pass `all_ids` to each `index_fragment()` call.

**Acceptance criteria:**
- A fragment with `links: [01JQ7V4Y]` in frontmatter AND `[[01JQ7V3X]]` in body → both appear in `fragment_links`.
- Body link prefixes are resolved to full ULIDs.
- Unresolvable body link prefixes are silently skipped (not indexed, not an error).
- `reindex` handles the merged link set correctly.

**Estimated effort:** 2–3 hours.

---

### T1.2 — Backlink query function

Add a function to query backlinks (fragments that link *to* a given target).

**Functions (in `index.rs` or `link.rs`):**

```rust
pub struct BacklinkInfo {
    pub source_id: String,
    pub source_type: String,
    pub source_title: String,
}

/// Find all fragments that link to the given target ID.
pub fn get_backlinks(conn: &Connection, target_id: &str) -> Result<Vec<BacklinkInfo>, ParcError>
```

**SQL:**

```sql
SELECT f.id, f.type, f.title
FROM fragment_links fl
JOIN fragments f ON f.id = fl.source_id
WHERE fl.target_id = ?1
ORDER BY f.updated_at DESC
```

**Acceptance criteria:**
- If A links to B, `get_backlinks(B)` returns A.
- If A links to B and C links to B, `get_backlinks(B)` returns both A and C.
- If nothing links to B, returns empty vec.
- Results include type and title for display.

**Estimated effort:** 1 hour.

---

## Phase 2: Link Management Commands

### T2.1 — `parc link <id-a> <id-b>`

Creates a bidirectional link between two fragments by modifying both files' frontmatter `links:` fields.

**Flow:**
1. Resolve both ID prefixes to full ULIDs.
2. Read fragment A — add B's full ID to `links:` if not already present.
3. Read fragment B — add A's full ID to `links:` if not already present.
4. Update `updated_at` on both fragments.
5. Write both files.
6. Re-index both fragments.
7. Print confirmation: `Linked 01JQ7V3X ↔ 01JQ7V4Y`

**Edge cases:**
- Linking to self → error: "Cannot link a fragment to itself."
- Already linked → no-op with message: "Already linked."
- One of the IDs not found → `FragmentNotFound` error.
- Ambiguous prefix → `AmbiguousId` error.

**CLI definition:**

```rust
Link {
    id_a: String,
    id_b: String,
}
```

**Acceptance criteria:**
- Both files updated with each other's ID in frontmatter `links:`.
- Both fragments re-indexed.
- `updated_at` refreshed on both.
- Self-link rejected.
- Already-linked is idempotent.

**Estimated effort:** 2–3 hours.

---

### T2.2 — `parc unlink <id-a> <id-b>`

Removes a bidirectional link between two fragments.

**Flow:**
1. Resolve both ID prefixes.
2. Read fragment A — remove B's ID from `links:`.
3. Read fragment B — remove A's ID from `links:`.
4. Update `updated_at` on both.
5. Write both files.
6. Re-index both.
7. Print confirmation: `Unlinked 01JQ7V3X ↔ 01JQ7V4Y`

**Edge cases:**
- Not currently linked → no-op with message: "Not linked."

**Note:** `unlink` only removes frontmatter links. If the body contains `[[id]]` wiki-links, those remain (the user must edit the body manually). This is consistent with files-as-source-of-truth — `unlink` manages the structured metadata, not prose.

**CLI definition:**

```rust
Unlink {
    id_a: String,
    id_b: String,
}
```

**Acceptance criteria:**
- Both files updated — IDs removed from `links:`.
- Both fragments re-indexed.
- Not-linked case handled gracefully.
- Body `[[id]]` links are untouched.

**Estimated effort:** 1–2 hours.

---

### T2.3 — `parc backlinks <id>`

Queries and displays all fragments that link to the given fragment.

**Flow:**
1. Resolve ID prefix.
2. Open index, call `get_backlinks()`.
3. Display results as a table.

**Output (default table):**
```
BACKLINKS TO 01JQ7V3X "Use SQLite for the search index"

ID        TYPE      TITLE
01JQ7V4Y  note      Initial architecture thoughts
01JQAB12  risk      SQLite scalability concerns
```

**Output (`--json`):**
```json
[
  {
    "id": "01JQ7V4YKP5GQZ2N8R6T1WBMVH",
    "type": "note",
    "title": "Initial architecture thoughts"
  }
]
```

**CLI definition:**

```rust
Backlinks {
    id: String,
    #[arg(long)]
    json: bool,
}
```

**Acceptance criteria:**
- Displays backlinks table with ID, type, title.
- `--json` outputs valid JSON array.
- No backlinks → "No backlinks found."
- Header shows target fragment's short ID and title for context.

**Estimated effort:** 1–2 hours.

---

## Phase 3: Show Backlinks

### T3.1 — Update `parc show` to display backlinks

Enhance the `show` command to query and display a backlinks section after the body.

**Changes to `show` command (`parc-cli`):**
1. After reading the fragment, open the index.
2. Call `get_backlinks(conn, fragment.id)`.
3. Pass the backlink list to the renderer.

**Changes to rendering:**
- After the body, if backlinks are non-empty, render a section:

```
─── Backlinks ──────────────────────────────
  01JQ7V4Y  note  Initial architecture thoughts
  01JQAB12  risk  SQLite scalability concerns
```

- If no backlinks, omit the section entirely (no "No backlinks" noise).

**Changes to `--json` output:**
- Add a `backlinks` field to the JSON output:

```json
{
  "id": "...",
  "type": "...",
  "title": "...",
  "backlinks": [
    { "id": "01JQ7V4Y...", "type": "note", "title": "..." }
  ]
}
```

**Acceptance criteria:**
- `parc show <id>` renders backlinks section after the body.
- Backlinks section omitted when empty.
- `--json` includes backlinks array.
- Backlink IDs shown truncated to `config.id_display_length`.

**Estimated effort:** 2–3 hours.

---

## Phase 4: Doctor

### T4.1 — Doctor module (`doctor.rs`)

Create `parc-core/src/doctor.rs` — vault health checks that scan for common issues.

**Data structures:**

```rust
#[derive(Debug)]
pub enum DoctorFinding {
    BrokenLink {
        source_id: String,
        source_title: String,
        target_ref: String,     // the unresolvable ID/prefix
    },
    OrphanFragment {
        id: String,
        title: String,
    },
    SchemaViolation {
        id: String,
        title: String,
        message: String,        // e.g. "missing required field: status"
    },
}

#[derive(Debug)]
pub struct DoctorReport {
    pub findings: Vec<DoctorFinding>,
    pub fragments_checked: usize,
}

impl DoctorReport {
    pub fn is_healthy(&self) -> bool { self.findings.is_empty() }
}
```

**Check functions:**

```rust
/// Check for broken links: frontmatter links and body [[...]] links
/// that reference non-existent fragment IDs.
pub fn check_broken_links(
    vault: &Path,
    fragments: &[Fragment],
) -> Result<Vec<DoctorFinding>, ParcError>

/// Check for orphan fragments: fragments with no inbound or outbound links
/// (neither frontmatter links, body wiki-links, nor backlinks from other fragments).
pub fn check_orphans(
    vault: &Path,
    fragments: &[Fragment],
) -> Result<Vec<DoctorFinding>, ParcError>

/// Check for schema violations: fragments whose extra_fields don't match
/// their type's schema (missing required fields, invalid enum values, etc.).
pub fn check_schema_violations(
    vault: &Path,
    fragments: &[Fragment],
    schemas: &SchemaRegistry,
) -> Result<Vec<DoctorFinding>, ParcError>

/// Run all checks and return a combined report.
pub fn run_doctor(vault: &Path) -> Result<DoctorReport, ParcError>
```

**Broken link detection:**
1. Collect all fragment IDs into a set.
2. For each fragment, check frontmatter `links:` — each must resolve to a known ID.
3. For each fragment, parse body for `[[...]]` wiki-links — each target must resolve to a known ID.
4. Report each unresolvable reference as a `BrokenLink`.

**Orphan detection:**
1. Build a set of all IDs that appear as either source or target in any link (frontmatter + body wiki-links).
2. Any fragment whose ID is NOT in this set is an orphan.
3. **Note:** Orphans are informational, not errors. Fragments may intentionally have no links.

**Schema violation detection:**
1. For each fragment, look up its type's schema.
2. Run validation (reuse `validate_fragment` from M0).
3. Collect validation errors as findings.

**Acceptance criteria:**
- Broken links detected for both frontmatter and body links.
- Orphan fragments identified correctly.
- Schema violations caught (missing required fields, invalid enum values).
- `run_doctor` aggregates all checks.
- `DoctorReport` indicates overall health.

**Estimated effort:** 3–4 hours.

---

### T4.2 — `parc doctor` CLI command

Expose the doctor module as a CLI command.

**Output:**
```
Checking vault health...

✗ Broken link: 01JQ7V3X "Use SQLite" → 01JQZZZZ (not found)
✗ Broken link: 01JQ7V4Y "Architecture" → [[01JQAAAA]] (not found)
! Orphan: 01JQAB12 "Random thought" (no links in or out)
✗ Schema violation: 01JQ7V3X "Use SQLite" — missing required field: status

Checked 42 fragments: 3 issues found.
```

```
Checking vault health...

Checked 42 fragments: no issues found. ✓
```

**CLI definition:**

```rust
Doctor,
```

**Behavior:**
- Exit code 0 if healthy, exit code 1 if issues found.
- Findings grouped by severity: schema violations and broken links first (errors), orphans last (warnings).
- `--json` outputs the full `DoctorReport` as JSON.

**Acceptance criteria:**
- Displays findings in a clear, scannable format.
- Exit code 1 when issues are found.
- Exit code 0 when healthy.
- `--json` outputs structured report.
- Orphans shown as warnings (not errors).

**Estimated effort:** 1–2 hours.

---

## Phase 5: Tests

### T5.1 — Unit and integration tests

**Unit tests (`parc-core`):**

Wiki-link parsing:
- Simple `[[id]]` extraction.
- `[[id|display text]]` extraction.
- Multiple links in one body.
- Links inside fenced code blocks → ignored.
- Links inside inline code → ignored.
- Empty `[[]]` → ignored.
- Deduplication of same target.

Backlink queries:
- A links to B → `get_backlinks(B)` returns A.
- Multiple backlinks.
- No backlinks → empty.

Link merge logic:
- Frontmatter links + body links deduplicated.
- Unresolvable body link prefix → skipped.

Doctor:
- Broken link detection (frontmatter and body).
- Orphan detection.
- Schema violation detection.
- Clean vault → healthy report.

**Integration tests (`parc-cli`):**

```bash
# Link round-trip
parc init --global
parc new note --title "Note A"    # → ID_A
parc new note --title "Note B"    # → ID_B
parc link ID_A ID_B
parc show ID_A                    # → shows B in metadata links
parc show ID_B                    # → shows A in backlinks section
parc backlinks ID_B               # → lists A

# Unlink
parc unlink ID_A ID_B
parc backlinks ID_B               # → empty

# Doctor — clean vault
parc doctor                       # → exit 0

# Doctor — broken link
# (manually create a fragment with a bogus link in frontmatter)
parc doctor                       # → exit 1, reports broken link

# Body wiki-links
parc new note --title "Note C"    # → ID_C
parc edit ID_C                    # add [[ID_A]] in body
parc backlinks ID_A               # → lists C

# Show with backlinks
parc show ID_A                    # → backlinks section shows C
parc show ID_A --json             # → backlinks array in JSON
```

**Acceptance criteria:**
- All unit tests pass.
- All integration tests pass.
- Tests use isolated temp directories.
- `cargo clippy` clean after all changes.

**Estimated effort:** 4–5 hours.

---

## Suggested Implementation Order

| Order | Task | Depends on | Est. Hours |
|-------|------|------------|------------|
| 1 | T0.1 — Wiki-link parser | M0 | 2.5 |
| 2 | T1.1 — Link indexing integration | T0.1 | 2.5 |
| 3 | T1.2 — Backlink query function | T1.1 | 1 |
| 4 | T2.1 — `link` command | T1.2 | 2.5 |
| 5 | T2.2 — `unlink` command | T2.1 | 1.5 |
| 6 | T2.3 — `backlinks` command | T1.2 | 1.5 |
| 7 | T3.1 — Show backlinks | T1.2 | 2.5 |
| 8 | T4.1 — Doctor module | T0.1, T1.1 | 3.5 |
| 9 | T4.2 — `doctor` CLI command | T4.1 | 1.5 |
| 10 | T5.1 — Tests | all above | 4.5 |
|    | **TOTAL** | | **~24 hours** |

---

## Key Design Decisions

1. **Link storage**: `parc link` writes to frontmatter `links:` (files are the source of truth). Body `[[...]]` wiki-links are additive at index time — they are not written back to frontmatter.

2. **Bidirectionality**: `parc link` modifies both fragment files (A→B and B→A in frontmatter). Body `[[...]]` links are unidirectional — only the authoring fragment's file contains them. Backlinks from body links are surfaced through the index.

3. **Prefix normalization**: At index time, `[[01JQ7V4Y]]` in a body is resolved to the full ULID and stored in `fragment_links.target_id`. This makes backlink queries exact-match lookups. Unresolvable prefixes are silently skipped (doctor catches them).

4. **Doctor scope for M1**: Broken links (frontmatter + body), orphan fragments (no links in or out), schema violations (reusing M0 validation). Attachment checks deferred to M5.

5. **Forward compatibility**: The `linked:` search filter is deferred to M3 (Search DSL milestone). The `fragment_links` table is already in place from M0, so M1 just needs to populate it more completely and query it.

6. **Unlink scope**: `parc unlink` only removes frontmatter `links:` entries. Body `[[...]]` wiki-links must be removed by editing the fragment. This keeps the command simple and predictable — structured metadata vs. prose are separate concerns.

---

## Definition of Done (M1)

- [ ] Wiki-link parser extracts `[[id]]` and `[[id|text]]` from Markdown bodies
- [ ] Index merges frontmatter links + body wiki-links, resolving prefixes to full ULIDs
- [ ] `parc link` creates bidirectional frontmatter links
- [ ] `parc unlink` removes bidirectional frontmatter links
- [ ] `parc backlinks` displays linking fragments
- [ ] `parc show` renders a backlinks section after body
- [ ] `parc doctor` reports broken links, orphans, and schema violations
- [ ] `parc doctor` exits nonzero when issues found
- [ ] All commands support `--json` where applicable
- [ ] All unit tests pass
- [ ] All integration tests pass
- [ ] `cargo clippy` clean
