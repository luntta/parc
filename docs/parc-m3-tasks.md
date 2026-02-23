# parc M3 — Implementation Task Breakdown

**Goal:** Search DSL — replace separate `--type`, `--status`, `--tag` CLI flags with a unified query DSL where all filters, full-text terms, and hashtags are expressed in a single query string.

**Prerequisite:** M2 complete (multi-vault support).

---

## Phase 0: AST & Parser

### T0.1 — Define SearchQuery AST types in `parc-core/src/search.rs`

Add the type definitions that represent a parsed search query.

**Types:**

```rust
pub enum TextTerm {
    Word(String),       // bare word → FTS match
    Phrase(String),     // "quoted phrase" → FTS exact match
}

pub enum CompareOp { Eq, Lt, Gt, Lte, Gte }

pub enum RelativeDate {
    Today, Yesterday, Tomorrow,
    ThisWeek, LastWeek, ThisMonth, LastMonth,
    Overdue,           // due: only — anything before today
    DaysAgo(u32),      // e.g. 30-days-ago
}

pub enum DateFilter {
    Relative(RelativeDate),
    Absolute { op: CompareOp, date: String },
}

pub enum HasCondition { Attachments, Links, Due }

pub enum Filter {
    Type { value: String, negated: bool },
    Status { value: String, negated: bool },
    Priority { op: CompareOp, value: String, negated: bool },
    Tag { value: String, negated: bool },
    Due(DateFilter),
    Created(DateFilter),
    Updated(DateFilter),
    CreatedBy { value: String, negated: bool },
    Has(HasCondition),
    Linked(String),     // ID prefix
}

pub struct SearchQuery {
    pub text_terms: Vec<TextTerm>,
    pub filters: Vec<Filter>,
    pub sort: SortOrder,
    pub limit: Option<usize>,
}
```

**Acceptance criteria:**
- All types derive `Debug`, `Clone`, `PartialEq`, `Eq` (for testability).
- `SearchQuery` implements `Default` (empty query).

**Estimated effort:** 0.5 hours.

---

### T0.2 — Add `ParseError` variant to `ParcError`

Add a new error variant for DSL parse failures.

```rust
#[error("parse error: {0}")]
ParseError(String),
```

**Files to change:** `parc-core/src/error.rs`

**Estimated effort:** 5 minutes.

---

### T0.3 — Implement DSL parser — `parse_query(input: &str) -> Result<SearchQuery, ParcError>`

Hand-written tokenizer (no parser combinator dependency — the grammar is simple enough).

**Parsing rules:**
1. Iterate through input, consuming tokens:
   - `"..."` → `TextTerm::Phrase(contents)`
   - `#word` → `Filter::Tag { value: word, negated: false }`
   - `field:value` where field is a known filter name → parse as `Filter`
   - anything else → `TextTerm::Word`
2. Known filter fields: `type`, `status`, `priority`, `tag`, `due`, `created`, `updated`, `by`, `has`, `linked`
3. Negation: `!` prefix on value (e.g. `status:!done`)
4. Comparison operators on priority: `>=`, `<=`, `>`, `<` prefix on value (e.g. `priority:>=medium`)
5. Date values: comparison operators (`>2026-01-01`), relative shorthands (`today`, `this-week`, `overdue`, `N-days-ago`), or absolute ISO dates (`2026-03-01`)
6. `has:` accepts `attachments`, `links`, `due` — returns `ParseError` for unknown values
7. Tags are lowercased on parse (case-insensitive matching)

**Helper functions:**
- `consume_word(chars) -> String` — consumes until whitespace
- `parse_filter(field, value) -> Result<Filter>` — dispatches by field name
- `parse_negation(value) -> (bool, &str)` — strips `!` prefix
- `parse_compare_value(value) -> (CompareOp, bool, &str)` — strips operator prefix
- `parse_date_filter(value) -> Result<DateFilter>` — handles relative/absolute dates
- `parse_relative_date(value) -> Option<RelativeDate>` — matches known shorthands
- `validate_date(value) -> Result<()>` — validates ISO date format

