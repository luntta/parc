//! Attachment text index sync, cache table (`attachment_text`), background
//! fill, and reader queries.
//!
//! `attachment_files` and `attachment_fts` live in `SCHEMA_SQL` (rebuilt when
//! the index schema changes). `attachment_text` and `attachment_lease` live
//! outside `SCHEMA_SQL` (like `meaning_*`), so an index rebuild keeps cached
//! extracted text for vaults using `attachments.text: index`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;

use crate::attachment::{self, list_attachments_by_id, validate_attachment_filename};
use crate::config::{load_config, AttachmentTextMode};
use crate::error::ParcError;
use crate::extract::sidecar::{read_sidecar, write_sidecar, SidecarState};
use crate::extract::{
    sha256_file, ExtractLimits, ExtractOutcome, Extracted, PartUnit, TextFormat, TEXT_FORMAT,
};
use crate::fragment;
use crate::quote::{self, QuoteLocation};

const CACHE_SQL: &str = "
CREATE TABLE IF NOT EXISTS attachment_text (
    sha256      TEXT PRIMARY KEY,
    text_format INTEGER NOT NULL,
    unit        TEXT NOT NULL,
    format      TEXT NOT NULL,
    title       TEXT,
    parts_json  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS attachment_lease (
    one      INTEGER PRIMARY KEY CHECK (one = 1),
    holder   TEXT NOT NULL,
    until_ms INTEGER NOT NULL
);
";

/// Ensure the out-of-schema `attachment_text` and `attachment_lease` tables exist.
pub fn ensure_cache_tables(conn: &Connection) -> Result<(), ParcError> {
    conn.execute_batch(CACHE_SQL)?;
    Ok(())
}

/// Summary of an attachment's extracted-text status for `parc attachments` and
/// `attachment.list`.
#[derive(Debug, Clone, Serialize)]
pub struct AttachmentTextStatus {
    pub filename: String,
    pub size: u64,
    /// `ready`, `pending`, `no_text`, or `error`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<PartUnit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<TextFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub parts: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What [`fill`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FillReport {
    pub extracted: usize,
    pub cached: usize,
    pub no_text: usize,
    pub errors: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiskStat {
    size: i64,
    mtime_ns: i64,
}

#[derive(Debug, Clone)]
struct TrackedRow {
    stat: DiskStat,
    text_format: i64,
    status: String,
}

fn scan_vault_attachments(
    conn: &Connection,
    vault: &Path,
) -> Result<HashMap<(String, String), DiskStat>, ParcError> {
    let mut stmt = conn.prepare_cached("SELECT id FROM fragments")?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    let mut on_disk = HashMap::new();
    for id in ids {
        let Ok(dir) = attachment::attachment_dir(vault, &id) else {
            continue;
        };
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == attachment::TEXT_SIDECAR_DIR {
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if !meta.is_file() || meta.file_type().is_symlink() {
                continue;
            }
            let mtime_ns = meta.modified().map(system_time_ns).unwrap_or(0);
            let size = i64::try_from(meta.len()).unwrap_or(i64::MAX);
            on_disk.insert((id.clone(), name.to_string()), DiskStat { size, mtime_ns });
        }
    }
    Ok(on_disk)
}

fn load_tracked(conn: &Connection) -> Result<HashMap<(String, String), TrackedRow>, ParcError> {
    let mut stmt = conn.prepare_cached(
        "SELECT fragment_id, filename, size, mtime_ns, text_format, status FROM attachment_files",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
            TrackedRow {
                stat: DiskStat {
                    size: row.get(2)?,
                    mtime_ns: row.get(3)?,
                },
                text_format: row.get(4)?,
                status: row.get(5)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
}

/// Fast read-only check so `index::sync` takes no write lock when attachments
/// are already up to date.
pub fn attachments_need_sync(conn: &Connection, vault: &Path) -> Result<bool, ParcError> {
    let on_disk = scan_vault_attachments(conn, vault)?;
    let tracked = load_tracked(conn)?;
    if on_disk.len() != tracked.len() {
        return Ok(true);
    }
    for (key, stat) in &on_disk {
        match tracked.get(key) {
            Some(row)
                if row.stat == *stat
                    && !(row.status == "error" && row.text_format < i64::from(TEXT_FORMAT)) => {}
            _ => return Ok(true),
        }
    }
    Ok(false)
}

/// Bring `attachment_files` and `attachment_fts` up to date with `attachments/`
/// WITHOUT ever running extraction: uses fresh sidecars or `attachment_text`
/// cache when available, otherwise marks entries `pending`.
pub fn sync_attachments(conn: &Connection, vault: &Path) -> Result<bool, ParcError> {
    let on_disk = scan_vault_attachments(conn, vault)?;
    let tracked = load_tracked(conn)?;

    let mut changed = false;

    // Remove rows for attachments that no longer exist on disk (or whose fragment is gone).
    for key in tracked.keys() {
        if !on_disk.contains_key(key) {
            delete_attachment_rows(conn, &key.0, &key.1)?;
            changed = true;
        }
    }

    let has_cache = cache_table_exists(conn)?;

    for ((frag_id, filename), stat) in &on_disk {
        let up_to_date = tracked
            .get(&(frag_id.clone(), filename.clone()))
            .is_some_and(|row| {
                row.stat == *stat
                    && !(row.status == "error" && row.text_format < i64::from(TEXT_FORMAT))
            });
        if up_to_date {
            continue;
        }
        changed = true;

        let Ok(path) = attachment::attachment_path(vault, frag_id, filename) else {
            continue;
        };
        let Ok(sha256) = sha256_file(&path) else {
            continue;
        };

        // 1. Check fresh or newer-version sidecar on disk.
        match read_sidecar(vault, frag_id, filename, &sha256) {
            SidecarState::Fresh(ext) | SidecarState::NewerVersion(ext) => {
                write_indexed_attachment(conn, frag_id, filename, &sha256, *stat, &ext)?;
                continue;
            }
            SidecarState::Stale | SidecarState::Missing => {}
        }

        // 2. Check out-of-schema `attachment_text` cache by sha256.
        if has_cache {
            if let Some(ext) = read_cache(conn, &sha256)? {
                write_indexed_attachment(conn, frag_id, filename, &sha256, *stat, &ext)?;
                continue;
            }
        }

        // 3. Otherwise mark pending for `extract::fill`. Never extract during sync.
        mark_pending(conn, frag_id, filename, &sha256, *stat)?;
    }

    Ok(changed)
}

/// Whether any attachment in the index is waiting for extraction.
pub fn has_pending(conn: &Connection) -> Result<bool, ParcError> {
    Ok(pending_count(conn)? > 0)
}

/// How many attachments are waiting for their text to be extracted.
pub fn pending_count(conn: &Connection) -> Result<usize, ParcError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attachment_files WHERE status = 'pending'",
        [],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(count).unwrap_or(0))
}

/// Extract text for every pending attachment (and, in `sidecar` mode, backfill
/// missing/stale sidecars from cached text when called e.g. from `parc reindex`).
pub fn fill(vault: &Path, conn: &Connection) -> Result<FillReport, ParcError> {
    fill_limit(vault, conn, None, true)
}

/// Extract text for up to `max_files` pending attachments. When `backfill_sidecars`
/// is true and `config.attachments.text == Sidecar`, also writes missing sidecars
/// for already-indexed files (e.g. after switching from `index` to `sidecar` and
/// running `parc reindex`).
pub fn fill_limit(
    vault: &Path,
    conn: &Connection,
    max_files: Option<usize>,
    backfill_sidecars: bool,
) -> Result<FillReport, ParcError> {
    ensure_cache_tables(conn)?;
    let mode = load_config(vault)?.attachments.text;
    let mut report = FillReport::default();

    // If reindexing in sidecar mode, materialize missing/stale sidecars from cached text.
    if backfill_sidecars && mode == AttachmentTextMode::Sidecar {
        let mut stmt = conn.prepare(
            "SELECT fragment_id, filename, sha256 FROM attachment_files WHERE status = 'ready'",
        )?;
        let ready: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (frag_id, filename, sha256) in ready {
            if matches!(
                read_sidecar(vault, &frag_id, &filename, &sha256),
                SidecarState::Missing | SidecarState::Stale
            ) {
                if let Some(ext) = read_cache(conn, &sha256)? {
                    let _ = write_sidecar(vault, &frag_id, &filename, &sha256, &ext);
                }
            }
        }
    }

    let mut stmt = conn.prepare(
        "SELECT fragment_id, filename FROM attachment_files
         WHERE status = 'pending'
         ORDER BY fragment_id, filename",
    )?;
    let pending: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;

    let total = pending.len();
    let take = max_files.map_or(total, |m| m.min(total));

    for (frag_id, filename) in pending.into_iter().take(take) {
        let outcome = match fill_one_inner(vault, conn, &frag_id, &filename, mode) {
            Ok(outcome) => outcome,
            Err(e) => {
                // A failed sidecar write must not starve the rest of the queue.
                conn.execute(
                    "UPDATE attachment_files SET status='error', text_format=?3, error=?4
                    WHERE fragment_id=?1 AND filename=?2 AND status='pending'",
                    rusqlite::params![frag_id, filename, TEXT_FORMAT, e.to_string()],
                )?;
                FillOneOutcome::Error
            }
        };
        match outcome {
            FillOneOutcome::Extracted(_) => report.extracted += 1,
            FillOneOutcome::Cached(_) => report.cached += 1,
            FillOneOutcome::NoText => report.no_text += 1,
            FillOneOutcome::Error => report.errors += 1,
            FillOneOutcome::Missing => {}
        }
    }

    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM attachment_files WHERE status = 'pending'",
        [],
        |r| r.get(0),
    )?;
    report.remaining = remaining.max(0) as usize;
    Ok(report)
}

enum FillOneOutcome {
    Extracted(Extracted),
    Cached(Extracted),
    NoText,
    Error,
    Missing,
}

/// Extract, store (sidecar or index cache), and index one specific attachment.
/// Returns `Ok(Some(extracted))` when text was obtained, `Ok(None)` when the
/// file has no extractable text or extraction failed (failures are recorded in
/// `attachment_files.error` so broken files aren't retried until changed).
pub fn fill_one(
    vault: &Path,
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
) -> Result<Option<Extracted>, ParcError> {
    ensure_cache_tables(conn)?;
    let mode = load_config(vault)?.attachments.text;
    match fill_one_inner(vault, conn, fragment_id, filename, mode)? {
        FillOneOutcome::Extracted(ext) | FillOneOutcome::Cached(ext) => Ok(Some(ext)),
        FillOneOutcome::NoText | FillOneOutcome::Error | FillOneOutcome::Missing => Ok(None),
    }
}

fn fill_one_inner(
    vault: &Path,
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
    mode: AttachmentTextMode,
) -> Result<FillOneOutcome, ParcError> {
    fragment::validate_id(fragment_id)?;
    validate_attachment_filename(filename)?;

    let found = attachment::attachment_path(vault, fragment_id, filename)
        .ok()
        .and_then(|path| std::fs::symlink_metadata(&path).ok().map(|m| (path, m)))
        .filter(|(_, m)| m.is_file() && !m.file_type().is_symlink());
    let Some((path, meta)) = found else {
        delete_attachment_rows(conn, fragment_id, filename)?;
        return Ok(FillOneOutcome::Missing);
    };
    let stat = DiskStat {
        size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
        mtime_ns: meta.modified().map(system_time_ns).unwrap_or(0),
    };
    let sha256 = sha256_file(&path)?;

    // 1. Fresh or newer sidecar already on disk?
    match read_sidecar(vault, fragment_id, filename, &sha256) {
        SidecarState::Fresh(ext) | SidecarState::NewerVersion(ext) => {
            let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
            write_indexed_attachment(&tx, fragment_id, filename, &sha256, stat, &ext)?;
            tx.commit()?;
            return Ok(FillOneOutcome::Cached(ext));
        }
        SidecarState::Stale | SidecarState::Missing => {}
    }

    // 2. Cached in `attachment_text` by sha256?
    if let Some(ext) = read_cache(conn, &sha256)? {
        if mode == AttachmentTextMode::Sidecar {
            write_sidecar(vault, fragment_id, filename, &sha256, &ext)?;
        }
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        write_indexed_attachment(&tx, fragment_id, filename, &sha256, stat, &ext)?;
        tx.commit()?;
        return Ok(FillOneOutcome::Cached(ext));
    }

    // 3. Extract via the isolated child process.
    let limits = ExtractLimits::default();
    let outcome = extract_file(&path, &limits);

    match outcome {
        Ok(ExtractOutcome::Extracted(ext)) => {
            if mode == AttachmentTextMode::Sidecar {
                write_sidecar(vault, fragment_id, filename, &sha256, &ext)?;
            } else {
                write_cache(conn, &sha256, &ext)?;
            }
            let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
            write_indexed_attachment(&tx, fragment_id, filename, &sha256, stat, &ext)?;
            tx.commit()?;
            Ok(FillOneOutcome::Extracted(ext))
        }
        Ok(ExtractOutcome::NoText) => {
            let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
            delete_fts_rows(&tx, fragment_id, filename)?;
            tx.prepare_cached(
                "INSERT OR REPLACE INTO attachment_files
                 (fragment_id, filename, sha256, size, mtime_ns, unit, format, title, parts, text_format, status, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, 0, ?6, 'no_text', NULL)",
            )?
            .execute(rusqlite::params![
                fragment_id,
                filename,
                sha256,
                stat.size,
                stat.mtime_ns,
                i64::from(TEXT_FORMAT),
            ])?;
            tx.commit()?;
            Ok(FillOneOutcome::NoText)
        }
        Err(err) => {
            let reason = err.to_string();
            let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
            delete_fts_rows(&tx, fragment_id, filename)?;
            tx.prepare_cached(
                "INSERT OR REPLACE INTO attachment_files
                 (fragment_id, filename, sha256, size, mtime_ns, unit, format, title, parts, text_format, status, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, 0, ?6, 'error', ?7)",
            )?
            .execute(rusqlite::params![
                fragment_id,
                filename,
                sha256,
                stat.size,
                stat.mtime_ns,
                i64::from(TEXT_FORMAT),
                reason,
            ])?;
            tx.commit()?;
            Ok(FillOneOutcome::Error)
        }
    }
}

#[cfg(not(test))]
fn extract_file(path: &Path, limits: &ExtractLimits) -> Result<ExtractOutcome, ParcError> {
    crate::extract::isolated(path, limits)
}

// Only the core unit-test harness bypasses re-exec. Never present in a
// production library (including one embedded by another executable).
#[cfg(test)]
fn extract_file(path: &Path, limits: &ExtractLimits) -> Result<ExtractOutcome, ParcError> {
    let bytes = crate::secure_fs::read_regular(path)?;
    crate::extract::in_process(&bytes, path.file_name().and_then(|n| n.to_str()), limits)
}

/// Read extracted parts for `(fragment_id, filename)`, filling on demand if pending.
pub fn read_parts(
    vault: &Path,
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
) -> Result<Option<Extracted>, ParcError> {
    read_parts_impl(vault, conn, fragment_id, filename, true)
}

/// Read cached text, or extract transiently without writing a sidecar or database row.
pub fn read_parts_read_only(
    vault: &Path,
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
) -> Result<Option<Extracted>, ParcError> {
    read_parts_impl(vault, conn, fragment_id, filename, false)
}

fn read_parts_impl(
    vault: &Path,
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
    persist: bool,
) -> Result<Option<Extracted>, ParcError> {
    let full_id = fragment::resolve_id(vault, fragment_id)?;
    validate_attachment_filename(filename)?;

    let path = attachment::attachment_path(vault, &full_id, filename)?;
    let meta = std::fs::symlink_metadata(&path).map_err(|_| {
        ParcError::ValidationError(format!(
            "attachment '{}' not found on fragment '{}'",
            filename, full_id
        ))
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ParcError::ValidationError(format!(
            "invalid attachment '{}'",
            filename
        )));
    }
    let sha256 = sha256_file(&path)?;

    match read_sidecar(vault, &full_id, filename, &sha256) {
        SidecarState::Fresh(ext) | SidecarState::NewerVersion(ext) => return Ok(Some(ext)),
        SidecarState::Stale | SidecarState::Missing => {}
    }

    if cache_table_exists(conn)? {
        if let Some(ext) = read_cache(conn, &sha256)? {
            return Ok(Some(ext));
        }
    }

    // Check if indexed as no_text or error already at current TEXT_FORMAT.
    let existing: Option<(String, i64, Option<String>)> = conn
        .prepare_cached(
            "SELECT status, text_format, error FROM attachment_files
             WHERE fragment_id = ?1 AND filename = ?2 AND sha256 = ?3",
        )?
        .query_row(rusqlite::params![full_id, filename, sha256], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    if let Some((status, tf, err)) = existing {
        if tf >= i64::from(TEXT_FORMAT) {
            if status == "no_text" {
                return Ok(None);
            }
            if status == "error" {
                return Err(ParcError::ValidationError(
                    err.unwrap_or_else(|| "extraction failed".into()),
                ));
            }
            if status == "ready" {
                if let Some(ext) = reconstruct_from_fts(conn, &full_id, filename)? {
                    return Ok(Some(ext));
                }
            }
        }
    }

    if !persist {
        return Ok(match extract_file(&path, &ExtractLimits::default())? {
            ExtractOutcome::Extracted(text) => Some(text),
            ExtractOutcome::NoText => None,
        });
    }
    // Extract on demand if still pending or uncached.
    fill_one(vault, conn, &full_id, filename)
}

fn reconstruct_from_fts(
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
) -> Result<Option<Extracted>, ParcError> {
    let meta: Option<(Option<String>, Option<String>, Option<String>)> = conn
        .prepare_cached(
            "SELECT unit, format, title FROM attachment_files
             WHERE fragment_id = ?1 AND filename = ?2 AND status = 'ready'",
        )?
        .query_row(rusqlite::params![fragment_id, filename], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let Some((unit_s, format_s, title)) = meta else {
        return Ok(None);
    };
    let unit = unit_s
        .as_deref()
        .and_then(PartUnit::parse)
        .unwrap_or(PartUnit::Page);
    let format = format_s
        .as_deref()
        .and_then(TextFormat::parse)
        .unwrap_or(TextFormat::Text);
    let mut stmt = conn.prepare_cached(
        "SELECT part, text FROM attachment_fts
         WHERE fragment_id = ?1 AND filename = ?2
         ORDER BY CAST(part AS INTEGER) ASC",
    )?;
    let parts: Vec<String> = stmt
        .query_map(rusqlite::params![fragment_id, filename], |r| r.get(1))?
        .collect::<Result<_, _>>()?;
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(Extracted {
        title,
        unit,
        format,
        parts,
    }))
}

/// Locate a quote inside an attachment's extracted parts.
pub fn locate(
    vault: &Path,
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
    quote_text: &str,
    part_hint: Option<usize>,
) -> Result<Option<QuoteLocation>, ParcError> {
    let Some(ext) = read_parts(vault, conn, fragment_id, filename)? else {
        return Ok(None);
    };
    Ok(quote::locate(&ext.parts, quote_text, part_hint))
}

/// List a fragment's attachments along with each file's extracted-text status.
pub fn list_status(
    conn: &Connection,
    vault: &Path,
    id_or_prefix: &str,
) -> Result<Vec<AttachmentTextStatus>, ParcError> {
    let full_id = fragment::resolve_id(vault, id_or_prefix)?;
    let files = list_attachments_by_id(vault, &full_id)?;
    let mut stmt = conn.prepare_cached(
        "SELECT filename, status, unit, format, title, parts, error
         FROM attachment_files WHERE fragment_id = ?1",
    )?;
    let mut rows: HashMap<
        String,
        (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
        ),
    > = HashMap::new();
    for r in stmt.query_map([&full_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<String>>(6)?,
            ),
        ))
    })? {
        let (name, info) = r?;
        rows.insert(name, info);
    }

    Ok(files
        .into_iter()
        .map(|a| {
            if let Some((status, unit, format, title, parts, error)) = rows.remove(&a.filename) {
                AttachmentTextStatus {
                    filename: a.filename,
                    size: a.size,
                    status,
                    unit: unit.as_deref().and_then(PartUnit::parse),
                    format: format.as_deref().and_then(TextFormat::parse),
                    title,
                    parts: parts.max(0) as usize,
                    error,
                }
            } else {
                AttachmentTextStatus {
                    filename: a.filename,
                    size: a.size,
                    status: "pending".to_string(),
                    unit: None,
                    format: None,
                    title: None,
                    parts: 0,
                    error: None,
                }
            }
        })
        .collect())
}

