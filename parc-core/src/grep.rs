//! Find text inside fragment bodies line by line, like grep: what `/` searches
//! in the desktop's Go to…. Literal text with smart case (case-insensitive
//! unless the pattern has an uppercase letter), read from the index, which
//! holds every fragment's body.

use rusqlite::Connection;
use serde::Serialize;

use crate::error::ParcError;

/// The longest pattern [`grep`] accepts, in characters.
pub const MAX_PATTERN: usize = 1000;
/// Stop collecting once the matched lines hold this much text, however few
/// they are: a body can be one very long line.
const MAX_TEXT_BYTES: usize = 1 << 20;

/// One body line that matches.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GrepLine {
    /// 1-based line of the body as it is edited: the blank line that
    /// separates the body from the frontmatter is not counted.
    pub line: usize,
    /// 1-based column of the line's first match, in characters.
    pub column: usize,
    /// The whole line.
    pub text: String,
    /// Every match in the line, as `[start, end)` character offsets into `text`.
    pub ranges: Vec<(usize, usize)>,
}

/// A fragment with the lines of its body that match.
#[derive(Debug, Clone, Serialize)]
pub struct GrepHit {
    pub id: String,
    #[serde(rename = "type")]
    pub fragment_type: String,
    pub title: String,
    pub status: Option<String>,
    pub archived: bool,
    pub updated_at: String,
    pub lines: Vec<GrepLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GrepResult {
    /// Fragments most recently edited first; lines in body order.
    pub matches: Vec<GrepHit>,
    /// True when more lines matched than `limit` (or the text cap) allowed.
    pub truncated: bool,
}

/// Search every indexed fragment's body for `pattern`, returning up to
/// `limit` matching lines. Archived fragments are included and marked.
pub fn grep(conn: &Connection, pattern: &str, limit: usize) -> Result<GrepResult, ParcError> {
    let mut result = GrepResult {
        matches: Vec::new(),
        truncated: false,
    };
    if pattern.is_empty() || limit == 0 {
        return Ok(result);
    }
    if pattern.chars().count() > MAX_PATTERN {
        return Err(ParcError::ValidationError(format!(
            "the search text is longer than {} characters",
            MAX_PATTERN
        )));
    }
    // Smart case: an uppercase letter in the pattern makes it case-sensitive.
    let ignore_case = !pattern.chars().any(char::is_uppercase);
    let re = regex::RegexBuilder::new(&regex::escape(pattern))
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| ParcError::ValidationError(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, type, title, status, archived, updated_at, body FROM fragments \
         ORDER BY updated_at DESC, id",
    )?;
    let mut rows = stmt.query([])?;
    let (mut lines_left, mut bytes) = (limit, 0usize);
    'fragments: while let Some(row) = rows.next()? {
        let body: String = row.get(6)?;
        let body = body.strip_prefix('\n').unwrap_or(&body);
        if !re.is_match(body) {
            continue;
        }
        let mut lines = Vec::new();
        for (i, raw) in body.split('\n').enumerate() {
            let text = raw.strip_suffix('\r').unwrap_or(raw);
            let ranges: Vec<(usize, usize)> = re
                .find_iter(text)
                .map(|m| {
                    let start = text[..m.start()].chars().count();
                    (start, start + m.as_str().chars().count())
                })
                .collect();
            let Some(&(first, _)) = ranges.first() else {
                continue;
            };
            if lines_left == 0 || bytes + text.len() > MAX_TEXT_BYTES {
                result.truncated = true;
                if !lines.is_empty() {
                    result.matches.push(hit(row, lines)?);
                }
                break 'fragments;
            }
            lines_left -= 1;
            bytes += text.len();
            lines.push(GrepLine {
                line: i + 1,
                column: first + 1,
                text: text.to_string(),
                ranges,
            });
        }
        if !lines.is_empty() {
            result.matches.push(hit(row, lines)?);
        }
    }
    Ok(result)
}