**Acceptance criteria:**
- `parse_query("")` → empty `SearchQuery`
- `parse_query("hello world")` → two `TextTerm::Word`s
- `parse_query("\"exact match\"")` → one `TextTerm::Phrase`
- `parse_query("type:todo")` → one `Filter::Type`
- `parse_query("status:!done")` → `Filter::Status { negated: true }`
- `parse_query("#backend")` → `Filter::Tag { value: "backend" }`
- `parse_query("due:today")` → `Filter::Due(DateFilter::Relative(Today))`
- `parse_query("created:>2026-01-01")` → `Filter::Created(DateFilter::Absolute { op: Gt })`
- `parse_query("priority:>=medium")` → `Filter::Priority { op: Gte }`
- `parse_query("has:links")` → `Filter::Has(Links)`
- `parse_query("linked:01JQ7V3X")` → `Filter::Linked("01JQ7V3X")`
- `parse_query("type:todo status:open #backend API")` → 3 filters + 1 text term

**Estimated effort:** 2–3 hours.

---

## Phase 1: Date Resolution

### T1.1 — Implement `resolve_relative_date(rel: &RelativeDate) -> (String, String)`

Returns a date range as ISO date strings (start, end). For point dates, start == end.

**Resolution logic:**
- `Today` → today's date
- `Yesterday` → today − 1 day
- `Tomorrow` → today + 1 day
- `ThisWeek` → Monday..Sunday of current week
- `LastWeek` → Monday..Sunday of previous week
- `ThisMonth` → 1st..last day of current month
- `LastMonth` → 1st..last day of previous month
- `Overdue` → `0000-01-01`..yesterday (everything before today)
- `DaysAgo(n)` → today − n days

Uses `chrono::Local::now()` for "today" reference.

**Acceptance criteria:**
- Each shorthand resolves to correct ISO date strings.
- Range shorthands return different start/end dates.
- Point shorthands return same start and end.

**Estimated effort:** 1 hour.

---

## Phase 2: SQL Compiler

### T2.1 — Implement `compile_query(query: &SearchQuery) -> Result<CompiledQuery, ParcError>`

Transforms a `SearchQuery` AST into parameterized SQL.

```rust
struct CompiledQuery {
    sql: String,
    params: Vec<Box<dyn rusqlite::types::ToSql>>,
}
```

**Compilation rules:**

| AST node | SQL output |
|----------|-----------|
| `TextTerm::Word` / `Phrase` | Concatenated into FTS5 MATCH clause (phrases in quotes) |
| `Filter::Type` / `Status` | `WHERE f.column = ?` (or `!=` for negated) |
| `Filter::Priority` with comparison | `WHERE f.priority IN (...)` using priority ordering |
| `Filter::Tag` (positive) | `JOIN fragment_tags ftN ON ftN.fragment_id = f.id AND ftN.tag = ?` (one JOIN per tag for AND semantics) |
| `Filter::Tag` (negated) | `NOT EXISTS (SELECT 1 FROM fragment_tags WHERE fragment_id = f.id AND tag = ?)` |
| `Filter::Due/Created/Updated` (relative, range) | `WHERE col >= ? AND col <= ?` |
| `Filter::Due/Created/Updated` (relative, point) | `WHERE col = ?` |
| `Filter::Due/Created/Updated` (absolute) | `WHERE col <op> ?` |
| `Filter::CreatedBy` | `WHERE f.created_by = ?` (or `!=`) |
| `Filter::Has(Links)` | `EXISTS (SELECT 1 FROM fragment_links WHERE source_id = f.id)` |
| `Filter::Has(Due)` | `f.due IS NOT NULL` |
| `Filter::Has(Attachments)` | Post-filter (deferred to M5 when attachments exist) |
| `Filter::Linked(prefix)` | `EXISTS (... fragment_links ... LIKE ?)` with prefix matching (both directions) |

