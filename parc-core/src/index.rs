//! The SQLite index: a cache of `fragments/`, never the source of truth.
//!
//! Opening it with [`open_index`] first brings it up to date with the files
//! ([`sync`]), so fragments added, edited or deleted outside parc (an editor,
//! a script, `git pull`, a sync tool) show up without a manual reindex. The
//! sync re-reads only files whose size or mtime moved since they were indexed,
//! and writes nothing when nothing changed. An index built by another version
//! of parc, or a damaged `index.db`, is rebuilt on open. [`reindex`] rebuilds
//! from scratch in one transaction.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior};

use crate::error::ParcError;
use crate::fragment::{self, Fragment};
use crate::link::{self, LinkResolver};
use crate::secure_fs;
use crate::tag;

/// Bump when what gets stored changes without `SCHEMA_SQL` changing. Changes
/// to `SCHEMA_SQL` need no bump: they change [`index_format`] by themselves.
const INDEX_VERSION: u32 = 2;

/// How long a file's mtime stays untrustworthy after it was indexed. A second
/// write in the same timestamp tick, with the same size, leaves mtime and size
/// unchanged, so a file whose mtime is this close to when it was checked is
/// read again on the next sync (git calls these entries "racily clean").
/// Generous enough for filesystems that store mtimes in 2-second steps.
const RACY_WINDOW_NS: i64 = 3_000_000_000;

/// How long to wait for another parc process that holds the write lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

const SCHEMA_SQL: &str = "
-- `seq` is a stable row number (an INTEGER PRIMARY KEY survives VACUUM) and
-- the rowid of the fragment's full-text entry, so that entry can be found
-- without scanning: FTS5 can't index its `id` column.
CREATE TABLE IF NOT EXISTS fragments (
    seq         INTEGER PRIMARY KEY,
    id          TEXT NOT NULL UNIQUE,
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
    extra_json  TEXT,
    attachment_count INTEGER NOT NULL DEFAULT 0,
    archived    INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS fragment_tags (
    fragment_id TEXT NOT NULL REFERENCES fragments(id),
    tag         TEXT NOT NULL,
    PRIMARY KEY (fragment_id, tag)
);

CREATE VIRTUAL TABLE IF NOT EXISTS fragments_fts USING fts5(
    id UNINDEXED,
    title,
    body,
    tags
);

-- Frontmatter links are stored as written (possibly an ID prefix). Body
-- wiki-links (from_body = 1) are stored resolved to a full ID. `resolved_id`
-- is the fragment a link names, NULL when it names none, several, or only its
-- source; every reader goes through it. Both are resolved again whenever a
-- fragment is added, removed or retitled.
CREATE TABLE IF NOT EXISTS fragment_links (
    source_id   TEXT NOT NULL REFERENCES fragments(id),
    target_id   TEXT NOT NULL,
    from_body   INTEGER NOT NULL DEFAULT 0,
    resolved_id TEXT,
    PRIMARY KEY (source_id, target_id)
);

-- What was last indexed from each file in fragments/ (keyed by file stem), so
-- a sync reads only files that changed. `revision` is the content hash of the
-- indexed bytes. A file that could not be indexed has `error` set and no row
-- in `fragments`.
CREATE TABLE IF NOT EXISTS fragment_files (
    id            TEXT PRIMARY KEY,
    mtime_ns      INTEGER NOT NULL,
    size          INTEGER NOT NULL,
    revision      TEXT NOT NULL,
    checked_at_ns INTEGER NOT NULL,
    error         TEXT
);

CREATE TABLE IF NOT EXISTS attachment_files (
    fragment_id TEXT NOT NULL,
    filename    TEXT NOT NULL,
    sha256      TEXT NOT NULL DEFAULT '',
    size        INTEGER NOT NULL,
    mtime_ns    INTEGER NOT NULL,
    unit        TEXT,
    format      TEXT,
    title       TEXT,
    parts       INTEGER NOT NULL DEFAULT 0,
    text_format INTEGER NOT NULL DEFAULT 0,
    status      TEXT NOT NULL DEFAULT 'pending',
    error       TEXT,
    PRIMARY KEY (fragment_id, filename)
);

CREATE VIRTUAL TABLE IF NOT EXISTS attachment_fts USING fts5(
    fragment_id UNINDEXED,
    filename UNINDEXED,
    part UNINDEXED,
    text
);

CREATE INDEX IF NOT EXISTS idx_fragments_type ON fragments(type);
CREATE INDEX IF NOT EXISTS idx_fragments_status ON fragments(status);
CREATE INDEX IF NOT EXISTS idx_fragments_due ON fragments(due);
CREATE INDEX IF NOT EXISTS idx_fragment_tags_tag ON fragment_tags(tag);
CREATE INDEX IF NOT EXISTS idx_fragment_links_resolved ON fragment_links(resolved_id);
CREATE INDEX IF NOT EXISTS idx_attachment_files_status ON attachment_files(status);
";

const DROP_SQL: &str = "
DROP TABLE IF EXISTS attachment_fts;
DROP TABLE IF EXISTS attachment_files;
DROP TABLE IF EXISTS fragments_fts;
DROP TABLE IF EXISTS fragment_tags;
DROP TABLE IF EXISTS fragment_links;
DROP TABLE IF EXISTS fragment_files;
DROP TABLE IF EXISTS fragments;
";

const CLEAR_SQL: &str = "
DELETE FROM attachment_fts;
DELETE FROM attachment_files;
DELETE FROM fragments_fts;
DELETE FROM fragment_tags;
DELETE FROM fragment_links;
DELETE FROM fragment_files;
DELETE FROM fragments;
";

/// What a [`sync`] or [`reindex`] changed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Fragments indexed for the first time (all of them, after a rebuild).
    pub added: usize,
    /// Fragments indexed again because their file changed.
    pub updated: usize,
    /// Fragments dropped because their file is gone or can no longer be indexed.
    pub removed: usize,
    /// Files this run tried and failed to index. A file that stays broken is
    /// reported by the run that first sees it, not by every run after.
    pub problems: Vec<IndexProblem>,
}

impl SyncReport {
    /// Whether the index changed or a problem was found.
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.updated == 0 && self.removed == 0 && self.problems.is_empty()
    }
}

