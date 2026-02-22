use rusqlite::Connection;

use crate::error::ParcError;

#[derive(Debug, Default)]
pub struct SearchParams {
    pub query: Option<String>,
    pub type_filter: Option<String>,
    pub status_filter: Option<String>,
    pub tag_filter: Vec<String>,
    pub sort: SortOrder,
    pub limit: Option<usize>,
}

#[derive(Debug, Default)]
pub enum SortOrder {
    #[default]
    UpdatedDesc,
    UpdatedAsc,
    CreatedDesc,
    CreatedAsc,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub id: String,
    pub fragment_type: String,
    pub title: String,
    pub status: Option<String>,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub snippet: Option<String>,
}

/// Execute a search query against the index.
pub fn search(conn: &Connection, params: &SearchParams) -> Result<Vec<SearchResult>, ParcError> {
    let mut conditions = Vec::new();
    let mut sql_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    let use_fts = params.query.is_some();

    // Base query
    let mut sql = if use_fts {
        let query = params.query.as_ref().unwrap();
        conditions.push(format!("fragments_fts MATCH ?{}", param_idx));
        sql_params.push(Box::new(query.clone()));
        param_idx += 1;
        "SELECT f.id, f.type, f.title, f.status, f.updated_at, snippet(fragments_fts, 2, '»', '«', '…', 20) as snippet \
         FROM fragments f \
         JOIN fragments_fts ON fragments_fts.id = f.id".to_string()
    } else {
        "SELECT f.id, f.type, f.title, f.status, f.updated_at, NULL as snippet \
         FROM fragments f".to_string()
    };

    // Type filter
    if let Some(ref type_filter) = params.type_filter {
        conditions.push(format!("f.type = ?{}", param_idx));
        sql_params.push(Box::new(type_filter.clone()));
        param_idx += 1;
    }

    // Status filter
    if let Some(ref status_filter) = params.status_filter {
        conditions.push(format!("f.status = ?{}", param_idx));
        sql_params.push(Box::new(status_filter.clone()));
        param_idx += 1;
    }

    // Tag filter (AND semantics)
    if !params.tag_filter.is_empty() {
        let tag_count = params.tag_filter.len();
        let tag_placeholders: Vec<String> = params
            .tag_filter
            .iter()
            .map(|t| {
                let ph = format!("?{}", param_idx);
                sql_params.push(Box::new(t.clone()));
                param_idx += 1;
                ph
            })
            .collect();
        sql += &format!(
            " JOIN fragment_tags ft ON ft.fragment_id = f.id AND ft.tag IN ({})",
            tag_placeholders.join(", ")
        );
        // GROUP BY + HAVING for AND semantics
        conditions.push(format!(
            "1=1 GROUP BY f.id HAVING COUNT(DISTINCT ft.tag) = {}",
            tag_count
        ));
    }

    // Build WHERE clause
    if !conditions.is_empty() {
        sql += " WHERE ";
        sql += &conditions.join(" AND ");
    }

    // If we don't have tag filter (which adds GROUP BY), we may need our own GROUP BY for dedup
    if params.tag_filter.is_empty() && !conditions.is_empty() {
        // no group by needed
    }

    // Sort
    let order = match params.sort {
        SortOrder::UpdatedDesc => "f.updated_at DESC",
        SortOrder::UpdatedAsc => "f.updated_at ASC",
        SortOrder::CreatedDesc => "f.created_at DESC",
        SortOrder::CreatedAsc => "f.created_at ASC",
    };
    sql += &format!(" ORDER BY {}", order);

    if let Some(limit) = params.limit {
        sql += &format!(" LIMIT {}", limit);
    }

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(SearchResult {
            id: row.get(0)?,
            fragment_type: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            tags: Vec::new(), // filled below
            updated_at: row.get(4)?,
            snippet: row.get(5)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        let mut result = row.map_err(ParcError::Sqlite)?;
        // Load tags for this result
        let tags: Vec<String> = {
            let mut tag_stmt = conn.prepare(
                "SELECT tag FROM fragment_tags WHERE fragment_id = ?1 ORDER BY tag",
            )?;
            let collected: Vec<String> = tag_stmt
                .query_map([&result.id], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            collected
        };
        result.tags = tags;
        results.push(result);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::{self, Fragment};
    use crate::index;
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn setup_test_vault() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = index::init_index(&vault).unwrap();
        (tmp, conn)
    }

    fn make_todo(title: &str, status: &str, tags: Vec<String>) -> Fragment {
        let mut extra = BTreeMap::new();
        extra.insert("status".to_string(), serde_json::Value::String(status.to_string()));
        Fragment {
            id: fragment::new_id(),
            fragment_type: "todo".to_string(),
            title: title.to_string(),
            tags: tags.clone(),
            links: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            extra_fields: extra,
            body: format!("Body of {}", title),
        }
    }

    #[test]
    fn test_fts_search() {
        let (_tmp, conn) = setup_test_vault();
        let frag = make_todo("SQLite indexing", "open", vec!["search".to_string()]);
        index::index_fragment(&conn, &frag, &["search".to_string()]).unwrap();

        let results = search(
            &conn,
            &SearchParams {
                query: Some("SQLite".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "SQLite indexing");
    }

    #[test]
    fn test_type_filter() {
        let (_tmp, conn) = setup_test_vault();
        let todo = make_todo("A todo", "open", vec![]);
        index::index_fragment(&conn, &todo, &[]).unwrap();

        let results = search(
            &conn,
            &SearchParams {
                type_filter: Some("todo".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(results.len(), 1);

        let results = search(
            &conn,
            &SearchParams {
                type_filter: Some("note".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_tag_filter_and() {
        let (_tmp, conn) = setup_test_vault();
        let frag = make_todo("Tagged", "open", vec!["a".to_string(), "b".to_string()]);
        index::index_fragment(&conn, &frag, &["a".to_string(), "b".to_string()]).unwrap();

        let frag2 = make_todo("Only a", "open", vec!["a".to_string()]);
        index::index_fragment(&conn, &frag2, &["a".to_string()]).unwrap();

        // Search for both tags (AND)
        let results = search(
            &conn,
            &SearchParams {
                tag_filter: vec!["a".to_string(), "b".to_string()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Tagged");
    }
}