**Priority ordering:**
`none < low < medium < high < critical`. For `priority:>=medium`, resolve to `IN ('medium', 'high', 'critical')`.

**FTS5 + tag JOIN compatibility:**
`snippet()` function cannot be used when additional JOINs beyond the FTS table are present. When both FTS text terms and positive tag filters exist, use `NULL as snippet` instead.

**Created/Updated date columns:**
These are stored as full ISO timestamps (RFC 3339). Use `substr(f.created_at, 1, 10)` to extract the date portion for comparison.

**Acceptance criteria:**
- FTS-only queries produce `MATCH` clause with snippet.
- Filter-only queries produce correct `WHERE` clauses.
- Combined FTS + filters produce correct SQL.
- Multiple positive tags produce multiple JOINs with `GROUP BY`.
- Negated tags use `NOT EXISTS` subquery.
- Priority comparisons expand to `IN (...)` with correct priority set.
- Date ranges produce `BETWEEN`-style conditions.

**Estimated effort:** 3–4 hours.

---

### T2.2 — Update `search()` function signature

Replace the old `search(conn, &SearchParams)` with:

```rust
pub fn search(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchResult>, ParcError>
```

Remove the `SearchParams` struct entirely (the CLI is the only consumer).

**Acceptance criteria:**
- `search()` accepts `SearchQuery` and returns correct results.
- `SearchParams` is removed.
- `SearchResult` struct unchanged.

**Estimated effort:** 0.5 hours.

---

## Phase 3: CLI Updates

### T3.1 — Update `parc-cli/src/commands/search.rs`

Simplify the search command to:
1. Join all positional args into one string.
2. Call `parse_query(&query_string)` to get `SearchQuery`.
3. Apply `--sort` and `--limit` from CLI flags (these stay as flags, not DSL).
4. Call `search(&conn, &query)`.

**New signature:**

```rust
pub fn run(vault: &Path, query: Vec<String>, json: bool, sort: Option<String>, limit: Option<usize>) -> Result<()>
```

**Acceptance criteria:**
- `parc search 'type:todo status:open'` works.
- `parc search '#backend authentication'` works.
- `--sort` and `--limit` flags still work.
- `--json` flag still works.

**Estimated effort:** 0.5 hours.

---

### T3.2 — Update `parc-cli/src/main.rs`

Remove `--type`, `--status`, `--tag` flags from the `Search` command variant. Keep `--sort`, `--limit`, `--json`.

Update the help text to mention DSL syntax.

**Acceptance criteria:**
- `parc search --help` shows DSL syntax info.
- Old flags (`--type`, `--status`, `--tag`) are gone.
- Match arm in `main()` updated to pass fewer args.

**Estimated effort:** 15 minutes.

---

### T3.3 — Update `parc-cli/src/commands/list.rs`

The `list` command also used `SearchParams`. Update it to build a `SearchQuery` from its flags instead.

```rust
let mut filters = Vec::new();
if let Some(t) = type_name {
    filters.push(Filter::Type { value: t, negated: false });
}
// ... same for status, tags
let query = SearchQuery { text_terms: vec![], filters, sort: ..., limit };
```

**Acceptance criteria:**
- `parc list` still works identically.
- `parc list todo --status open --tag backend` still works.
- No more `SearchParams` import.

**Estimated effort:** 15 minutes.

---

## Phase 4: Tests

### T4.1 — Parser unit tests

**In `parc-core/src/search.rs`:**

