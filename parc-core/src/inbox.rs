//! Vault inbox: turn files dropped into `<vault>/inbox/` into `source`
//! fragments.
//!
//! Security & reliability rules:
//! - The inbox folder is ALWAYS `<vault>/inbox/` — never read from `config.yml`.
//! - Symlinks, directories, dotfiles, and partial browser downloads (`.part`,
//!   `.crdownload`, `.tmp`, `.download`, `.partial`) are skipped.
//! - A file must be settled (mtime older than [`SETTLE_DURATION`] and
//!   size+mtime unchanged right before move) so an in-progress copy or browser
//!   save is never swept half-written.
//! - Every fragment write and move goes through [`Writer`]. If moving the file
//!   into `attachments/<id>/` fails, the newly created empty `source` fragment
//!   may be trashed, but automatic cleanup never purges original bytes.

use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::attachment::{validate_attachment_filename, AttachMode};
use crate::config::load_config;
use crate::error::ParcError;
use crate::fragment::new_fragment;
use crate::ops::Writer;
use crate::schema::load_schemas;
use crate::vault;

/// How long a file's mtime must be quiet before `sweep` picks it up.
pub const SETTLE_DURATION: Duration = Duration::from_secs(1);

const IMPORT_BATCH: usize = 16;
const PARTIAL_SUFFIXES: &[&str] = &[".part", ".crdownload", ".tmp", ".download", ".partial"];

/// One file successfully swept from `<vault>/inbox/` into a `source` fragment.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SweptItem {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub text_extracted: bool,
}

/// Summary of one [`sweep`] pass over `<vault>/inbox/`.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct SweepReport {
    pub created: Vec<SweptItem>,
    pub waiting: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<ImportFailure>,
    /// Metadata jobs left for the background extractor, and ones just reconciled.
    pub pending: usize,
    pub completed: usize,
    pub more: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportFailure {
    pub filename: String,
    pub error: String,
}

/// Sweep `<vault>/inbox/` with the default [`SETTLE_DURATION`].
pub fn sweep(writer: &mut Writer) -> Result<SweepReport, ParcError> {
    sweep_with_settle(writer, SETTLE_DURATION)
}

/// Sweep `<vault>/inbox/` using a custom quiet window (tests can pass
/// `Duration::ZERO` for deterministic runs).
pub fn sweep_with_settle(writer: &mut Writer, settle: Duration) -> Result<SweepReport, ParcError> {
    sweep_impl(writer, settle, false)
}

/// Short server pass: commit originals, leaving parsing to the existing
/// background worker. Bound each pass so other RPCs can run between batches.
pub fn sweep_deferred(writer: &mut Writer) -> Result<SweepReport, ParcError> {
    sweep_impl(writer, SETTLE_DURATION, true)
}

