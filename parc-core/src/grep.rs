//! Find text inside fragment bodies and their attachments line by line, like
//! grep: what `/` searches in the desktop's Go to…. Literal text with smart
//! case (case-insensitive unless the pattern has an uppercase letter), read
//! from the index, which holds every fragment's body and the extracted text of
//! its attachments (PDF pages, slides, sections: the sidecars' text).

use std::collections::HashMap;

use regex::Regex;
use rusqlite::Connection;
use serde::Serialize;

use crate::error::ParcError;
use crate::extract::PartUnit;

/// The longest pattern [`grep`] accepts, in characters.
pub const MAX_PATTERN: usize = 1000;
/// Stop collecting once the matched lines hold this much text, however few
/// they are: a body can be one very long line.
const MAX_TEXT_BYTES: usize = 1 << 20;

/// One line that matches: of the body, or of an attachment's extracted text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GrepLine {
    /// 1-based line of the body as it is edited: the blank line that
    /// separates the body from the frontmatter is not counted. For an
    /// attachment line, the line within its part (counted the same way).
    pub line: usize,
    /// 1-based column of the line's first match, in characters.
    pub column: usize,
    /// The whole line.
    pub text: String,
    /// Every match in the line, as `[start, end)` character offsets into `text`.
    pub ranges: Vec<(usize, usize)>,
    /// The attachment's filename when the line is from its extracted text
    /// rather than the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<String>,
    /// 1-based page, slide or section of `attachment` that holds the line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<usize>,
    /// Where in `attachment`, for people: `p. 12`, `slide 3`, `§ Heading`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// A fragment with the lines that match: its body's, then its attachments'
/// (by filename, then part).
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
    /// Fragments most recently edited first; each one's body lines in order,
    /// then its attachment lines.
    pub matches: Vec<GrepHit>,
    /// True when more lines matched than `limit` (or the text cap) allowed.
    pub truncated: bool,
}

/// Search every indexed fragment's body, and its attachments' extracted text,
/// for `pattern`, returning up to `limit` matching lines. Archived fragments
/// are included and marked.
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

    let mut parts = matching_parts(conn, &re)?;
    let mut part_text = conn.prepare("SELECT text FROM attachment_fts WHERE rowid = ?1")?;
    let mut stmt = conn.prepare(
        "SELECT id, type, title, status, archived, updated_at, body FROM fragments \
         ORDER BY updated_at DESC, id",
    )?;
    let mut rows = stmt.query([])?;
    let mut budget = Budget {
        lines_left: limit,
        bytes: 0,
    };
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let body: String = row.get(6)?;
        let in_body = re.is_match(&body);
        let in_parts = parts.remove(&id);
        if !in_body && in_parts.is_none() {
            continue;
        }
        let mut lines = Vec::new();
        let mut room = !in_body || push_lines(&body, &re, None, &mut budget, &mut lines);
        for p in in_parts.unwrap_or_default() {
            if !room {
                break;
            }
            let text: String = part_text.query_row([p.rowid], |r| r.get(0))?;
            let at = Place {
                label: p.unit.format_ref(p.part, Some(&text)),
                filename: p.filename,
                part: p.part,
            };
            room = push_lines(&text, &re, Some(&at), &mut budget, &mut lines);
        }
        if !lines.is_empty() {
            result.matches.push(hit(row, lines)?);
        }
        if !room {
            result.truncated = true;
            break;
        }
    }
    Ok(result)
}

/// An attachment part whose text matches, found before its text is read again
/// line by line.
struct PartRef {
    rowid: i64,
    filename: String,
    part: usize,
    unit: PartUnit,
}

/// Where an attachment line is, as [`GrepLine`] reports it.
struct Place {
    filename: String,
    part: usize,
    label: String,
}

struct Budget {
    lines_left: usize,
    bytes: usize,
}

/// The attachment parts that contain `re`, by fragment ID, in filename and
/// part order. Only row IDs are kept: the matching lines are read when their
/// fragment's turn comes, so the limit bounds what is held.
fn matching_parts(
    conn: &Connection,
    re: &Regex,
) -> Result<HashMap<String, Vec<PartRef>>, ParcError> {
    let mut stmt = conn.prepare(
        "SELECT a.rowid, a.fragment_id, a.filename, CAST(a.part AS INTEGER), af.unit, a.text
         FROM attachment_fts a
         LEFT JOIN attachment_files af ON af.fragment_id = a.fragment_id AND af.filename = a.filename",
    )?;
    let mut rows = stmt.query([])?;
    let mut found: HashMap<String, Vec<PartRef>> = HashMap::new();
    while let Some(row) = rows.next()? {
        let text: String = row.get(5)?;
        if !re.is_match(&text) {
            continue;
        }
        let unit: Option<String> = row.get(4)?;
        found.entry(row.get(1)?).or_default().push(PartRef {
            rowid: row.get(0)?,
            filename: row.get(2)?,
            part: row.get::<_, i64>(3)?.max(1) as usize,
            unit: unit
                .as_deref()
                .and_then(PartUnit::parse)
                .unwrap_or(PartUnit::Page),
        });
    }
    for refs in found.values_mut() {
        refs.sort_by(|a, b| a.filename.cmp(&b.filename).then(a.part.cmp(&b.part)));
    }
    Ok(found)
}