fn hit(row: &rusqlite::Row, lines: Vec<GrepLine>) -> Result<GrepHit, ParcError> {
    Ok(GrepHit {
        id: row.get(0)?,
        fragment_type: row.get(1)?,
        title: row.get(2)?,
        status: row.get(3)?,
        archived: row.get::<_, i64>(4)? != 0,
        updated_at: row.get(5)?,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::{self, Fragment};
    use crate::index;
    use chrono::{Duration, Utc};
    use std::collections::BTreeMap;

    fn vault() -> (tempfile::TempDir, Connection) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = index::init_index(&vault).unwrap();
        (tmp, conn)
    }

    fn add(conn: &Connection, title: &str, body: &str, age_minutes: i64, archived: bool) -> String {
        let mut extra = BTreeMap::new();
        if archived {
            extra.insert("archived".to_string(), serde_json::Value::Bool(true));
        }
        let when = Utc::now() - Duration::minutes(age_minutes);
        let f = Fragment {
            id: fragment::new_id(),
            fragment_type: "note".to_string(),
            title: title.to_string(),
            tags: Vec::new(),
            links: Vec::new(),
            attachments: Vec::new(),
            created_at: when,
            updated_at: when,
            created_by: None,
            extra_fields: extra,
            body: body.to_string(),
        };
        index::index_fragment(conn, &f, &[], &[]).unwrap();
        f.id
    }

    #[test]
    fn lines_and_columns_count_the_body_as_edited() {
        let (_tmp, conn) = vault();
        // The blank line after the frontmatter isn't a line of the body.
        let id = add(
            &conn,
            "Ferry",
            "\n## Questions\nWho runs the ferry?\r\nNone\nferry, Ferry, FERRY",
            0,
            false,
        );
        let r = grep(&conn, "ferry", 10).unwrap();
        assert!(!r.truncated);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].id, id);
        let lines = &r.matches[0].lines;
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0].line, lines[0].column), (2, 14));
        assert_eq!(lines[0].text, "Who runs the ferry?");
        assert_eq!((lines[1].line, lines[1].column), (4, 1));
        assert_eq!(lines[1].ranges, vec![(0, 5), (7, 12), (14, 19)]);
    }

    #[test]
    fn an_uppercase_letter_makes_it_case_sensitive() {
        let (_tmp, conn) = vault();
        add(&conn, "A", "ferry, Ferry, FERRY", 0, false);
        let r = grep(&conn, "Ferry", 10).unwrap();
        assert_eq!(r.matches[0].lines[0].ranges, vec![(7, 12)]);
    }

    #[test]
    fn the_pattern_is_literal_text_and_columns_are_characters() {
        let (_tmp, conn) = vault();
        add(&conn, "A", "Käytä C++ (ja .* myös) täällä", 0, false);
        let r = grep(&conn, ".*", 10).unwrap();
        assert_eq!(r.matches[0].lines[0].ranges, vec![(14, 16)]);
        let r = grep(&conn, "täällä", 10).unwrap();
        assert_eq!(r.matches[0].lines[0].column, 24);
        assert!(grep(&conn, "C+", 10).unwrap().matches.len() == 1);
        assert!(grep(&conn, "x{2}", 10).unwrap().matches.is_empty());
    }

    #[test]
    fn newest_first_with_archived_marked_and_a_limit() {
        let (_tmp, conn) = vault();
        let old = add(&conn, "Old", "dock\ndock", 60, true);
        let new = add(&conn, "New", "the dock", 1, false);
        let r = grep(&conn, "dock", 10).unwrap();
        let ids: Vec<_> = r.matches.iter().map(|h| h.id.clone()).collect();
        assert_eq!(ids, vec![new.clone(), old.clone()]);
        assert!(r.matches[1].archived && !r.matches[0].archived);

        let r = grep(&conn, "dock", 2).unwrap();
        assert!(r.truncated);
        assert_eq!(r.matches.len(), 2);
        assert_eq!(r.matches[1].lines.len(), 1);
        assert!(grep(&conn, "", 10).unwrap().matches.is_empty());
        assert!(grep(&conn, &"x".repeat(MAX_PATTERN + 1), 10).is_err());
    }
}