/// Claim the extraction lease so two servers on the same vault don't extract at once.
pub fn claim_lease(conn: &Connection, holder: &str, seconds: u64) -> Result<bool, ParcError> {
    ensure_cache_tables(conn)?;
    let now = now_ms();
    let until = now + (seconds as i64) * 1000;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let current: Option<(String, i64)> = tx
        .query_row(
            "SELECT holder, until_ms FROM attachment_lease WHERE one = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let ok = match current {
        Some((h, u)) if h != holder && u > now => false,
        _ => {
            tx.execute(
                "INSERT OR REPLACE INTO attachment_lease (one, holder, until_ms) VALUES (1, ?1, ?2)",
                rusqlite::params![holder, until],
            )?;
            true
        }
    };
    tx.commit()?;
    Ok(ok)
}

pub fn release_lease(conn: &Connection, holder: &str) -> Result<(), ParcError> {
    if !cache_table_exists(conn)? {
        return Ok(());
    }
    conn.execute(
        "DELETE FROM attachment_lease WHERE one = 1 AND holder = ?1",
        [holder],
    )?;
    Ok(())
}

// ── Internal SQL helpers ───────────────────────────────────────────────

fn cache_table_exists(conn: &Connection) -> Result<bool, ParcError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'attachment_text'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn read_cache(conn: &Connection, sha256: &str) -> Result<Option<Extracted>, ParcError> {
    let row: Option<(i64, String, String, Option<String>, String)> = conn
        .prepare_cached(
            "SELECT text_format, unit, format, title, parts_json
             FROM attachment_text WHERE sha256 = ?1",
        )?
        .query_row([sha256], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .optional()?;
    let Some((tf, unit_s, format_s, title, parts_json)) = row else {
        return Ok(None);
    };
    if tf < i64::from(TEXT_FORMAT) {
        return Ok(None);
    }
    let unit = PartUnit::parse(&unit_s).unwrap_or(PartUnit::Page);
    let format = TextFormat::parse(&format_s).unwrap_or(TextFormat::Text);
    let parts: Vec<String> = serde_json::from_str(&parts_json)?;
    Ok(Some(Extracted {
        title,
        unit,
        format,
        parts,
    }))
}

fn write_cache(conn: &Connection, sha256: &str, ext: &Extracted) -> Result<(), ParcError> {
    ensure_cache_tables(conn)?;
    let parts_json = serde_json::to_string(&ext.parts)?;
    conn.prepare_cached(
        "INSERT OR REPLACE INTO attachment_text (sha256, text_format, unit, format, title, parts_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(rusqlite::params![
        sha256,
        i64::from(TEXT_FORMAT),
        ext.unit.as_str(),
        ext.format.as_str(),
        ext.title.as_deref(),
        parts_json,
    ])?;
    Ok(())
}