/// Add `text`'s matching lines to `lines`, as lines of the body or, with
/// `at`, of that attachment part. False when `budget` ran out first: the
/// result is truncated.
fn push_lines(
    text: &str,
    re: &Regex,
    at: Option<&Place>,
    budget: &mut Budget,
    lines: &mut Vec<GrepLine>,
) -> bool {
    let text = text.strip_prefix('\n').unwrap_or(text);
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let ranges: Vec<(usize, usize)> = re
            .find_iter(line)
            .map(|m| {
                let start = line[..m.start()].chars().count();
                (start, start + m.as_str().chars().count())
            })
            .collect();
        let Some(&(first, _)) = ranges.first() else {
            continue;
        };
        if budget.lines_left == 0 || budget.bytes + line.len() > MAX_TEXT_BYTES {
            return false;
        }
        budget.lines_left -= 1;
        budget.bytes += line.len();
        lines.push(GrepLine {
            line: i + 1,
            column: first + 1,
            text: line.to_string(),
            ranges,
            attachment: at.map(|p| p.filename.clone()),
            part: at.map(|p| p.part),
            label: at.map(|p| p.label.clone()),
        });
    }
    true
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

    /// Index an attachment's extracted parts as `extract::store` does.
    fn attach(conn: &Connection, id: &str, filename: &str, unit: &str, parts: &[&str]) {
        conn.execute(
            "INSERT INTO attachment_files (fragment_id, filename, sha256, size, mtime_ns, unit, format, parts, status)
             VALUES (?1, ?2, '', 0, 0, ?3, 'text', ?4, 'ready')",
            rusqlite::params![id, filename, unit, parts.len() as i64],
        )
        .unwrap();
        for (i, text) in parts.iter().enumerate() {
            conn.execute(
                "INSERT INTO attachment_fts (fragment_id, filename, part, text) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, filename, (i + 1) as i64, text],
            )
            .unwrap();
        }
    }

    #[test]
    fn attachment_lines_follow_the_body_with_their_page() {
        let (_tmp, conn) = vault();
        let id = add(&conn, "Harbour", "the ferry timetable", 0, false);
        attach(
            &conn,
            &id,
            "b.pdf",
            "page",
            &["Nothing here", "Line one\nThe Ferry leaves at 9"],
        );
        attach(
            &conn,
            &id,
            "a.docx",
            "section",
            &["## Routes\nferry routes"],
        );
        let r = grep(&conn, "ferry", 10).unwrap();
        assert_eq!(r.matches.len(), 1);
        let lines = &r.matches[0].lines;
        let places: Vec<_> = lines
            .iter()
            .map(|l| {
                (
                    l.attachment.as_deref(),
                    l.part,
                    l.label.as_deref(),
                    l.line,
                    l.column,
                )
            })
            .collect();
        assert_eq!(
            places,
            vec![
                (None, None, None, 1, 5),
                (Some("a.docx"), Some(1), Some("§ Routes"), 2, 1),
                (Some("b.pdf"), Some(2), Some("p. 2"), 2, 5),
            ]
        );
        assert_eq!(lines[2].text, "The Ferry leaves at 9");
        assert_eq!(lines[2].ranges, vec![(4, 9)]);
    }

    #[test]
    fn a_fragment_found_only_by_its_attachment_and_the_limit_counts_both() {
        let (_tmp, conn) = vault();
        let doc = add(&conn, "Report", "my notes", 0, false);
        attach(&conn, &doc, "report.pdf", "page", &["dock\ndock", "dock"]);
        let note = add(&conn, "Older", "dock", 5, false);
        let r = grep(&conn, "dock", 10).unwrap();
        let ids: Vec<_> = r.matches.iter().map(|h| h.id.clone()).collect();
        assert_eq!(ids, vec![doc.clone(), note]);
        assert_eq!(r.matches[0].lines.len(), 3);
        assert!(r.matches[0]
            .lines
            .iter()
            .all(|l| l.attachment.as_deref() == Some("report.pdf")));

        let r = grep(&conn, "dock", 2).unwrap();
        assert!(r.truncated);
        assert_eq!(r.matches.len(), 1);
        assert_eq!(r.matches[0].lines.len(), 2);
        // An uppercase letter is case-sensitive in attachments too.
        assert!(grep(&conn, "Dock", 10).unwrap().matches.is_empty());
    }

    #[test]
    fn body_lines_say_nothing_about_attachments() {
        let (_tmp, conn) = vault();
        add(&conn, "A", "plain", 0, false);
        let json = serde_json::to_value(grep(&conn, "plain", 10).unwrap()).unwrap();
        let line = &json["matches"][0]["lines"][0];
        assert!(
            line.get("attachment").is_none()
                && line.get("part").is_none()
                && line.get("label").is_none()
        );
    }
}