fn sweep_impl(
    writer: &mut Writer,
    settle: Duration,
    deferred: bool,
) -> Result<SweepReport, ParcError> {
    let vault_path = writer.vault().to_path_buf();
    let dir = vault::ensure_inbox_ready(&vault_path)?;

    let mut report = SweepReport::default();
    let jobs = if deferred {
        let conn = crate::index::init_index(&vault_path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS inbox_titles (
            id TEXT PRIMARY KEY, filename TEXT NOT NULL, revision TEXT NOT NULL)",
        )?;
        reconcile_titles(writer, &conn, &mut report)?;
        Some(conn)
    } else {
        None
    };
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        match entry.file_name().into_string() {
            Ok(name) => entries.push((name, entry.path())),
            Err(name) => report.skipped.push(name.to_string_lossy().into_owned()),
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    if entries.is_empty() {
        return Ok(report);
    }

    let config = load_config(&vault_path)?;
    let schemas = load_schemas(&vault_path)?;
    let source_schema = schemas
        .get_by_name("source")
        .ok_or_else(|| ParcError::SchemaNotFound("source".into()))?
        .clone();

    for (name, path) in entries {
        if should_skip_name(&name) {
            report.skipped.push(name);
            continue;
        }
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                report.failed.push(ImportFailure {
                    filename: name,
                    error: e.to_string(),
                });
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            report.skipped.push(name);
            continue;
        }
        if !is_settled(&meta, settle) {
            report.waiting.push(name);
            continue;
        }
        if validate_attachment_filename(&name).is_err() {
            report.skipped.push(name);
            continue;
        }

        // Re-check size and mtime immediately before creating/moving.
        let initial_len = meta.len();
        let initial_mtime = meta.modified().ok();
        let meta_now = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                report.failed.push(ImportFailure {
                    filename: name,
                    error: e.to_string(),
                });
                continue;
            }
        };
        if meta_now.file_type().is_symlink() || !meta_now.is_file() {
            report.skipped.push(name);
            continue;
        }
        if meta_now.len() != initial_len || meta_now.modified().ok() != initial_mtime {
            report.waiting.push(name);
            continue;
        }

        if deferred && report.created.len() >= IMPORT_BATCH {
            report.more = true;
            break;
        }
        let stem_title = title_from_filename(&name);
        let frag = new_fragment("source", &stem_title, &source_schema, &config);
        let created = match writer.create(frag) {
            Ok(frag) => frag,
            Err(e) => {
                report.failed.push(ImportFailure {
                    filename: name,
                    error: e.to_string(),
                });
                continue;
            }
        };

        match writer.attach_file_only(&created.id, &path, AttachMode::Move) {
            Ok((mut frag, filename)) => {
                let mut text_extracted = false;
                if let Some(conn) = &jobs {
                    // Register cheaply (stat only): no hashing, parsing or sidecar writes on this RPC.
                    let register = || -> Result<(), ParcError> {
                        let tx = conn.unchecked_transaction()?;
                        crate::extract::store::queue_import(&tx, &vault_path, &frag.id, &filename)?;
                        let revision = crate::fragment::revision(&vault_path, &frag.id)?;
                        tx.execute(
                            "INSERT OR REPLACE INTO inbox_titles VALUES (?1,?2,?3)",
                            rusqlite::params![frag.id, filename, revision],
                        )?;
                        tx.commit()?;
                        Ok(())
                    };
                    match register() {
                        Ok(()) => report.pending += 1,
                        Err(e) => writer.warn(format!(
                            "Imported {filename}, but could not queue its metadata: {e}"
                        )),
                    }
                } else if let Some(ext) = writer.extract_attached(&frag.id, &filename) {
                    text_extracted = true;
                    if let Some(title) = ext.title {
                        apply_title(writer, &mut frag, &title);
                    }
                }
                report.created.push(SweptItem {
                    id: frag.id,
                    title: frag.title,
                    filename,
                    text_extracted,
                });
            }
            Err(err) => {
                // A transfer may have failed after acquiring bytes. Never purge
                // on this path, even if rollback or storage inspection fails.
                let stored = crate::attachment::attachment_path(&vault_path, &created.id, &name);
                if matches!(stored.map(std::fs::symlink_metadata),
                            Ok(Err(ref e)) if e.kind() == std::io::ErrorKind::NotFound)
                {
                    if let Err(e) = writer.delete(&created.id) {
                        writer.warn(format!("Could not trash empty source {}: {e}", created.id));
                    }
                }
                report.failed.push(ImportFailure {
                    filename: name,
                    error: err.to_string(),
                });
            }
        }
    }
    Ok(report)
}

fn apply_title(writer: &mut Writer, frag: &mut crate::fragment::Fragment, raw: &str) {
    let title = clean_doc_title(raw);
    if title.is_empty() || title == frag.title {
        return;
    }
    let mut updated = frag.clone();
    updated.title = title;
    match writer.update(updated) {
        Ok(saved) => *frag = saved,
        Err(e) => writer.warn(format!("Could not set document title for {}: {e}", frag.id)),
    }
}

// Only the foreground sweep writes fragment metadata. A durable revision
// guard prevents a late title result from undoing an intervening user edit.
fn reconcile_titles(
    writer: &mut Writer,
    conn: &rusqlite::Connection,
    report: &mut SweepReport,
) -> Result<(), ParcError> {
    let mut stmt = conn.prepare(
        "SELECT j.id, j.revision, a.title, j.filename, a.error FROM inbox_titles j
        LEFT JOIN attachment_files a ON a.fragment_id=j.id AND a.filename=j.filename
        WHERE a.status IS NULL OR a.status != 'pending' LIMIT 32",
    )?;
    let jobs = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    for (id, expected, title, filename, error) in jobs {
        if let Some(error) = error {
            writer.warn(format!(
                "Kept {filename}, but text indexing failed: {error}. Retry with parc reindex."
            ));
        }
        if let Some(title) = title {
            if crate::fragment::revision(writer.vault(), &id).ok().as_ref() == Some(&expected) {
                let mut frag = crate::fragment::read_fragment(writer.vault(), &id)?;
                apply_title(writer, &mut frag, &title);
            }
        }
        conn.execute("DELETE FROM inbox_titles WHERE id=?1", [&id])?;
        report.completed += 1;
    }
    report.pending = conn.query_row("SELECT count(*) FROM inbox_titles", [], |r| r.get(0))?;
    Ok(())
}

fn should_skip_name(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    PARTIAL_SUFFIXES.iter().any(|s| lower.ends_with(s))
}

