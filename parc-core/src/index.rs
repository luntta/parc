use std::path::Path;

use rusqlite::Connection;

use crate::error::ParcError;
use crate::fragment::{self, Fragment};
use crate::link;
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

CREATE TABLE IF NOT EXISTS fragment_links (
    source_id TEXT NOT NULL REFERENCES fragments(id),
    target_id TEXT NOT NULL,
    PRIMARY KEY (source_id, target_id)
);

CREATE INDEX IF NOT EXISTS idx_fragments_type ON fragments(type);
CREATE INDEX IF NOT EXISTS idx_fragments_status ON fragments(status);
CREATE INDEX IF NOT EXISTS idx_fragments_due ON fragments(due);
CREATE INDEX IF NOT EXISTS idx_fragment_tags_tag ON fragment_tags(tag);
CREATE INDEX IF NOT EXISTS idx_fragment_links_target ON fragment_links(target_id);
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
/// `merged_links` should include both frontmatter links and resolved body wiki-links.
pub fn index_fragment(
    conn: &Connection,
    fragment: &Fragment,
    merged_tags: &[String],
    merged_links: &[String],
) -> Result<(), ParcError> {
    let extra_json = serde_json::to_string(&fragment.extra_fields)?;

    let archived = fragment
        .extra_fields
        .get("archived")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Upsert into fragments table
    conn.execute(
        "INSERT OR REPLACE INTO fragments (id, type, title, status, priority, due, assignee, created_by, created_at, updated_at, body, extra_json, attachment_count, archived)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
            fragment.attachments.len() as i64,
            archived as i32,
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

    // Update links (using merged frontmatter + body wiki-links)
    conn.execute(
        "DELETE FROM fragment_links WHERE source_id = ?1",
        [&fragment.id],
    )?;
    for link_id in merged_links {
        conn.execute(
            "INSERT OR IGNORE INTO fragment_links (source_id, target_id) VALUES (?1, ?2)",
            rusqlite::params![fragment.id, link_id],
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

    let all_ids = fragment::list_fragment_ids(vault)?;
    let link_candidates = load_fragment_refs(vault)?;
    let mut count = 0;
    let mut warnings = Vec::new();

    for id in &all_ids {
        let path = vault.join("fragments").join(format!("{}.md", id));
        match std::fs::read_to_string(&path) {
            Ok(content) => match fragment::parse_fragment(&content) {
                Ok(frag) => {
                    let inline_tags = tag::extract_inline_tags(&frag.body);
                    let merged_tags = tag::merge_tags(&frag.tags, &inline_tags);

                    let body_links = link::parse_wiki_links(&frag.body);
                    let merged_links = link::merge_links(&frag.links, &body_links, |target| {
                        match link::resolve_link_target(target, &link_candidates) {
                            link::ResolveOutcome::Unique(id) => Some(id),
                            link::ResolveOutcome::Ambiguous(_) | link::ResolveOutcome::None => None,
                        }
                    });

                    if let Err(e) = index_fragment(&conn, &frag, &merged_tags, &merged_links) {
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

    Ok(count)
}

/// Index a fragment with automatic tag and link merging.
/// Uses the vault's fragment list for wiki-link prefix resolution.
pub fn index_fragment_auto(
    conn: &Connection,
    fragment: &Fragment,
    vault: &Path,
) -> Result<(), ParcError> {
    let inline_tags = tag::extract_inline_tags(&fragment.body);
    let merged_tags = tag::merge_tags(&fragment.tags, &inline_tags);

    let link_candidates = load_fragment_refs(vault)?;
    let body_links = link::parse_wiki_links(&fragment.body);
    let merged_links = link::merge_links(&fragment.links, &body_links, |target| {
        match link::resolve_link_target(target, &link_candidates) {
            link::ResolveOutcome::Unique(id) => Some(id),
            link::ResolveOutcome::Ambiguous(_) | link::ResolveOutcome::None => None,
        }
    });

    index_fragment(conn, fragment, &merged_tags, &merged_links)
}

fn load_fragment_refs(vault: &Path) -> Result<Vec<link::FragmentRef>, ParcError> {
    let mut refs = Vec::new();
    for id in fragment::list_fragment_ids(vault)? {
        let path = vault.join("fragments").join(format!("{}.md", id));
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(frag) = fragment::parse_fragment(&content) {
                refs.push(link::FragmentRef {
                    id: frag.id,
                    title: frag.title,
                });
            }
        }
    }
    Ok(refs)
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
         FROM fragment_links fl
         JOIN fragments f ON f.id = fl.source_id
         WHERE fl.target_id = ?1
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

        let count = reindex(&vault).unwrap();
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

        let count = reindex(&vault).unwrap();
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

        let count = reindex(&vault).unwrap();
        assert_eq!(count, 2);

        let conn = open_index(&vault).unwrap();
        let backlinks = get_backlinks(&conn, &frag_a.id).unwrap();
        assert_eq!(backlinks.len(), 1);
        assert_eq!(backlinks[0].source_id, frag_b.id);
    }
}