| Test | Input | Expected |
|------|-------|----------|
| Empty query | `""` | Empty `SearchQuery` |
| Simple word | `"hello"` | `TextTerm::Word("hello")` |
| Multiple words | `"hello world"` | Two `TextTerm::Word`s |
| Phrase | `"\"exact match\""` | `TextTerm::Phrase("exact match")` |
| Type filter | `"type:todo"` | `Filter::Type { value: "todo", negated: false }` |
| Status negation | `"status:!done"` | `Filter::Status { value: "done", negated: true }` |
| Hashtag | `"#backend"` | `Filter::Tag { value: "backend", negated: false }` |
| Tag filter | `"tag:frontend"` | `Filter::Tag { value: "frontend", negated: false }` |
| Negated tag | `"tag:!wip"` | `Filter::Tag { value: "wip", negated: true }` |
| Date shorthand today | `"due:today"` | `Filter::Due(Relative(Today))` |
| Date shorthand this-week | `"due:this-week"` | `Filter::Due(Relative(ThisWeek))` |
| Date overdue | `"due:overdue"` | `Filter::Due(Relative(Overdue))` |
| Date days-ago | `"created:30-days-ago"` | `Filter::Created(Relative(DaysAgo(30)))` |
| Date comparison | `"created:>2026-01-01"` | `Filter::Created(Absolute { op: Gt })` |
| Date absolute eq | `"due:2026-03-01"` | `Filter::Due(Absolute { op: Eq })` |
| Has links | `"has:links"` | `Filter::Has(Links)` |
| Has due | `"has:due"` | `Filter::Has(Due)` |
| Has attachments | `"has:attachments"` | `Filter::Has(Attachments)` |
| Linked | `"linked:01JQ7V3X"` | `Filter::Linked("01JQ7V3X")` |
| Priority comparison | `"priority:>=medium"` | `Filter::Priority { op: Gte, value: "medium" }` |
| By filter | `"by:alice"` | `Filter::CreatedBy { value: "alice" }` |
| Combined | `"type:todo status:open #backend API"` | 3 filters + 1 text term |

**Estimated effort:** 1–2 hours.

---

### T4.2 — Integration tests for search execution

**In `parc-core/src/search.rs`:**

| Test | Setup | Query | Expected |
|------|-------|-------|----------|
| FTS search | Index a todo | `"SQLite"` | Finds it |
| Type filter | Index a todo | `"type:todo"` / `"type:note"` | Finds / doesn't find |
| Status negation | Index open + done | `"status:!done"` | Finds only open |
| Tag AND | Index (a,b) + (a) | `"#a #b"` | Finds only (a,b) |
| Negated tag | Index (wip) + (other) | `"tag:!wip"` | Finds only (other) |
| Phrase search | Index "exact match" body | `"\"exact match\""` | Finds it |
| Priority >= | Index low, medium, high | `"priority:>=medium"` | Finds medium + high |
| Has links | Index one with links | `"has:links"` | Finds only linked one |
| Has due | Index one with due | `"has:due"` | Finds only due one |
| Linked | Index A → B | `"linked:<prefix>"` | Finds linker |
| Date absolute | Index due:2026-03-01, due:2026-06-01 | `"due:<2026-04-01"` | Finds only first |
| Combined DSL | Index todo+note, both #backend | `"type:todo #backend API"` | Finds only matching todo |

**Estimated effort:** 2–3 hours.

---

### T4.3 — Update existing integration tests

Two CLI integration tests use the removed `--type` and `--tag` flags on `search`:

- `test_search_fts` — uses `--type note` → change to `type:note` in query
- `test_hashtag_extraction` — uses `--tag inline-tag` and `--tag explicit` → change to `#inline-tag` and `tag:explicit`

**Files to change:** `parc-cli/tests/integration.rs`

**Estimated effort:** 15 minutes.

---

## Suggested Implementation Order