fn write_indexed_attachment(
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
    sha256: &str,
    stat: DiskStat,
    ext: &Extracted,
) -> Result<(), ParcError> {
    delete_fts_rows(conn, fragment_id, filename)?;
    conn.prepare_cached(
        "INSERT OR REPLACE INTO attachment_files
         (fragment_id, filename, sha256, size, mtime_ns, unit, format, title, parts, text_format, status, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'ready', NULL)",
    )?
    .execute(rusqlite::params![
        fragment_id,
        filename,
        sha256,
        stat.size,
        stat.mtime_ns,
        ext.unit.as_str(),
        ext.format.as_str(),
        ext.title.as_deref(),
        ext.parts.len() as i64,
        i64::from(TEXT_FORMAT),
    ])?;

    let mut insert_fts = conn.prepare_cached(
        "INSERT INTO attachment_fts (fragment_id, filename, part, text)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (idx, part_text) in ext.parts.iter().enumerate() {
        insert_fts.execute(rusqlite::params![
            fragment_id,
            filename,
            (idx + 1) as i64,
            part_text,
        ])?;
    }
    Ok(())
}

pub(crate) fn queue_import(
    conn: &Connection,
    vault: &Path,
    id: &str,
    filename: &str,
) -> Result<(), ParcError> {
    let meta = std::fs::symlink_metadata(attachment::attachment_path(vault, id, filename)?)?;
    let stat = DiskStat {
        size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
        mtime_ns: system_time_ns(meta.modified()?),
    };
    mark_pending(conn, id, filename, "", stat)
}

