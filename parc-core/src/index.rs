use std::path::Path;

use rusqlite::Connection;

use crate::error::ParcError;
use crate::fragment::{self, Fragment};
use crate::tag;

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS fragments (
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
    extra_json  TEXT
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

CREATE TABLE IF NOT EXISTS fragment_links (
    source_id TEXT NOT NULL REFERENCES fragments(id),
    target_id TEXT NOT NULL,
    PRIMARY KEY (source_id, target_id)
);

CREATE INDEX IF NOT EXISTS idx_fragments_type ON fragments(type);
CREATE INDEX IF NOT EXISTS idx_fragments_status ON fragments(status);
CREATE INDEX IF NOT EXISTS idx_fragments_due ON fragments(due);
CREATE INDEX IF NOT EXISTS idx_fragment_tags_tag ON fragment_tags(tag);
";

/// Initialize the database schema (create tables if not exist).
pub fn init_index(vault: &Path) -> Result<Connection, ParcError> {
    let db_path = vault.join("index.db");
    let conn = Connection::open(&db_path)?;
    conn.execute_batch(SCHEMA_SQL)?;
    Ok(conn)
}

/// Open an existing index (also ensures schema exists).
pub fn open_index(vault: &Path) -> Result<Connection, ParcError> {
    init_index(vault)
}

/// Index a single fragment (upsert into all tables).
pub fn index_fragment(
    conn: &Connection,
    fragment: &Fragment,
    merged_tags: &[String],
) -> Result<(), ParcError> {
    let extra_json = serde_json::to_string(&fragment.extra_fields)?;

    // Upsert into fragments table
    conn.execute(
        "INSERT OR REPLACE INTO fragments (id, type, title, status, priority, due, assignee, created_by, created_at, updated_at, body, extra_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
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
        ],
    )?;

    // Update tags
    conn.execute(
        "DELETE FROM fragment_tags WHERE fragment_id = ?1",
        [&fragment.id],
    )?;
    for t in merged_tags {
        conn.execute(
            "INSERT OR IGNORE INTO fragment_tags (fragment_id, tag) VALUES (?1, ?2)",
            rusqlite::params![fragment.id, t],
        )?;
    }

    // Update FTS
    conn.execute(
        "DELETE FROM fragments_fts WHERE id = ?1",
        [&fragment.id],
    )?;
    let tags_str = merged_tags.join(" ");
    conn.execute(
        "INSERT INTO fragments_fts (id, title, body, tags) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![fragment.id, fragment.title, fragment.body, tags_str],
    )?;

    // Update links
    conn.execute(
        "DELETE FROM fragment_links WHERE source_id = ?1",
        [&fragment.id],
    )?;
    for link in &fragment.links {
        conn.execute(
            "INSERT OR IGNORE INTO fragment_links (source_id, target_id) VALUES (?1, ?2)",
            rusqlite::params![fragment.id, link],
        )?;
    }

    Ok(())
}

/// Remove a fragment from the index.
pub fn remove_from_index(conn: &Connection, id: &str) -> Result<(), ParcError> {
    conn.execute("DELETE FROM fragment_tags WHERE fragment_id = ?1", [id])?;
    conn.execute("DELETE FROM fragments_fts WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM fragment_links WHERE source_id = ?1", [id])?;
    conn.execute("DELETE FROM fragments WHERE id = ?1", [id])?;
    Ok(())
}

/// Rebuild the entire index from fragment files. Returns count.
pub fn reindex(vault: &Path) -> Result<usize, ParcError> {
    let conn = init_index(vault)?;

    // Clear all tables
    conn.execute_batch(
        "DELETE FROM fragment_tags;
         DELETE FROM fragments_fts;
         DELETE FROM fragment_links;
         DELETE FROM fragments;",
    )?;

    let ids = fragment::list_fragment_ids(vault)?;
    let mut count = 0;
    let mut warnings = Vec::new();

    for id in &ids {
        let path = vault.join("fragments").join(format!("{}.md", id));
        match std::fs::read_to_string(&path) {
            Ok(content) => match fragment::parse_fragment(&content) {
                Ok(frag) => {
                    let inline_tags = tag::extract_inline_tags(&frag.body);
                    let merged = tag::merge_tags(&frag.tags, &inline_tags);
                    if let Err(e) = index_fragment(&conn, &frag, &merged) {
                        warnings.push(format!("warning: failed to index {}: {}", id, e));
                    } else {
                        count += 1;
                    }
                }
                Err(e) => {
                    warnings.push(format!("warning: failed to parse {}: {}", id, e));
                }
            },
            Err(e) => {
                warnings.push(format!("warning: failed to read {}: {}", id, e));
            }
        }
    }

    // We don't print warnings here (core has no I/O), caller can handle them
    Ok(count)
}

/// Index a fragment with automatic tag merging.
pub fn index_fragment_auto(conn: &Connection, fragment: &Fragment) -> Result<(), ParcError> {
    let inline_tags = tag::extract_inline_tags(&fragment.body);
    let merged = tag::merge_tags(&fragment.tags, &inline_tags);
    index_fragment(conn, fragment, &merged)
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
        index_fragment(&conn, &frag, &["test".to_string()]).unwrap();

        // Verify fragment is in the index
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM fragments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Verify FTS works
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
        index_fragment(&conn, &frag, &["tag1".to_string()]).unwrap();
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

        // Create a fragment file
        let frag = make_fragment("Reindex test", "Body with #hashtag");
        fragment::create_fragment(&vault, &frag).unwrap();

        let count = reindex(&vault).unwrap();
        assert_eq!(count, 1);
    }
}