| Order | Task | Depends on | Est. Hours |
|-------|------|------------|------------|
| 1 | T0.2 — `ParseError` variant | M2 | 0.1 |
| 2 | T0.1 — AST types | — | 0.5 |
| 3 | T0.3 — DSL parser | T0.1, T0.2 | 2.5 |
| 4 | T1.1 — Date resolution | T0.1 | 1 |
| 5 | T2.1 — SQL compiler | T0.3, T1.1 | 3.5 |
| 6 | T2.2 — Update `search()` | T2.1 | 0.5 |
| 7 | T3.1 — Update CLI search command | T2.2 | 0.5 |
| 8 | T3.2 — Update CLI main.rs | T3.1 | 0.25 |
| 9 | T3.3 — Update list command | T2.2 | 0.25 |
| 10 | T4.1 — Parser unit tests | T0.3 | 1.5 |
| 11 | T4.2 — Integration tests | T2.2 | 2.5 |
| 12 | T4.3 — Update existing tests | T3.2 | 0.25 |
|   | **TOTAL** | | **~13 hours** |

---

## Key Design Decisions

1. **Hand-written parser**: The DSL grammar is simple enough (no nesting, no boolean operators, no precedence) that a hand-written tokenizer is cleaner and avoids adding a parser combinator dependency.

2. **Priority ordering**: Defined as `none < low < medium < high < critical`. Comparison operators on `priority:` expand to `IN (...)` lists at compile time.

3. **Tag AND semantics**: Multiple positive tag filters use separate JOINs (one per tag) with `GROUP BY`, matching the M0 behavior. Negated tags use `NOT EXISTS` subqueries.

4. **FTS5 snippet compatibility**: SQLite's `snippet()` function breaks when additional JOINs beyond the FTS virtual table are present. When both FTS text and positive tag JOINs exist, the compiler falls back to `NULL as snippet`.

5. **Date columns**: `created_at` and `updated_at` are stored as full RFC 3339 timestamps. The compiler uses `substr(col, 1, 10)` to extract the date portion for comparison with date filters. The `due` column stores just a date string.

6. **`SearchParams` removed**: Since `parc-cli` was the only consumer, the old `SearchParams` struct was removed entirely. The `list` command now builds a `SearchQuery` from its flags.

7. **Flags kept as flags**: `--sort`, `--limit`, and `--json` remain as CLI flags rather than DSL syntax. These are output-formatting concerns, not query semantics.

8. **`has:attachments` deferred**: Attachment storage doesn't exist yet (M5). The filter parses correctly but is a no-op at query time.

---

## Files Changed Summary

| File | Change |
|------|--------|
| `parc-core/src/error.rs` | Add `ParseError(String)` variant |
| `parc-core/src/search.rs` | Rewrite: add AST types, parser, date resolution, SQL compiler, update `search()`, remove `SearchParams`, add 32 tests |
| `parc-cli/src/main.rs` | Remove `--type`, `--status`, `--tag` from Search command |
| `parc-cli/src/commands/search.rs` | Simplify to use `parse_query()` + `SearchQuery` |
| `parc-cli/src/commands/list.rs` | Build `SearchQuery` from flags instead of `SearchParams` |
| `parc-cli/tests/integration.rs` | Update 2 tests to use DSL syntax |

---

## Definition of Done (M3)

- [x] `SearchQuery` AST types defined with all filter variants
- [x] `parse_query()` handles all DSL syntax: text, phrases, hashtags, filters, negation, comparison ops, date shorthands
- [x] Relative dates resolve correctly (`today`, `this-week`, `overdue`, `N-days-ago`, etc.)
- [x] SQL compiler translates all filter types to correct parameterized SQL
- [x] Priority comparison expands to `IN (...)` using defined ordering
- [x] `search()` accepts `SearchQuery` (old `SearchParams` removed)
- [x] CLI `search` command uses DSL parser — old `--type`, `--status`, `--tag` flags removed
- [x] CLI `list` command updated to build `SearchQuery` from flags
- [x] 20 parser unit tests pass
- [x] 8 search integration tests pass
- [x] All existing M0/M1/M2 tests pass (no regressions)
- [x] `cargo build` clean