/// A file in `fragments/` that is not in the index, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexProblem {
    /// File name within `fragments/`, e.g. `01JQ7V3XKP5GQZ2N8R6T1WBMVH.md`.
    pub file_name: String,
    pub reason: String,
}

/// Open the index, creating it or rebuilding it for this version of parc if
/// needed, without syncing it. Use [`open_index`] to read fragments.
pub fn init_index(vault: &Path) -> Result<Connection, ParcError> {
    let db_path = vault.join("index.db");
    match open_schema(&db_path) {
        // The index is derived data: replace a damaged file instead of failing.
        Err(ParcError::Sqlite(e)) if is_damaged(&e) => {
            discard_index(vault)?;
            open_schema(&db_path)
        }
        other => other,
    }
}

fn discard_index(vault: &Path) -> Result<(), ParcError> {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let path = vault.join(format!("index.db{}", suffix));
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(())
}

/// Open the index and bring it up to date with the files in `fragments/`.
pub fn open_index(vault: &Path) -> Result<Connection, ParcError> {
    Ok(open_index_with_report(vault)?.0)
}

/// [`open_index`], also returning what the sync changed and which files it
/// could not index, for frontends that want to tell the user.
pub fn open_index_with_report(vault: &Path) -> Result<(Connection, SyncReport), ParcError> {
    let open = || -> Result<(Connection, SyncReport), ParcError> {
        let conn = init_index(vault)?;
        let report = sync(&conn, vault)?;
        Ok((conn, report))
    };
    match open() {
        // Damage past the header only shows once the sync reads the tables.
        Err(ParcError::Sqlite(e)) if is_damaged(&e) => {
            discard_index(vault)?;
            open()
        }
        other => other,
    }
}

fn open_schema(db_path: &Path) -> Result<Connection, ParcError> {
    secure_fs::prepare_private_file(db_path)?;
    let conn = Connection::open(db_path)?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    let format = index_format();
    if user_version(&conn)? != format {
        let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate)?;
        // Another process may have rebuilt it while we waited for the lock.
        if user_version(&tx)? != format {
            tx.execute_batch(DROP_SQL)?;
            tx.execute_batch(SCHEMA_SQL)?;
            tx.pragma_update(None, "user_version", format)?;
        }
        tx.commit()?;
    }
    secure_fs::set_private_file_permissions(db_path)?;
    Ok(conn)
}

/// The format stamp kept in `PRAGMA user_version`: a hash of the schema text
/// and [`INDEX_VERSION`]. An index stamped with anything else (built by
/// another version of parc, or by a build from before a schema edit) is
/// dropped and rebuilt when it is opened. Always at least 2^30, so it never
/// equals 0 (an unstamped index) or a hand-numbered stamp from older builds.
fn index_format() -> i64 {
    let hash = SCHEMA_SQL
        .bytes()
        .chain(INDEX_VERSION.to_le_bytes())
        .fold(0x811c_9dc5u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
    i64::from(0x4000_0000 | (hash & 0x3fff_ffff))
}

fn user_version(conn: &Connection) -> Result<i64, ParcError> {
    Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
}

fn is_damaged(e: &rusqlite::Error) -> bool {
    matches!(
        e.sqlite_error_code(),
        Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt)
    )
}