fn mark_pending(
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
    sha256: &str,
    stat: DiskStat,
) -> Result<(), ParcError> {
    delete_fts_rows(conn, fragment_id, filename)?;
    conn.prepare_cached(
        "INSERT OR REPLACE INTO attachment_files
         (fragment_id, filename, sha256, size, mtime_ns, unit, format, title, parts, text_format, status, error)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, 0, 0, 'pending', NULL)",
    )?
    .execute(rusqlite::params![
        fragment_id,
        filename,
        sha256,
        stat.size,
        stat.mtime_ns,
    ])?;
    Ok(())
}

fn delete_fts_rows(conn: &Connection, fragment_id: &str, filename: &str) -> Result<(), ParcError> {
    conn.prepare_cached("DELETE FROM attachment_fts WHERE fragment_id = ?1 AND filename = ?2")?
        .execute(rusqlite::params![fragment_id, filename])?;
    Ok(())
}

fn delete_attachment_rows(
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
) -> Result<(), ParcError> {
    delete_fts_rows(conn, fragment_id, filename)?;
    conn.prepare_cached("DELETE FROM attachment_files WHERE fragment_id = ?1 AND filename = ?2")?
        .execute(rusqlite::params![fragment_id, filename])?;
    Ok(())
}

/// Remove all attachment rows for a fragment (when the fragment is deleted).
pub fn remove_fragment_attachments(conn: &Connection, fragment_id: &str) -> Result<(), ParcError> {
    conn.prepare_cached("DELETE FROM attachment_fts WHERE fragment_id = ?1")?
        .execute([fragment_id])?;
    conn.prepare_cached("DELETE FROM attachment_files WHERE fragment_id = ?1")?
        .execute([fragment_id])?;
    Ok(())
}