fn is_settled(meta: &std::fs::Metadata, settle: Duration) -> bool {
    if settle.is_zero() {
        return true;
    }
    let Ok(modified) = meta.modified() else {
        return false;
    };
    match SystemTime::now().duration_since(modified) {
        Ok(age) => age >= settle,
        // Future mtime (clock skew or just written): wait.
        Err(_) => false,
    }
}

/// Derive a clean initial title from an attachment filename stem.
pub fn title_from_filename(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let cleaned: String = stem
        .chars()
        .map(|c| match c {
            '_' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        filename.to_string()
    } else {
        collapsed
    }
}

fn clean_doc_title(raw: &str) -> String {
    let single_line: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(160).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment;
    use crate::hook::{HookStderr, ScriptRunner};
    use crate::index;

    fn setup() -> (tempfile::TempDir, std::path::PathBuf, Writer) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        vault::init_vault(&vault).unwrap();
        let writer = Writer::new(&vault, ScriptRunner::new(HookStderr::Capture));
        (tmp, vault, writer)
    }

    #[test]
    fn sweep_skips_dotfiles_partials_dirs_and_symlinks() {
        let (_tmp, vault, mut writer) = setup();
        let inbox = vault::inbox_dir(&vault);

        std::fs::write(inbox.join(".DS_Store"), b"junk").unwrap();
        std::fs::write(inbox.join("download.pdf.part"), b"partial").unwrap();
        std::fs::write(inbox.join("report.crdownload"), b"partial").unwrap();
        std::fs::write(inbox.join("temp.tmp"), b"partial").unwrap();
        std::fs::create_dir(inbox.join("nested_folder")).unwrap();
        #[cfg(unix)]
        {
            let outside = _tmp.path().join("outside.txt");
            std::fs::write(&outside, b"secret").unwrap();
            std::os::unix::fs::symlink(&outside, inbox.join("link.txt")).unwrap();
        }

        let report = sweep_with_settle(&mut writer, Duration::ZERO).unwrap();
        assert!(report.created.is_empty());
        assert!(report.waiting.is_empty());
        assert!(report.skipped.contains(&".DS_Store".to_string()));
        assert!(report.skipped.contains(&"download.pdf.part".to_string()));
        assert!(report.skipped.contains(&"report.crdownload".to_string()));
        assert!(report.skipped.contains(&"temp.tmp".to_string()));
        assert!(report.skipped.contains(&"nested_folder".to_string()));
        #[cfg(unix)]
        assert!(report.skipped.contains(&"link.txt".to_string()));
    }

    #[test]
    fn extraction_index_failure_preserves_the_only_original_and_allows_retry() {
        let (_tmp, vault, mut writer) = setup();
        let conn = index::open_index(&vault).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER test_fail_index BEFORE INSERT ON attachment_files
            BEGIN SELECT RAISE(FAIL, 'injected attachment index failure'); END",
        )
        .unwrap();
        let bytes = b"This original must remain recoverable.";
        std::fs::write(vault::inbox_dir(&vault).join("memo.txt"), bytes).unwrap();
        let report = sweep_with_settle(&mut writer, Duration::ZERO).unwrap();
        assert!(report.failed.is_empty());
        let item = &report.created[0];
        assert!(!item.text_extracted);
        assert!(writer
            .take_warnings()
            .iter()
            .any(|w| w.contains("injected attachment index failure")));
        assert_eq!(
            std::fs::read(vault.join("attachments").join(&item.id).join("memo.txt")).unwrap(),
            bytes
        );
        assert_eq!(
            fragment::read_fragment(&vault, &item.id)
                .unwrap()
                .attachments,
            ["memo.txt"]
        );
        conn.execute_batch("DROP TRIGGER test_fail_index").unwrap();
        let conn = index::open_index(&vault).unwrap();
        crate::extract::fill(&vault, &conn).unwrap();
        assert_eq!(
            crate::extract::list_status(&conn, &vault, &item.id).unwrap()[0].status,
            "ready"
        );
    }

    #[test]
    fn deferred_batch_returns_before_extraction_and_never_renames_an_edited_source() {
        let (_tmp, vault, mut writer) = setup();
        for n in 0..17 {
            std::fs::write(
                vault::inbox_dir(&vault).join(format!("{n}.md")),
                "# Document title\n\nBody",
            )
            .unwrap();
        }
        let first = sweep_impl(&mut writer, Duration::ZERO, true).unwrap();
        assert_eq!(first.created.len(), 16);
        assert!(first.more);
        let id = &first.created[0].id;
        assert!(!crate::attachment::sidecar_dir(&vault, id).unwrap().exists());
        let mut edited = fragment::read_fragment(&vault, id).unwrap();
        edited.title = "My own title".into();
        writer.update(edited).unwrap();

        let rest = sweep_impl(&mut writer, Duration::ZERO, true).unwrap();
        assert_eq!(rest.created.len(), 1);
        assert_eq!(rest.pending, 17);
        let conn = index::open_index(&vault).unwrap();
        crate::extract::fill(&vault, &conn).unwrap();
        let done = sweep_impl(&mut writer, Duration::ZERO, true).unwrap();
        assert_eq!((done.completed, done.pending), (17, 0));
        assert_eq!(
            fragment::read_fragment(&vault, id).unwrap().title,
            "My own title"
        );
        assert_eq!(
            fragment::read_fragment(&vault, &rest.created[0].id)
                .unwrap()
                .title,
            "Document title"
        );
    }

    #[test]
    fn sweep_waits_until_settled_then_creates_source_and_populates_title() {
        let (_tmp, vault, mut writer) = setup();
        let inbox = vault::inbox_dir(&vault);

        // Freshly written file with long settle window -> waiting
        let html_fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attachments/sample.html");
        let dest = inbox.join("dropped_guide.html");
        std::fs::copy(&html_fixture, &dest).unwrap();

        let waiting = sweep_with_settle(&mut writer, Duration::from_secs(60)).unwrap();
        assert!(waiting.created.is_empty());
        assert_eq!(waiting.waiting, vec!["dropped_guide.html"]);
        assert!(dest.exists(), "unsettled file stays in inbox");

        // Once settled -> swept into a source fragment with title from HTML H1
        let swept = sweep_with_settle(&mut writer, Duration::ZERO).unwrap();
        assert_eq!(swept.created.len(), 1);
        let item = &swept.created[0];
        assert_eq!(item.filename, "dropped_guide.html");
        assert_eq!(item.title, "Harbor Operations Handbook");
        assert!(item.text_extracted);
        assert!(!dest.exists(), "file was moved out of inbox");

        let frag = fragment::read_fragment(&vault, &item.id).unwrap();
        assert_eq!(frag.fragment_type, "source");
        assert_eq!(
            frag.extra_fields.get("status").and_then(|v| v.as_str()),
            Some("inbox")
        );
        assert_eq!(frag.attachments, vec!["dropped_guide.html"]);

        // Searchable via in:files right away
        let conn = index::open_index(&vault).unwrap();
        let q = crate::search::parse_query("in:files Harbormaster").unwrap();
        let hits = crate::search::search(&conn, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, item.id);
    }

    #[test]
    fn older_vault_gets_inbox_source_schema_and_gitignore_on_first_sweep() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        let vault = tmp.path().join(".parc");
        vault::init_vault(&vault).unwrap();

        // Simulate an older vault created before Inbox existed
        std::fs::remove_dir_all(vault.join("inbox")).unwrap();
        std::fs::remove_file(vault.join("schemas/source.yml")).unwrap();
        std::fs::remove_file(vault.join("templates/source.md")).unwrap();
        std::fs::write(tmp.path().join(".gitignore"), ".parc/index.db\n").unwrap();

        let mut writer = Writer::new(&vault, ScriptRunner::new(HookStderr::Capture));
        let report = sweep_with_settle(&mut writer, Duration::ZERO).unwrap();
        assert!(report.created.is_empty());
        assert!(vault.join("inbox").is_dir());
        assert!(vault.join("schemas/source.yml").is_file());
        assert!(vault.join("templates/source.md").is_file());
        let gi = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".parc/inbox/"), "{gi}");
    }

    #[test]
    fn discard_restore_and_purge_cleans_attachment_sidecar_and_history() {
        let (_tmp, vault, mut writer) = setup();
        let inbox = vault::inbox_dir(&vault);
        std::fs::write(
            inbox.join("memo.txt"),
            b"First paragraph.\n\nSecond paragraph.",
        )
        .unwrap();

        let swept = sweep_with_settle(&mut writer, Duration::ZERO).unwrap();
        let id = swept.created[0].id.clone();
        let att_dir = vault.join("attachments").join(&id);
        assert!(att_dir.join("memo.txt").exists());
        assert!(att_dir.join(".text/memo.txt.txt").exists());

        // Discard moves fragment to trash; attachment stays so Restore works
        writer.delete(&id).unwrap();
        assert!(vault.join("trash").join(format!("{id}.md")).exists());
        assert!(att_dir.join("memo.txt").exists());

        // Restore brings it back intact
        let restored = writer.restore_from_trash(&id).unwrap();
        assert_eq!(restored.id, id);
        assert!(att_dir.join("memo.txt").exists());

        // Delete again and purge permanently -> file, sidecar, history all gone
        writer.delete(&id).unwrap();
        writer.purge(&id).unwrap();
        assert!(!att_dir.exists());
        assert!(!vault.join("history").join(&id).exists());
        assert!(!vault.join("trash").join(format!("{id}.md")).exists());
    }
}