/// Bring the index up to date with `fragments/`: index new and changed files
/// and drop fragments whose file is gone, in one transaction. Only files whose
/// size or mtime moved since they were indexed (or that were indexed too soon
/// after a write to trust those) are read. Nothing is written when nothing
/// changed, so the common case takes no write lock.
pub fn sync(conn: &Connection, vault: &Path) -> Result<SyncReport, ParcError> {
    if Plan::read(conn, vault)?.is_empty() && !crate::extract::attachments_need_sync(conn, vault)? {
        return Ok(SyncReport::default());
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    // Plan again under the lock: another process may have synced meanwhile.
    let plan = Plan::read(&tx, vault)?;
    let report = apply(&tx, vault, plan)?;
    crate::extract::sync_attachments(&tx, vault)?;
    tx.commit()?;
    Ok(report)
}

/// Rebuild the index from the fragment files in one transaction, so an
/// interrupted rebuild leaves the previous index in place. Reports every file
/// that could not be indexed.
pub fn reindex(vault: &Path) -> Result<SyncReport, ParcError> {
    let conn = init_index(vault)?;
    let tx = Transaction::new_unchecked(&conn, TransactionBehavior::Immediate)?;
    tx.execute_batch(CLEAR_SQL)?;
    let plan = Plan::read(&tx, vault)?;
    let report = apply(&tx, vault, plan)?;
    crate::extract::sync_attachments(&tx, vault)?;
    tx.commit()?;
    Ok(report)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStat {
    mtime_ns: i64,
    size: i64,
}

impl FileStat {
    fn of(metadata: &std::fs::Metadata) -> Self {
        Self {
            mtime_ns: metadata.modified().map(system_time_ns).unwrap_or(0),
            size: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
        }
    }
}

struct FileState {
    stat: FileStat,
    revision: String,
    checked_at_ns: i64,
    error: Option<String>,
}

impl FileState {
    /// Whether the recorded mtime was old enough to trust when it was checked,
    /// so an unchanged size and mtime mean unchanged content.
    fn is_trusted(&self) -> bool {
        self.stat.mtime_ns <= self.checked_at_ns - RACY_WINDOW_NS
    }
}

/// What a sync has to do, worked out from a directory scan and the
/// `fragment_files` table. Only files written moments ago are read.
struct Plan {
    scan_started_ns: i64,
    /// New, changed, or not yet trusted files, by ID.
    to_read: Vec<(String, FileStat)>,
    /// Indexed or tracked IDs whose file is gone.
    gone: Vec<String>,
    known: HashMap<String, FileState>,
}

impl Plan {
    fn read(conn: &Connection, vault: &Path) -> Result<Self, ParcError> {
        // Taken before the scan, so a file written during the scan looks racy.
        let scan_started_ns = system_time_ns(SystemTime::now());
        let on_disk = scan_fragment_files(vault)?;
        let known = load_file_states(conn)?;

        let mut to_read = Vec::new();
        for (id, stat) in &on_disk {
            if let Some(state) = known.get(id).filter(|state| state.stat == *stat) {
                if state.is_trusted() {
                    continue;
                }
                // Written moments ago: recording it as checked now would not
                // make it trusted yet, so just make sure it hasn't changed.
                // Saves a write (and the write lock) on every sync right
                // after a save.
                if stat.mtime_ns > scan_started_ns - RACY_WINDOW_NS
                    && file_revision(vault, id).as_deref() == Some(state.revision.as_str())
                {
                    continue;
                }
            }
            to_read.push((id.clone(), *stat));
        }
        to_read.sort_by(|a, b| a.0.cmp(&b.0));

        let mut indexed = conn.prepare("SELECT id FROM fragments")?;
        let indexed = indexed
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut gone: Vec<String> = known
            .keys()
            .cloned()
            .chain(indexed)
            .filter(|id| !on_disk.contains_key(id))
            .collect();
        gone.sort();
        gone.dedup();

        Ok(Self {
            scan_started_ns,
            to_read,
            gone,
            known,
        })
    }

    fn is_empty(&self) -> bool {
        self.to_read.is_empty() && self.gone.is_empty()
    }
}

fn file_revision(vault: &Path, id: &str) -> Option<String> {
    let bytes = std::fs::read(vault.join("fragments").join(format!("{}.md", id))).ok()?;
    Some(fragment::content_revision(&bytes))
}

/// Every `*.md` file in `fragments/` by stem, with its size and mtime.
fn scan_fragment_files(vault: &Path) -> Result<HashMap<String, FileStat>, ParcError> {
    let dir = vault.join("fragments");
    let mut files = HashMap::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(stem) = name
            .to_str()
            .and_then(|name| name.strip_suffix(".md"))
            .filter(|stem| !stem.is_empty())
        else {
            continue;
        };
        // Follows symlinks, like reading the file does. A file deleted since
        // the directory was listed is simply not there.
        let metadata = match entry.file_type() {
            Ok(file_type) if file_type.is_symlink() => std::fs::metadata(entry.path()),
            Ok(_) => entry.metadata(),
            Err(e) => Err(e),
        };
        match metadata {
            Ok(metadata) if metadata.is_file() => {
                files.insert(stem.to_string(), FileStat::of(&metadata));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(files)
}

fn load_file_states(conn: &Connection) -> Result<HashMap<String, FileState>, ParcError> {
    let mut stmt = conn
        .prepare("SELECT id, mtime_ns, size, revision, checked_at_ns, error FROM fragment_files")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            FileState {
                stat: FileStat {
                    mtime_ns: row.get(1)?,
                    size: row.get(2)?,
                },
                revision: row.get(3)?,
                checked_at_ns: row.get(4)?,
                error: row.get(5)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

fn apply(conn: &Connection, vault: &Path, plan: Plan) -> Result<SyncReport, ParcError> {
    let mut report = SyncReport::default();
    // Set when a fragment appears, disappears or changes title: other
    // fragments' `[[title]]` and `[[prefix]]` links may then resolve differently.
    let mut names_changed = false;
    let mut relink_ids = Vec::new();

    for (id, stat) in plan.to_read {
        let file_name = format!("{}.md", id);
        let bytes = match std::fs::read(vault.join("fragments").join(&file_name)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Deleted since the scan.
                if remove_rows(conn, &id)? {
                    report.removed += 1;
                    names_changed = true;
                }
                conn.execute("DELETE FROM fragment_files WHERE id = ?1", [&id])?;
                continue;
            }
            Err(e) => {
                // Unreadable (permissions, I/O): leave the index as it was and
                // record nothing, so the next sync tries again.
                report.problems.push(IndexProblem {
                    file_name,
                    reason: format!("could not read the file: {}", e),
                });
                continue;
            }
        };
        let revision = fragment::content_revision(&bytes);

        if let Some(known) = plan.known.get(&id).filter(|k| k.revision == revision) {
            // Same content as indexed (touched, checked out again, or a racy
            // entry that turned out unchanged): only the stat is new.
            record_file(
                conn,
                &id,
                stat,
                &revision,
                plan.scan_started_ns,
                known.error.as_deref(),
            )?;
            continue;
        }

        match fragment::parse_fragment_file(&id, &bytes) {
            Ok(frag) => {
                let old_title = indexed_title(conn, &id)?;
                write_rows(conn, &frag, &merged_tags(&frag), &frag.links)?;
                if old_title.is_some() {
                    report.updated += 1;
                } else {
                    report.added += 1;
                }
                names_changed |= old_title.as_deref() != Some(frag.title.as_str());
                relink_ids.push(id.clone());
                record_file(conn, &id, stat, &revision, plan.scan_started_ns, None)?;
            }
            Err(e) => {
                let reason = e.to_string();
                if remove_rows(conn, &id)? {
                    report.removed += 1;
                    names_changed = true;
                }
                record_file(
                    conn,
                    &id,
                    stat,
                    &revision,
                    plan.scan_started_ns,
                    Some(&reason),
                )?;
                report.problems.push(IndexProblem { file_name, reason });
            }
        }
    }

    for id in &plan.gone {
        if remove_rows(conn, id)? {
            report.removed += 1;
            names_changed = true;
        }
        conn.execute("DELETE FROM fragment_files WHERE id = ?1", [id])?;
    }

    if names_changed {
        relink(conn, None)?;
    } else if !relink_ids.is_empty() {
        relink(conn, Some(&relink_ids))?;
    }
    Ok(report)
}

fn merged_tags(fragment: &Fragment) -> Vec<String> {
    let inline_tags = tag::extract_inline_tags(&fragment.body);
    tag::merge_tags(&fragment.tags, &inline_tags)
}

fn indexed_title(conn: &Connection, id: &str) -> Result<Option<String>, ParcError> {
    Ok(conn
        .prepare_cached("SELECT title FROM fragments WHERE id = ?1")?
        .query_row([id], |row| row.get(0))
        .optional()?)
}

fn record_file(
    conn: &Connection,
    id: &str,
    stat: FileStat,
    revision: &str,
    checked_at_ns: i64,
    error: Option<&str>,
) -> Result<(), ParcError> {
    conn.prepare_cached(
        "INSERT OR REPLACE INTO fragment_files (id, mtime_ns, size, revision, checked_at_ns, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(rusqlite::params![
        id,
        stat.mtime_ns,
        stat.size,
        revision,
        checked_at_ns,
        error
    ])?;
    Ok(())
}

fn system_time_ns(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}

/// Index a single fragment as given, with these tags and links stored as
/// frontmatter links. No file bookkeeping: a later [`sync`] drops it unless
/// `fragments/` has its file. The write path uses [`index_fragment_auto`].
pub fn index_fragment(
    conn: &Connection,
    fragment: &Fragment,
    merged_tags: &[String],
    merged_links: &[String],
) -> Result<(), ParcError> {
    write_rows(conn, fragment, merged_tags, merged_links)
}

/// Upsert a fragment's row, tags and full-text entry, and replace its links
/// with `links` (stored as frontmatter links). Body links are added by
/// [`relink`].
fn write_rows(
    conn: &Connection,
    fragment: &Fragment,
    tags: &[String],
    links: &[String],
) -> Result<(), ParcError> {
    let extra_json = serde_json::to_string(&fragment.extra_fields)?;

    let archived = fragment
        .extra_fields
        .get("archived")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // An upsert, not INSERT OR REPLACE: the row keeps its `seq`.
    let seq: i64 = conn
        .prepare_cached(
            "INSERT INTO fragments (id, type, title, status, priority, due, assignee, created_by, created_at, updated_at, body, extra_json, attachment_count, archived)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET
                type = excluded.type, title = excluded.title, status = excluded.status,
                priority = excluded.priority, due = excluded.due, assignee = excluded.assignee,
                created_by = excluded.created_by, created_at = excluded.created_at,
                updated_at = excluded.updated_at, body = excluded.body,
                extra_json = excluded.extra_json, attachment_count = excluded.attachment_count,
                archived = excluded.archived
             RETURNING seq",
        )?
        .query_row(rusqlite::params![
        fragment.id,
        fragment.fragment_type,
        fragment.title,
        fragment.extra_fields.get("status").and_then(|v| v.as_str()),
        fragment.extra_fields.get("priority").and_then(|v| v.as_str()),
        fragment.extra_fields.get("due").and_then(|v| v.as_str()),
        fragment.extra_fields.get("assignee").and_then(|v| v.as_str()),
        fragment.created_by.as_deref(),
        fragment.created_at.to_rfc3339(),
        fragment.updated_at.to_rfc3339(),
        fragment.body,
        extra_json,
            fragment.attachments.len() as i64,
            archived as i32,
        ],
        |row| row.get(0),
    )?;

    conn.prepare_cached("DELETE FROM fragment_tags WHERE fragment_id = ?1")?
        .execute([&fragment.id])?;
    let mut insert_tag = conn
        .prepare_cached("INSERT OR IGNORE INTO fragment_tags (fragment_id, tag) VALUES (?1, ?2)")?;
    for t in tags {
        insert_tag.execute(rusqlite::params![fragment.id, t])?;
    }

    conn.prepare_cached("DELETE FROM fragments_fts WHERE rowid = ?1")?
        .execute([seq])?;
    conn.prepare_cached(
        "INSERT INTO fragments_fts (rowid, id, title, body, tags) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(rusqlite::params![
        seq,
        fragment.id,
        fragment.title,
        fragment.body,
        tags.join(" ")
    ])?;

    conn.prepare_cached("DELETE FROM fragment_links WHERE source_id = ?1")?
        .execute([&fragment.id])?;
    let mut insert_link = conn.prepare_cached(&format!(
        "INSERT OR IGNORE INTO fragment_links (source_id, target_id, from_body, resolved_id)
         SELECT source_id, target_id, 0, {RESOLVE_FRONTMATTER}
         FROM (SELECT ?1 AS source_id, ?2 AS target_id)"
    ))?;
    let mut seen = HashSet::new();
    for target in links {
        if seen.insert(target.to_uppercase()) {
            insert_link.execute(rusqlite::params![fragment.id, target])?;
        }
    }

    Ok(())
}

/// The fragment a frontmatter link row names, as SQL over that row's
/// `source_id` and `target_id`: the one indexed fragment whose ID starts with
/// the target (case-insensitively, four characters at least), unless that is
/// the source itself. NULL when it names none or several. A range over the
/// primary key, so resolving every link in a vault stays cheap.
const RESOLVE_FRONTMATTER: &str =
    "(SELECT CASE WHEN COUNT(*) = 1 AND MIN(f.id) <> source_id THEN MIN(f.id) END
     FROM fragments f
     WHERE length(target_id) >= 4
       AND f.id >= upper(target_id) AND f.id < upper(target_id) || '~')";

/// Resolve links again: with `None`, every frontmatter link and the body
/// wiki-links of every fragment whose body has one (the set of fragments or
/// their titles changed); with ids, those fragments' body wiki-links (their
/// frontmatter links were resolved as they were written). Targets resolve
/// against every indexed fragment; a body link that names no fragment, or
/// several, is left out.
fn relink(conn: &Connection, ids: Option<&[String]>) -> Result<(), ParcError> {
    if ids.is_none() {
        conn.execute(
            &format!(
                "UPDATE fragment_links SET resolved_id = {RESOLVE_FRONTMATTER} WHERE from_body = 0"
            ),
            [],
        )?;
    }
    let resolver = {
        let mut stmt = conn.prepare("SELECT id, title FROM fragments")?;
        let refs = stmt
            .query_map([], |row| {
                Ok(link::FragmentRef {
                    id: row.get(0)?,
                    title: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        LinkResolver::new(refs)
    };

    let sources: Vec<(String, String)> = match ids {
        None => conn
            .prepare("SELECT id, body FROM fragments WHERE body LIKE '%[[%'")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?,
        Some(ids) => {
            let mut stmt = conn.prepare_cached("SELECT body FROM fragments WHERE id = ?1")?;
            let mut sources = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(body) = stmt.query_row([id], |row| row.get(0)).optional()? {
                    sources.push((id.clone(), body));
                }
            }
            sources
        }
    };

    let mut clear =
        conn.prepare_cached("DELETE FROM fragment_links WHERE source_id = ?1 AND from_body = 1")?;
    let mut frontmatter = conn.prepare_cached(
        "SELECT COALESCE(resolved_id, target_id) FROM fragment_links WHERE source_id = ?1",
    )?;
    let mut insert = conn.prepare_cached(
        "INSERT OR IGNORE INTO fragment_links (source_id, target_id, from_body, resolved_id)
         VALUES (?1, ?2, 1, CASE WHEN ?2 <> ?1 THEN ?2 END)",
    )?;
    for (id, body) in sources {
        clear.execute([&id])?;
        let wiki_links = link::parse_wiki_links(&body);
        if wiki_links.is_empty() {
            continue;
        }
        // Frontmatter links come first; a body link to the same target adds nothing.
        let mut seen: HashSet<String> = frontmatter
            .query_map([&id], |row| row.get::<_, String>(0))?
            .map(|target| target.map(|t| t.to_uppercase()))
            .collect::<Result<_, _>>()?;
        for wiki_link in wiki_links {
            if let Some(target) = resolver.unique(&wiki_link.target) {
                if seen.insert(target.to_uppercase()) {
                    insert.execute(rusqlite::params![id, target])?;
                }
            }
        }
    }
    Ok(())
}

/// Index a fragment the write path just saved to `fragments/`: merge inline
/// tags, resolve body wiki-links, and record the file so the next sync need
/// not read it again. A new or retitled fragment also re-resolves other
/// fragments' wiki-links, since `[[its title]]` may now name it.
pub fn index_fragment_auto(
    conn: &Connection,
    fragment: &Fragment,
    vault: &Path,
) -> Result<(), ParcError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let old_title = indexed_title(&tx, &fragment.id)?;
    write_rows(&tx, fragment, &merged_tags(fragment), &fragment.links)?;

    let path = vault.join("fragments").join(format!("{}.md", fragment.id));
    if let Ok(metadata) = std::fs::metadata(&path) {
        // The revision of what was indexed, not a re-read of the file: if
        // another process wrote it in between, the next sync sees the
        // mismatch (the entry is racy) and indexes the file as it is.
        let revision =
            fragment::content_revision(fragment::serialize_fragment(fragment).as_bytes());
        let checked_at_ns = system_time_ns(SystemTime::now());
        record_file(
            &tx,
            &fragment.id,
            FileStat::of(&metadata),
            &revision,
            checked_at_ns,
            None,
        )?;
    }

    if old_title.as_deref() == Some(fragment.title.as_str()) {
        relink(&tx, Some(std::slice::from_ref(&fragment.id)))?;
    } else {
        relink(&tx, None)?;
    }
    tx.commit()?;
    Ok(())
}

/// Remove a fragment from the index (its file has been moved or deleted).
pub fn remove_from_index(conn: &Connection, id: &str) -> Result<(), ParcError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let removed = remove_rows(&tx, id)?;
    tx.execute("DELETE FROM fragment_files WHERE id = ?1", [id])?;
    if removed {
        relink(&tx, None)?;
    }
    tx.commit()?;
    Ok(())
}

/// Delete a fragment's rows. Returns whether it was indexed.
fn remove_rows(conn: &Connection, id: &str) -> Result<bool, ParcError> {
    crate::extract::store::remove_fragment_attachments(conn, id)?;
    conn.prepare_cached("DELETE FROM fragment_tags WHERE fragment_id = ?1")?
        .execute([id])?;
    conn.prepare_cached("DELETE FROM fragment_links WHERE source_id = ?1")?
        .execute([id])?;
    let seq: Option<i64> = conn
        .prepare_cached("DELETE FROM fragments WHERE id = ?1 RETURNING seq")?
        .query_row([id], |row| row.get(0))
        .optional()?;
    if let Some(seq) = seq {
        conn.prepare_cached("DELETE FROM fragments_fts WHERE rowid = ?1")?
            .execute([seq])?;
    }
    Ok(seq.is_some())
}

/// Resolve a prefix against a list of known IDs.
/// Returns Some(full_id) if exactly one match, None otherwise.
#[cfg(test)]
fn resolve_prefix(prefix: &str, all_ids: &[String]) -> Option<String> {
    let upper = prefix.to_uppercase();
    let matches: Vec<&String> = all_ids.iter().filter(|id| id.starts_with(&upper)).collect();
    if matches.len() == 1 {
        Some(matches[0].clone())
    } else {
        None
    }
}

// --- Backlink queries ---

#[derive(Debug, Clone)]
pub struct BacklinkInfo {
    pub source_id: String,
    pub source_type: String,
    pub source_title: String,
}

/// Find all fragments that link to the given target ID.
pub fn get_backlinks(conn: &Connection, target_id: &str) -> Result<Vec<BacklinkInfo>, ParcError> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.type, f.title
         FROM fragments f
         WHERE f.id IN (SELECT source_id FROM fragment_links WHERE resolved_id = ?1)
         ORDER BY f.updated_at DESC",
    )?;

    let results = stmt
        .query_map([target_id], |row| {
            Ok(BacklinkInfo {
                source_id: row.get(0)?,
                source_type: row.get(1)?,
                source_title: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

/// Every indexed link as `(source, target)` full IDs, once per pair, for
/// drawing a whole-vault graph. Links to nothing (a deleted or ambiguous
/// target) and links to self are left out.
pub fn all_links(conn: &Connection) -> Result<Vec<(String, String)>, ParcError> {
    let edges = conn
        .prepare(
            "SELECT DISTINCT source_id, resolved_id FROM fragment_links
             WHERE resolved_id IS NOT NULL ORDER BY source_id, resolved_id",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::new_id;
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn make_fragment(title: &str, body: &str) -> Fragment {
        Fragment {
            id: new_id(),
            fragment_type: "note".to_string(),
            title: title.to_string(),
            tags: vec!["test".to_string()],
            links: Vec::new(),
            attachments: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            extra_fields: BTreeMap::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn test_init_index_idempotent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        init_index(&vault).unwrap();
        init_index(&vault).unwrap(); // should not error
    }

    #[test]
    fn test_index_and_search() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = init_index(&vault).unwrap();

        let frag = make_fragment("SQLite indexing", "Using FTS5 for search");
        index_fragment(&conn, &frag, &["test".to_string()], &[]).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM fragments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let fts_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fragments_fts WHERE fragments_fts MATCH 'SQLite'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);
    }

    #[test]
    fn test_remove_from_index() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = init_index(&vault).unwrap();

        let frag = make_fragment("To remove", "Content here");
        index_fragment(&conn, &frag, &["tag1".to_string()], &[]).unwrap();
        remove_from_index(&conn, &frag.id).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM fragments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_reindex() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag = make_fragment("Reindex test", "Body with #hashtag");
        fragment::create_fragment(&vault, &frag).unwrap();

        let count = reindex(&vault).unwrap().added;
        assert_eq!(count, 1);
    }

    #[test]
    fn test_backlinks() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = init_index(&vault).unwrap();

        let frag_a = make_fragment("Fragment A", "Body A");
        let mut frag_b = make_fragment("Fragment B", "Body B");
        frag_b.links = vec![frag_a.id.clone()];

        index_fragment(&conn, &frag_a, &[], &[]).unwrap();
        index_fragment(&conn, &frag_b, &[], &[frag_a.id.clone()]).unwrap();

        let backlinks = get_backlinks(&conn, &frag_a.id).unwrap();
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].source_id, frag_b.id);
        assert_eq!(backlinks[0].source_title, "Fragment B");
    }

    #[test]
    fn test_all_links_resolves_prefixes_and_drops_dangling() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = init_index(&vault).unwrap();

        let a = make_fragment("A", "");
        let b = make_fragment("B", "");
        let c = make_fragment("C", "");
        index_fragment(&conn, &a, &[], &[]).unwrap();
        // b links to a by full ID; c by a lowercase frontmatter prefix, to a
        // fragment that no longer exists, and to itself.
        index_fragment(&conn, &b, &[], &[a.id.clone()]).unwrap();
        let prefix = a.id[..16].to_lowercase();
        index_fragment(
            &conn,
            &c,
            &[],
            &[prefix, "01ZZZZZZZZZZZZZZZZZZZZZZZZ".into(), c.id.clone()],
        )
        .unwrap();

        let mut edges = all_links(&conn).unwrap();
        edges.sort();
        let mut expected = vec![(b.id.clone(), a.id.clone()), (c.id.clone(), a.id.clone())];
        expected.sort();
        assert_eq!(edges, expected);
    }

    #[test]
    fn test_backlinks_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = init_index(&vault).unwrap();

        let frag = make_fragment("Lonely", "No links here");
        index_fragment(&conn, &frag, &[], &[]).unwrap();

        let backlinks = get_backlinks(&conn, &frag.id).unwrap();
        assert!(backlinks.is_empty());
    }

    #[test]
    fn test_resolve_prefix() {
        let ids = vec![
            "01JQ7V3XKP5GQZ2N8R6T1WBMVH".to_string(),
            "01JQ7V4YAB1234567890ABCDEF".to_string(),
        ];
        assert_eq!(
            resolve_prefix("01JQ7V3X", &ids),
            Some("01JQ7V3XKP5GQZ2N8R6T1WBMVH".to_string())
        );
        assert_eq!(resolve_prefix("01JQ7V", &ids), None); // ambiguous
        assert_eq!(resolve_prefix("ZZZZZ", &ids), None); // not found
    }

    #[test]
    fn test_reindex_with_wiki_links() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag_a = make_fragment("Target", "I am the target.");
        fragment::create_fragment(&vault, &frag_a).unwrap();

        // Use full ID to avoid ambiguity (ULIDs generated close together share prefixes)
        let mut frag_b = make_fragment("Linker", &format!("Links to [[{}]].", frag_a.id));
        frag_b.links = Vec::new();
        fragment::create_fragment(&vault, &frag_b).unwrap();

        let count = reindex(&vault).unwrap().added;
        assert_eq!(count, 2);

        // Verify backlink was created from body wiki-link
        let conn = open_index(&vault).unwrap();
        let backlinks = get_backlinks(&conn, &frag_a.id).unwrap();
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].source_id, frag_b.id);
    }

    #[test]
    fn test_reindex_with_title_wiki_links() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag_a = make_fragment("Auth refactor", "I am the target.");
        fragment::create_fragment(&vault, &frag_a).unwrap();

        let frag_b = make_fragment("Linker", "Links to [[Auth refactor]].");
        fragment::create_fragment(&vault, &frag_b).unwrap();

        let count = reindex(&vault).unwrap().added;
        assert_eq!(count, 2);

        let conn = open_index(&vault).unwrap();
        let backlinks = get_backlinks(&conn, &frag_a.id).unwrap();
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].source_id, frag_b.id);
    }

    // --- Keeping the index in sync with the files ---

    fn test_vault() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        (tmp, vault)
    }

    fn fragment_path(vault: &Path, id: &str) -> std::path::PathBuf {
        vault.join("fragments").join(format!("{}.md", id))
    }

    fn set_mtime(path: &Path, time: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    fn indexed_titles(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT title FROM fragments ORDER BY title")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn backlink_ids(conn: &Connection, id: &str) -> Vec<String> {
        get_backlinks(conn, id)
            .unwrap()
            .into_iter()
            .map(|b| b.source_id)
            .collect()
    }

    /// Two IDs sharing no prefix with each other, so `[[prefix]]` tests are
    /// not thrown off by ULIDs minted in the same millisecond.
    fn with_id(mut frag: Fragment, id: &str) -> Fragment {
        frag.id = id.to_string();
        frag
    }

    #[test]
    fn sync_indexes_fragments_written_outside_parc() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Written by hand", "Body about ferries");
        fragment::create_fragment(&vault, &frag).unwrap();

        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(indexed_titles(&conn), vec!["Written by hand"]);

        // Nothing changed since: nothing to do.
        let (_, report) = open_index_with_report(&vault).unwrap();
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn sync_picks_up_an_edit_in_the_same_timestamp_tick() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Title AAAA", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();
        let path = fragment_path(&vault, &frag.id);
        let first_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        open_index(&vault).unwrap();

        // Same size, and the mtime put back: only the content differs. The
        // file was indexed moments after it was written, so the sync must not
        // trust size and mtime alone.
        let edited = Fragment {
            title: "Title BBBB".into(),
            ..frag.clone()
        };
        std::fs::write(&path, fragment::serialize_fragment(&edited)).unwrap();
        set_mtime(&path, first_mtime);

        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert_eq!(report.updated, 1);
        assert_eq!(indexed_titles(&conn), vec!["Title BBBB"]);
    }

    #[test]
    fn sync_does_not_read_files_whose_size_and_mtime_are_settled() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Original", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();
        let path = fragment_path(&vault, &frag.id);
        let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        set_mtime(&path, an_hour_ago);
        open_index(&vault).unwrap();

        // Swap the content behind sync's back, keeping size and mtime. The
        // mtime was long settled when indexed, so the file isn't read again.
        let swapped = Fragment {
            title: "Swapped!".into(),
            ..frag.clone()
        };
        std::fs::write(&path, fragment::serialize_fragment(&swapped)).unwrap();
        set_mtime(&path, an_hour_ago);
        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert!(report.is_empty());
        assert_eq!(indexed_titles(&conn), vec!["Original"]);

        // Any real edit moves the mtime, and is picked up.
        set_mtime(&path, SystemTime::now());
        let (conn, _) = open_index_with_report(&vault).unwrap();
        assert_eq!(indexed_titles(&conn), vec!["Swapped!"]);
    }

    #[test]
    fn sync_without_changes_does_not_write_the_index() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Settled", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();
        set_mtime(
            &fragment_path(&vault, &frag.id),
            SystemTime::now() - Duration::from_secs(3600),
        );
        open_index(&vault).unwrap();

        let db = vault.join("index.db");
        let long_ago = SystemTime::now() - Duration::from_secs(7200);
        set_mtime(&db, long_ago);
        let (_, report) = open_index_with_report(&vault).unwrap();
        assert!(report.is_empty());
        assert_eq!(
            std::fs::metadata(&db).unwrap().modified().unwrap(),
            long_ago
        );
    }

    #[test]
    fn a_write_through_the_index_is_not_indexed_again() {
        let (_tmp, vault) = test_vault();
        let conn = open_index(&vault).unwrap();
        let frag = make_fragment("Saved by parc", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();
        index_fragment_auto(&conn, &frag, &vault).unwrap();

        let (_, report) = open_index_with_report(&vault).unwrap();
        assert!(report.is_empty(), "{report:?}");
    }

    #[test]
    fn sync_drops_fragments_whose_file_is_gone() {
        let (_tmp, vault) = test_vault();
        let keep = make_fragment("Keep", "");
        let gone = make_fragment("Gone", "");
        fragment::create_fragment(&vault, &keep).unwrap();
        fragment::create_fragment(&vault, &gone).unwrap();
        open_index(&vault).unwrap();

        std::fs::remove_file(fragment_path(&vault, &gone.id)).unwrap();
        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert_eq!(report.removed, 1);
        assert_eq!(indexed_titles(&conn), vec!["Keep"]);
        let tracked: i64 = conn
            .query_row("SELECT COUNT(*) FROM fragment_files", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tracked, 1);
    }

    #[test]
    fn a_broken_file_is_reported_once_and_indexed_once_fixed() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Fixable", "Body");
        let path = fragment_path(&vault, &frag.id);
        std::fs::write(&path, "---\nid: [unclosed\n---\nbody\n").unwrap();

        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert_eq!(report.problems.len(), 1);
        assert_eq!(report.problems[0].file_name, format!("{}.md", frag.id));
        assert!(indexed_titles(&conn).is_empty());

        let (_, report) = open_index_with_report(&vault).unwrap();
        assert!(report.problems.is_empty(), "reported once, not every sync");

        std::fs::write(&path, fragment::serialize_fragment(&frag)).unwrap();
        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert!(report.problems.is_empty());
        assert_eq!(report.added, 1);
        assert_eq!(indexed_titles(&conn), vec!["Fixable"]);
    }

    #[test]
    fn breaking_an_indexed_file_drops_it_and_reports_it() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Soon broken", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();
        open_index(&vault).unwrap();

        std::fs::write(fragment_path(&vault, &frag.id), "no frontmatter at all").unwrap();
        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert_eq!(report.removed, 1);
        assert_eq!(report.problems.len(), 1);
        assert!(indexed_titles(&conn).is_empty());
    }

    #[test]
    fn a_conflict_copy_does_not_shadow_the_original() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Original", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();
        let copy = Fragment {
            title: "Their version".into(),
            ..frag.clone()
        };
        let copy_name = format!("{}.sync-conflict-20261122-101010-ABCDEFG.md", frag.id);
        std::fs::write(
            vault.join("fragments").join(&copy_name),
            fragment::serialize_fragment(&copy),
        )
        .unwrap();

        let (conn, report) = open_index_with_report(&vault).unwrap();
        assert_eq!(indexed_titles(&conn), vec!["Original"]);
        assert_eq!(report.problems.len(), 1);
        assert_eq!(report.problems[0].file_name, copy_name);
        assert!(report.problems[0]
            .reason
            .contains("does not match the file name"));
    }

    #[test]
    fn a_new_fragment_resolves_links_written_before_it_existed() {
        let (_tmp, vault) = test_vault();
        let linker = with_id(
            make_fragment("Linker", "See [[Later note]] and [[01KAAA]]."),
            "01JZZZZZZZZZZZZZZZZZZZZZZZ",
        );
        fragment::create_fragment(&vault, &linker).unwrap();
        open_index(&vault).unwrap();

        // Arrives through a sync (a pull, an editor)…
        let later = with_id(
            make_fragment("Later note", ""),
            "01KAAA0000000000000000000A",
        );
        fragment::create_fragment(&vault, &later).unwrap();
        let conn = open_index(&vault).unwrap();
        assert_eq!(backlink_ids(&conn, &later.id), vec![linker.id.clone()]);

        // …or through the write path. "Later" now prefixes two titles, and
        // `[[01KAAA]]` two IDs: both links become ambiguous and drop out.
        let other = with_id(
            make_fragment("Later notes", ""),
            "01KAAA0000000000000000000B",
        );
        fragment::create_fragment(&vault, &other).unwrap();
        index_fragment_auto(&conn, &other, &vault).unwrap();
        assert_eq!(
            backlink_ids(&conn, &later.id),
            vec![linker.id.clone()],
            "exact title still wins"
        );
        assert!(backlink_ids(&conn, &other.id).is_empty());
    }

    /// Frontmatter `links:` usually hold a short prefix. Backlinks, the vault
    /// graph and `linked:` all have to see it as a link to the full ID, even
    /// when the linker was indexed before its target existed.
    #[test]
    fn frontmatter_prefix_links_resolve_once_their_target_is_indexed() {
        let (_tmp, vault) = test_vault();
        let target = with_id(make_fragment("Target", ""), "01KAAA0000000000000000000A");
        let mut linker = with_id(make_fragment("Linker", ""), "01JZZZZZZZZZZZZZZZZZZZZZZZ");
        // A lowercase 10-character prefix, and the same target again in the
        // body: one link, not two.
        linker.links = vec!["01kaaa0000".into()];
        linker.body = "See [[01KAAA0000000000000000000A]].".into();
        fragment::create_fragment(&vault, &linker).unwrap();
        let conn = open_index(&vault).unwrap();
        assert!(
            all_links(&conn).unwrap().is_empty(),
            "dangling until the target exists"
        );

        fragment::create_fragment(&vault, &target).unwrap();
        index_fragment_auto(&conn, &target, &vault).unwrap();
        assert_eq!(backlink_ids(&conn, &target.id), vec![linker.id.clone()]);
        assert_eq!(
            all_links(&conn).unwrap(),
            vec![(linker.id.clone(), target.id.clone())]
        );

        // Rebuilt from the files, in whatever order sync reads them.
        drop(conn);
        let conn = reindex(&vault)
            .map(|_| open_index(&vault).unwrap())
            .unwrap();
        assert_eq!(backlink_ids(&conn, &target.id), vec![linker.id.clone()]);

        std::fs::remove_file(fragment_path(&vault, &target.id)).unwrap();
        remove_from_index(&conn, &target.id).unwrap();
        assert!(all_links(&conn).unwrap().is_empty());
    }

    #[test]
    fn retitling_or_deleting_a_fragment_updates_links_to_it() {
        let (_tmp, vault) = test_vault();
        let target = with_id(make_fragment("Old name", ""), "01KAAA0000000000000000000A");
        let linker = with_id(
            make_fragment("Linker", "See [[Old name]]."),
            "01JZZZZZZZZZZZZZZZZZZZZZZZ",
        );
        fragment::create_fragment(&vault, &target).unwrap();
        fragment::create_fragment(&vault, &linker).unwrap();
        let conn = open_index(&vault).unwrap();
        assert_eq!(backlink_ids(&conn, &target.id), vec![linker.id.clone()]);

        let renamed = Fragment {
            title: "New name".into(),
            ..target.clone()
        };
        std::fs::write(
            fragment_path(&vault, &target.id),
            fragment::serialize_fragment(&renamed),
        )
        .unwrap();
        let conn = open_index(&vault).unwrap();
        assert!(backlink_ids(&conn, &target.id).is_empty());

        // Name it again, then delete it through the write path.
        index_fragment_auto(&conn, &target, &vault).unwrap();
        assert_eq!(backlink_ids(&conn, &target.id), vec![linker.id.clone()]);
        std::fs::remove_file(fragment_path(&vault, &target.id)).unwrap();
        remove_from_index(&conn, &target.id).unwrap();
        let dangling: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fragment_links WHERE target_id = ?1",
                [&target.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dangling, 0);
    }

    #[test]
    fn an_index_from_another_version_is_rebuilt() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Real", "");
        fragment::create_fragment(&vault, &frag).unwrap();
        let conn = open_index(&vault).unwrap();
        conn.execute(
            "INSERT INTO fragments (id, type, title, created_at, updated_at, body) \
             VALUES ('STALE', 'note', 'Stale row', '', '', '')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
        drop(conn);

        let conn = open_index(&vault).unwrap();
        assert_eq!(indexed_titles(&conn), vec!["Real"]);
        assert_eq!(user_version(&conn).unwrap(), index_format());
    }

    /// An index left by a build whose schema differed from this one but whose
    /// stamp was the same: the first write used to fail with "no such column".
    #[test]
    fn an_index_from_a_build_with_another_schema_is_rebuilt() {
        let (_tmp, vault) = test_vault();
        let conn = Connection::open(vault.join("index.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE fragments (id TEXT PRIMARY KEY, type TEXT NOT NULL, title TEXT NOT NULL,
                status TEXT, priority TEXT, due TEXT, assignee TEXT, created_by TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL, body TEXT NOT NULL,
                extra_json TEXT, attachment_count INTEGER NOT NULL DEFAULT 0,
                archived INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE fragment_tags (fragment_id TEXT NOT NULL, tag TEXT NOT NULL,
                PRIMARY KEY (fragment_id, tag));
             CREATE VIRTUAL TABLE fragments_fts USING fts5(id UNINDEXED, title, body, tags);
             CREATE TABLE fragment_links (source_id TEXT NOT NULL, target_id TEXT NOT NULL,
                from_body INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (source_id, target_id));
             CREATE TABLE fragment_files (id TEXT PRIMARY KEY, mtime_ns INTEGER NOT NULL,
                size INTEGER NOT NULL, revision TEXT NOT NULL, checked_at_ns INTEGER NOT NULL,
                error TEXT);
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(conn);

        let frag = make_fragment("Written after", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();
        let conn = open_index(&vault).unwrap();
        index_fragment_auto(&conn, &frag, &vault).unwrap();
        assert_eq!(indexed_titles(&conn), vec!["Written after"]);
    }

    #[test]
    fn the_format_stamp_follows_the_schema() {
        let format = index_format();
        assert!(format >= 1 << 30 && format <= i64::from(i32::MAX));
        assert_eq!(format, index_format(), "stable within a build");
    }

    #[test]
    fn a_damaged_index_is_replaced() {
        let (_tmp, vault) = test_vault();
        let frag = make_fragment("Survivor", "");
        fragment::create_fragment(&vault, &frag).unwrap();
        std::fs::write(
            vault.join("index.db"),
            b"definitely not sqlite, but long enough to look like a header..........",
        )
        .unwrap();

        let conn = open_index(&vault).unwrap();
        assert_eq!(indexed_titles(&conn), vec!["Survivor"]);
    }

    #[test]
    fn reindex_reports_files_it_could_not_index() {
        let (_tmp, vault) = test_vault();
        fragment::create_fragment(&vault, &make_fragment("Good", "")).unwrap();
        let bad = make_fragment("Bad", "");
        std::fs::write(fragment_path(&vault, &bad.id), [0xff, 0xfe, 0x00]).unwrap();

        let report = reindex(&vault).unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(report.problems.len(), 1);
        assert!(report.problems[0].reason.contains("UTF-8"));

        // A rebuild reports it again: it starts from nothing.
        assert_eq!(reindex(&vault).unwrap().problems.len(), 1);
    }
}