/// The bytes did not change. Transfer their index and pending status along
/// with them rather than running an extractor on the foreground move request.
pub(crate) fn move_indexed_attachment(
    conn: &Connection,
    from: &str,
    to: &str,
    file: &str,
) -> Result<(), ParcError> {
    let tx = conn.unchecked_transaction()?;
    // Opening the writer's connection may already have synced the move.
    let tracked: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM attachment_files
        WHERE fragment_id=?1 AND filename=?2)",
        [from, file],
        |r| r.get(0),
    )?;
    if !tracked {
        return Ok(());
    }
    delete_attachment_rows(&tx, to, file)?;
    tx.execute(
        "UPDATE attachment_files SET fragment_id=?2 WHERE fragment_id=?1 AND filename=?3",
        rusqlite::params![from, to, file],
    )?;
    tx.execute(
        "UPDATE attachment_fts SET fragment_id=?2 WHERE fragment_id=?1 AND filename=?3",
        rusqlite::params![from, to, file],
    )?;
    tx.commit()?;
    Ok(())
}

/// Remove a single attachment's index rows (when detached).
pub fn remove_one_attachment(
    conn: &Connection,
    fragment_id: &str,
    filename: &str,
) -> Result<(), ParcError> {
    delete_attachment_rows(conn, fragment_id, filename)
}

/// Look up the set of fragment IDs that currently have attachment files tracked.
#[allow(dead_code)]
pub(crate) fn tracked_fragment_ids(conn: &Connection) -> Result<HashSet<String>, ParcError> {
    let mut stmt = conn.prepare_cached("SELECT DISTINCT fragment_id FROM attachment_files")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn system_time_ns(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_nanos()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_nanos()).unwrap_or(i64::MAX),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
