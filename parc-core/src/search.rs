use chrono::NaiveDate;
use rusqlite::Connection;

use crate::date::{self, RelativeDate};
use crate::error::ParcError;

// ── AST types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextTerm {
    Word(String),
    Phrase(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Lt,
    Gt,
    Lte,
    Gte,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateFilter {
    Relative(RelativeDate),
    RelativeWithOp { op: CompareOp, rel: RelativeDate },
    Absolute { op: CompareOp, date: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HasCondition {
    Attachments,
    Links,
    Due,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    Type {
        value: String,
        negated: bool,
    },
    Status {
        value: String,
        negated: bool,
    },
    Priority {
        op: CompareOp,
        value: String,
        negated: bool,
    },
    Tag {
        value: String,
        negated: bool,
    },
    Due(DateFilter),
    Created(DateFilter),
    Updated(DateFilter),
    CreatedBy {
        value: String,
        negated: bool,
    },
    Has(HasCondition),
    Linked(String),
    Is(IsCondition),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsCondition {
    Archived,
    All,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortOrder {
    #[default]
    UpdatedDesc,
    UpdatedAsc,
    CreatedDesc,
    CreatedAsc,
    Random,
    /// Order by fuzzy match score (highest first). Falls back to UpdatedDesc
    /// for SQL pre-fetch — the actual scoring is applied after nucleo runs.
    Score,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text_terms: Vec<TextTerm>,
    pub filters: Vec<Filter>,
    pub sort: SortOrder,
    pub limit: Option<usize>,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text_terms: Vec::new(),
            filters: Vec::new(),
            sort: SortOrder::default(),
            limit: None,
        }
    }
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

// ── Parser ─────────────────────────────────────────────────────────────

const FILTER_FIELDS: &[&str] = &[
    "type", "status", "priority", "tag", "due", "created", "updated", "by", "has", "linked", "is",
];

/// Parse a DSL query string into a SearchQuery AST.
pub fn parse_query(input: &str) -> Result<SearchQuery, ParcError> {
    let mut text_terms = Vec::new();
    let mut filters = Vec::new();

    let input = input.trim();
    if input.is_empty() {
        return Ok(SearchQuery::default());
    }

    let mut chars = input.chars().peekable();

    while chars.peek().is_some() {
        // Skip whitespace
        while chars.peek().map_or(false, |c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        // Quoted phrase
        if chars.peek() == Some(&'"') {
            chars.next(); // consume opening quote
            let mut phrase = String::new();
            while let Some(&c) = chars.peek() {
                if c == '"' {
                    chars.next(); // consume closing quote
                    break;
                }
                phrase.push(c);
                chars.next();
            }
            if !phrase.is_empty() {
                text_terms.push(TextTerm::Phrase(phrase));
            }
            continue;
        }

        // Hashtag shorthand for tag filter
        if chars.peek() == Some(&'#') {
            chars.next(); // consume #
            let word = consume_word(&mut chars);
            if !word.is_empty() {
                filters.push(Filter::Tag {
                    value: word.to_lowercase(),
                    negated: false,
                });
            }
            continue;
        }

        // Consume a word/token
        let token = consume_word(&mut chars);
        if token.is_empty() {
            continue;
        }

        // Check if it's a filter: field:value
        if let Some(colon_pos) = token.find(':') {
            let field = &token[..colon_pos];
            let value = &token[colon_pos + 1..];

            if FILTER_FIELDS.contains(&field) && !value.is_empty() {
                match parse_filter(field, value) {
                    Ok(f) => {
                        filters.push(f);
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        // Plain text word
        text_terms.push(TextTerm::Word(token));
    }

    Ok(SearchQuery {
        text_terms,
        filters,
        sort: SortOrder::default(),
        limit: None,
    })
}

fn consume_word(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut word = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            break;
        }
        word.push(c);
        chars.next();
    }
    word
}

fn parse_filter(field: &str, value: &str) -> Result<Filter, ParcError> {
    match field {
        "type" => {
            let (negated, val) = parse_negation(value);
            Ok(Filter::Type {
                value: val.to_string(),
                negated,
            })
        }
        "status" => {
            let (negated, val) = parse_negation(value);
            Ok(Filter::Status {
                value: val.to_string(),
                negated,
            })
        }
        "priority" => {
            let (op, negated, val) = parse_compare_value(value);
            Ok(Filter::Priority {
                op,
                value: val.to_string(),
                negated,
            })
        }
        "tag" => {
            let (negated, val) = parse_negation(value);
            Ok(Filter::Tag {
                value: val.to_lowercase(),
                negated,
            })
        }
        "due" => Ok(Filter::Due(parse_date_filter(value)?)),
        "created" => Ok(Filter::Created(parse_date_filter(value)?)),
        "updated" => Ok(Filter::Updated(parse_date_filter(value)?)),
        "by" => {
            let (negated, val) = parse_negation(value);
            Ok(Filter::CreatedBy {
                value: val.to_string(),
                negated,
            })
        }
        "has" => match value {
            "attachments" => Ok(Filter::Has(HasCondition::Attachments)),
            "links" => Ok(Filter::Has(HasCondition::Links)),
            "due" => Ok(Filter::Has(HasCondition::Due)),
            _ => Err(ParcError::ParseError(format!(
                "unknown has: condition '{}'",
                value
            ))),
        },
        "linked" => Ok(Filter::Linked(value.to_string())),
        "is" => match value {
            "archived" => Ok(Filter::Is(IsCondition::Archived)),
            "all" => Ok(Filter::Is(IsCondition::All)),
            _ => Err(ParcError::ParseError(format!(
                "unknown is: condition '{}'",
                value
            ))),
        },
        _ => Err(ParcError::ParseError(format!(
            "unknown filter field '{}'",
            field
        ))),
    }
}

fn parse_negation(value: &str) -> (bool, &str) {
    if let Some(rest) = value.strip_prefix('!') {
        (true, rest)
    } else {
        (false, value)
    }
}

fn parse_compare_value(value: &str) -> (CompareOp, bool, &str) {
    let (negated, value) = parse_negation(value);
    if let Some(rest) = value.strip_prefix(">=") {
        (CompareOp::Gte, negated, rest)
    } else if let Some(rest) = value.strip_prefix("<=") {
        (CompareOp::Lte, negated, rest)
    } else if let Some(rest) = value.strip_prefix('>') {
        (CompareOp::Gt, negated, rest)
    } else if let Some(rest) = value.strip_prefix('<') {
        (CompareOp::Lt, negated, rest)
    } else {
        (CompareOp::Eq, negated, value)
    }
}

fn parse_date_filter(value: &str) -> Result<DateFilter, ParcError> {
    // Check for comparison operators first
    let (op, val) = if let Some(rest) = value.strip_prefix(">=") {
        (CompareOp::Gte, rest)
    } else if let Some(rest) = value.strip_prefix("<=") {
        (CompareOp::Lte, rest)
    } else if let Some(rest) = value.strip_prefix('>') {
        (CompareOp::Gt, rest)
    } else if let Some(rest) = value.strip_prefix('<') {
        (CompareOp::Lt, rest)
    } else {
        // Could be a relative date or an exact date (implicit Eq)
        if let Some(rel) = parse_relative_date(value) {
            return Ok(DateFilter::Relative(rel));
        }
        // Try as absolute date
        validate_date(value)?;
        return Ok(DateFilter::Absolute {
            op: CompareOp::Eq,
            date: value.to_string(),
        });
    };

    // After operator: could be relative or absolute
    if let Some(rel) = parse_relative_date(val) {
        return Ok(DateFilter::RelativeWithOp { op, rel });
    }
    validate_date(val)?;
    Ok(DateFilter::Absolute {
        op,
        date: val.to_string(),
    })
}

fn parse_relative_date(value: &str) -> Option<RelativeDate> {
    date::parse_relative_date(value)
}

fn validate_date(value: &str) -> Result<(), ParcError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| ParcError::ParseError(format!("invalid date '{}'", value)))
}

// ── Date resolution ────────────────────────────────────────────────────

fn resolve_relative_date(rel: &RelativeDate) -> (String, String) {
    date::resolve_relative_date_to_range(rel)
}

// ── SQL Compiler ───────────────────────────────────────────────────────

const PRIORITY_ORDER: &[&str] = &["none", "low", "medium", "high", "critical"];

fn priority_rank(p: &str) -> Option<usize> {
    PRIORITY_ORDER.iter().position(|&x| x == p)
}

fn priorities_for_op(op: CompareOp, value: &str) -> Option<Vec<String>> {
    let rank = priority_rank(value)?;
    let selected: Vec<String> = match op {
        CompareOp::Eq => vec![value.to_string()],
        CompareOp::Gt => PRIORITY_ORDER[rank + 1..]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        CompareOp::Gte => PRIORITY_ORDER[rank..]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        CompareOp::Lt => PRIORITY_ORDER[..rank]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        CompareOp::Lte => PRIORITY_ORDER[..=rank]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    };
    Some(selected)
}

struct CompiledQuery {
    sql: String,
    params: Vec<Box<dyn rusqlite::types::ToSql>>,
}

struct FilterClauses {
    conditions: Vec<String>,
    params: Vec<Box<dyn rusqlite::types::ToSql>>,
    joins: String,
    needs_tag_group_by: bool,
    has_is_filter: bool,
}

#[allow(clippy::too_many_arguments)]
fn apply_filters(
    filters: &[Filter],
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    joins: &mut String,
    param_idx: &mut usize,
    needs_tag_group_by: &mut bool,
    tag_count: &mut usize,
    has_is_filter: &mut bool,
) -> Result<(), ParcError> {
    for filter in filters {
        match filter {
            Filter::Type { value, negated } => {
                let op = if *negated { "!=" } else { "=" };
                conditions.push(format!("f.type {} ?{}", op, *param_idx));
                params.push(Box::new(value.clone()));
                *param_idx += 1;
            }
            Filter::Status { value, negated } => {
                let op = if *negated { "!=" } else { "=" };
                conditions.push(format!("f.status {} ?{}", op, *param_idx));
                params.push(Box::new(value.clone()));
                *param_idx += 1;
            }
            Filter::Priority { op, value, negated } => {
                let priorities = priorities_for_op(*op, value).ok_or_else(|| {
                    ParcError::ParseError(format!("unknown priority '{}'", value))
                })?;
                if priorities.is_empty() {
                    conditions.push("1=0".to_string());
                } else {
                    let placeholders: Vec<String> = priorities
                        .iter()
                        .map(|p| {
                            let ph = format!("?{}", *param_idx);
                            params.push(Box::new(p.clone()));
                            *param_idx += 1;
                            ph
                        })
                        .collect();
                    let not = if *negated { "NOT " } else { "" };
                    conditions.push(format!(
                        "f.priority {}IN ({})",
                        not,
                        placeholders.join(", ")
                    ));
                }
            }
            Filter::Tag { value, negated } => {
                if *negated {
                    conditions.push(format!(
                        "NOT EXISTS (SELECT 1 FROM fragment_tags WHERE fragment_id = f.id AND tag = ?{})",
                        *param_idx
                    ));
                    params.push(Box::new(value.clone()));
                    *param_idx += 1;
                } else {
                    *tag_count += 1;
                    joins.push_str(&format!(
                        " JOIN fragment_tags ft{tag_n} ON ft{tag_n}.fragment_id = f.id AND ft{tag_n}.tag = ?{pi}",
                        tag_n = *tag_count,
                        pi = *param_idx,
                    ));
                    params.push(Box::new(value.clone()));
                    *param_idx += 1;
                    *needs_tag_group_by = true;
                }
            }
            Filter::Due(df) => {
                apply_date_condition("f.due", df, conditions, params, param_idx);
            }
            Filter::Created(df) => {
                apply_date_condition(
                    "date(f.created_at, 'localtime')",
                    df,
                    conditions,
                    params,
                    param_idx,
                );
            }
            Filter::Updated(df) => {
                apply_date_condition(
                    "date(f.updated_at, 'localtime')",
                    df,
                    conditions,
                    params,
                    param_idx,
                );
            }
            Filter::CreatedBy { value, negated } => {
                let op = if *negated { "!=" } else { "=" };
                conditions.push(format!("f.created_by {} ?{}", op, *param_idx));
                params.push(Box::new(value.clone()));
                *param_idx += 1;
            }
            Filter::Has(HasCondition::Links) => {
                conditions.push(
                    "EXISTS (SELECT 1 FROM fragment_links WHERE source_id = f.id)".to_string(),
                );
            }
            Filter::Has(HasCondition::Due) => {
                conditions.push("f.due IS NOT NULL".to_string());
            }
            Filter::Has(HasCondition::Attachments) => {
                conditions.push("f.attachment_count > 0".to_string());
            }
            Filter::Is(IsCondition::Archived) => {
                *has_is_filter = true;
                conditions.push("f.archived = 1".to_string());
            }
            Filter::Is(IsCondition::All) => {
                *has_is_filter = true;
            }
            Filter::Linked(id_prefix) => {
                let prefix = id_prefix.to_uppercase();
                conditions.push(format!(
                    "EXISTS (SELECT 1 FROM fragment_links WHERE \
                     (source_id = f.id AND target_id LIKE ?{pi}) OR \
                     (target_id = f.id AND source_id LIKE ?{pi}))",
                    pi = *param_idx
                ));
                params.push(Box::new(format!("{}%", prefix)));
                *param_idx += 1;
            }
        }
    }
    Ok(())
}

fn build_filter_clauses(
    filters: &[Filter],
    start_param_idx: usize,
) -> Result<FilterClauses, ParcError> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut joins = String::new();
    let mut param_idx: usize = start_param_idx;
    let mut needs_tag_group_by = false;
    let mut tag_count: usize = 0;
    let mut has_is_filter = false;

    apply_filters(
        filters,
        &mut conditions,
        &mut params,
        &mut joins,
        &mut param_idx,
        &mut needs_tag_group_by,
        &mut tag_count,
        &mut has_is_filter,
    )?;

    let _ = param_idx;
    Ok(FilterClauses {
        conditions,
        params,
        joins,
        needs_tag_group_by,
        has_is_filter,
    })
}

fn compile_query(query: &SearchQuery) -> Result<CompiledQuery, ParcError> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut joins = String::new();
    let mut param_idx: usize = 1;
    let mut needs_tag_group_by = false;
    let mut tag_count: usize = 0;
    let mut has_is_filter = false;

    let use_fts = !query.text_terms.is_empty();

    // Count positive tag filters to know if we'll have extra JOINs
    // (snippet() can't be used with additional JOINs beyond the FTS table)
    let has_tag_joins = query
        .filters
        .iter()
        .any(|f| matches!(f, Filter::Tag { negated: false, .. }));

    // Base SELECT
    let base_select = if use_fts {
        let fts_expr = build_fts_expression(&query.text_terms);
        conditions.push(format!("fragments_fts MATCH ?{}", param_idx));
        params.push(Box::new(fts_expr));
        param_idx += 1;
        if has_tag_joins {
            // Can't use snippet() with additional JOINs
            "SELECT f.id, f.type, f.title, f.status, f.updated_at, NULL as snippet \
             FROM fragments f \
             JOIN fragments_fts ON fragments_fts.id = f.id"
                .to_string()
        } else {
            "SELECT f.id, f.type, f.title, f.status, f.updated_at, snippet(fragments_fts, 2, '»', '«', '…', 20) as snippet \
             FROM fragments f \
             JOIN fragments_fts ON fragments_fts.id = f.id".to_string()
        }
    } else {
        "SELECT f.id, f.type, f.title, f.status, f.updated_at, NULL as snippet \
         FROM fragments f"
            .to_string()
    };

    apply_filters(
        &query.filters,
        &mut conditions,
        &mut params,
        &mut joins,
        &mut param_idx,
        &mut needs_tag_group_by,
        &mut tag_count,
        &mut has_is_filter,
    )?;

    // By default, exclude archived fragments unless is:archived or is:all is specified
    if !has_is_filter {
        conditions.push("f.archived = 0".to_string());
    }

    // Build full SQL
    let mut sql = base_select;
    sql += &joins;

    if !conditions.is_empty() {
        sql += " WHERE ";
        sql += &conditions.join(" AND ");
    }

    if needs_tag_group_by {
        sql += " GROUP BY f.id";
    }

    sql += &format!(" ORDER BY {}", sql_order_clause(query.sort));

    if let Some(limit) = query.limit {
        sql += &format!(" LIMIT {}", limit);
    }

    Ok(CompiledQuery { sql, params })
}

fn sql_order_clause(sort: SortOrder) -> &'static str {
    match sort {
        // Score is applied post-fetch by the fuzzy matcher; pre-sort by
        // updated_at so empty-pattern fuzzy queries land in a sensible order.
        SortOrder::UpdatedDesc | SortOrder::Score => "f.updated_at DESC",
        SortOrder::UpdatedAsc => "f.updated_at ASC",
        SortOrder::CreatedDesc => "f.created_at DESC",
        SortOrder::CreatedAsc => "f.created_at ASC",
        SortOrder::Random => "RANDOM()",
    }
}

fn build_fts_expression(terms: &[TextTerm]) -> String {
    let parts: Vec<String> = terms
        .iter()
        .map(|t| match t {
            TextTerm::Word(w) => w.clone(),
            TextTerm::Phrase(p) => format!("\"{}\"", p),
        })
        .collect();
    parts.join(" ")
}

fn apply_date_condition(
    column: &str,
    df: &DateFilter,
    conditions: &mut Vec<String>,
    params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    param_idx: &mut usize,
) {
    match df {
        DateFilter::Relative(rel) => {
            let (start, end) = resolve_relative_date(rel);
            if start == end {
                conditions.push(format!("{} = ?{}", column, *param_idx));
                params.push(Box::new(start));
                *param_idx += 1;
            } else {
                conditions.push(format!(
                    "{col} >= ?{p1} AND {col} <= ?{p2}",
                    col = column,
                    p1 = *param_idx,
                    p2 = *param_idx + 1,
                ));
                params.push(Box::new(start));
                params.push(Box::new(end));
                *param_idx += 2;
            }
        }
        DateFilter::RelativeWithOp { op, rel } => {
            let (start, end) = resolve_relative_date(rel);
            let boundary = match op {
                CompareOp::Lt | CompareOp::Lte => end,
                CompareOp::Gt | CompareOp::Gte | CompareOp::Eq => start,
            };
            let sql_op = match op {
                CompareOp::Eq => "=",
                CompareOp::Lt => "<",
                CompareOp::Gt => ">",
                CompareOp::Lte => "<=",
                CompareOp::Gte => ">=",
            };
            conditions.push(format!("{} {} ?{}", column, sql_op, *param_idx));
            params.push(Box::new(boundary));
            *param_idx += 1;
        }
        DateFilter::Absolute { op, date } => {
            let sql_op = match op {
                CompareOp::Eq => "=",
                CompareOp::Lt => "<",
                CompareOp::Gt => ">",
                CompareOp::Lte => "<=",
                CompareOp::Gte => ">=",
            };
            conditions.push(format!("{} {} ?{}", column, sql_op, *param_idx));
            params.push(Box::new(date.clone()));
            *param_idx += 1;
        }
    }
}

// ── Public search function ─────────────────────────────────────────────

/// Execute a search query against the index.
pub fn search(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchResult>, ParcError> {
    let compiled = compile_query(query)?;

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        compiled.params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&compiled.sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(SearchResult {
            id: row.get(0)?,
            fragment_type: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            tags: Vec::new(),
            updated_at: row.get(4)?,
            snippet: row.get(5)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        let mut result = row.map_err(ParcError::Sqlite)?;
        let tags: Vec<String> = {
            let mut tag_stmt =
                conn.prepare("SELECT tag FROM fragment_tags WHERE fragment_id = ?1 ORDER BY tag")?;
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

// ── Fuzzy search ───────────────────────────────────────────────────────

/// Execute a fuzzy search. Structured filters are applied via SQL; bare
/// text terms are joined into a fuzzy pattern run by [`crate::fuzzy`]; quoted
/// phrases survive as required substrings (case-insensitive) over title or
/// body.
pub fn fuzzy_search(
    conn: &Connection,
    query: &SearchQuery,
) -> Result<Vec<crate::fuzzy::FuzzyHit>, ParcError> {
    let candidates = load_fuzzy_candidates(conn, query)?;

    let pattern: String = query
        .text_terms
        .iter()
        .filter_map(|t| match t {
            TextTerm::Word(w) => Some(w.as_str()),
            TextTerm::Phrase(_) => None,
        })
        .collect::<Vec<_>>()
        .join(" ");

    let phrases: Vec<String> = query
        .text_terms
        .iter()
        .filter_map(|t| match t {
            TextTerm::Phrase(p) => Some(p.to_lowercase()),
            TextTerm::Word(_) => None,
        })
        .collect();

    let mut engine = crate::fuzzy::FuzzyEngine::new();
    engine.set_candidates(candidates);
    engine.set_pattern(&pattern);
    engine.poll_until_done();

    let mut hits = engine.hits(usize::MAX);

    if !phrases.is_empty() {
        hits.retain(|h| {
            let title_lc = h.item.title.to_lowercase();
            let body_lc = h.item.body.to_lowercase();
            phrases
                .iter()
                .all(|p| title_lc.contains(p) || body_lc.contains(p))
        });
    }

    // For non-empty patterns the snapshot is in score order. If the user
    // requested an explicit non-Score sort, re-sort the surviving hits.
    if !pattern.is_empty() {
        match query.sort {
            SortOrder::Score | SortOrder::UpdatedDesc => {
                if matches!(query.sort, SortOrder::UpdatedDesc) {
                    hits.sort_by(|a, b| b.item.updated_at.cmp(&a.item.updated_at));
                }
            }
            SortOrder::UpdatedAsc => hits.sort_by(|a, b| a.item.updated_at.cmp(&b.item.updated_at)),
            SortOrder::CreatedDesc => {
                hits.sort_by(|a, b| b.item.created_at.cmp(&a.item.created_at))
            }
            SortOrder::CreatedAsc => hits.sort_by(|a, b| a.item.created_at.cmp(&b.item.created_at)),
            SortOrder::Random => {
                // Deterministic-enough shuffle without pulling in `rand`:
                // rotate by a value derived from the pattern length.
                let n = hits.len();
                if n > 1 {
                    let shift = pattern.len() % n;
                    hits.rotate_left(shift);
                }
            }
        }
    }
    // For empty pattern, the snapshot preserves SQL insertion order, which
    // already matches the requested sort (see `sql_order_clause`). Nothing to do.

    if let Some(limit) = query.limit {
        hits.truncate(limit);
    }

    Ok(hits)
}

/// Load candidate fragments for fuzzy matching after applying structured
/// filters in SQL. Bare text terms are intentionally ignored here; callers pass
/// them to [`crate::fuzzy::FuzzyEngine`].
pub fn load_fuzzy_candidates(
    conn: &Connection,
    query: &SearchQuery,
) -> Result<Vec<crate::fuzzy::FuzzyItem>, ParcError> {
    // Filter-only: ignore text_terms here; nucleo handles those.
    let clauses = build_filter_clauses(&query.filters, 1)?;

    let mut sql = String::from(
        "SELECT f.id, f.type, f.title, f.status, f.body, f.created_at, f.updated_at \
         FROM fragments f",
    );
    sql.push_str(&clauses.joins);

    let mut all_conditions = clauses.conditions;
    if !clauses.has_is_filter {
        all_conditions.push("f.archived = 0".to_string());
    }
    if !all_conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&all_conditions.join(" AND "));
    }
    if clauses.needs_tag_group_by {
        sql.push_str(" GROUP BY f.id");
    }
    sql.push_str(&format!(" ORDER BY {}", sql_order_clause(query.sort)));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        clauses.params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(crate::fuzzy::FuzzyItem {
            id: row.get(0)?,
            fragment_type: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            body: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            tags: Vec::new(),
        })
    })?;

    let mut items = Vec::new();
    for row in rows {
        let mut item = row.map_err(ParcError::Sqlite)?;
        let mut tag_stmt =
            conn.prepare("SELECT tag FROM fragment_tags WHERE fragment_id = ?1 ORDER BY tag")?;
        item.tags = tag_stmt
            .query_map([&item.id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        items.push(item);
    }
    Ok(items)
}

// ── Tests ──────────────────────────────────────────────────────────────

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
        extra.insert(
            "status".to_string(),
            serde_json::Value::String(status.to_string()),
        );
        Fragment {
            id: fragment::new_id(),
            fragment_type: "todo".to_string(),
            title: title.to_string(),
            tags: tags.clone(),
            links: Vec::new(),
            attachments: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            extra_fields: extra,
            body: format!("Body of {}", title),
        }
    }

    fn make_fragment_with(
        title: &str,
        ftype: &str,
        status: Option<&str>,
        priority: Option<&str>,
        due: Option<&str>,
        tags: Vec<String>,
        body: &str,
    ) -> Fragment {
        let mut extra = BTreeMap::new();
        if let Some(s) = status {
            extra.insert(
                "status".to_string(),
                serde_json::Value::String(s.to_string()),
            );
        }
        if let Some(p) = priority {
            extra.insert(
                "priority".to_string(),
                serde_json::Value::String(p.to_string()),
            );
        }
        if let Some(d) = due {
            extra.insert("due".to_string(), serde_json::Value::String(d.to_string()));
        }
        Fragment {
            id: fragment::new_id(),
            fragment_type: ftype.to_string(),
            title: title.to_string(),
            tags: tags.clone(),
            links: Vec::new(),
            attachments: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            extra_fields: extra,
            body: body.to_string(),
        }
    }

    // ── Parser unit tests ──────────────────────────────────────────────

    #[test]
    fn test_parse_empty() {
        let q = parse_query("").unwrap();
        assert!(q.text_terms.is_empty());
        assert!(q.filters.is_empty());
    }

    #[test]
    fn test_parse_simple_word() {
        let q = parse_query("hello").unwrap();
        assert_eq!(q.text_terms, vec![TextTerm::Word("hello".to_string())]);
        assert!(q.filters.is_empty());
    }

    #[test]
    fn test_parse_multiple_words() {
        let q = parse_query("hello world").unwrap();
        assert_eq!(q.text_terms.len(), 2);
        assert_eq!(q.text_terms[0], TextTerm::Word("hello".to_string()));
        assert_eq!(q.text_terms[1], TextTerm::Word("world".to_string()));
    }

    #[test]
    fn test_parse_phrase() {
        let q = parse_query("\"exact match\"").unwrap();
        assert_eq!(
            q.text_terms,
            vec![TextTerm::Phrase("exact match".to_string())]
        );
    }

    #[test]
    fn test_parse_type_filter() {
        let q = parse_query("type:todo").unwrap();
        assert!(q.text_terms.is_empty());
        assert_eq!(
            q.filters,
            vec![Filter::Type {
                value: "todo".to_string(),
                negated: false
            }]
        );
    }

    #[test]
    fn test_parse_status_negation() {
        let q = parse_query("status:!done").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Status {
                value: "done".to_string(),
                negated: true
            }]
        );
    }

    #[test]
    fn test_parse_hashtag() {
        let q = parse_query("#backend").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Tag {
                value: "backend".to_string(),
                negated: false
            }]
        );
    }

    #[test]
    fn test_parse_tag_filter() {
        let q = parse_query("tag:frontend").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Tag {
                value: "frontend".to_string(),
                negated: false
            }]
        );
    }

    #[test]
    fn test_parse_date_shorthand_today() {
        let q = parse_query("due:today").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Due(DateFilter::Relative(RelativeDate::Today))]
        );
    }

    #[test]
    fn test_parse_date_shorthand_this_week() {
        let q = parse_query("due:this-week").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Due(DateFilter::Relative(RelativeDate::ThisWeek))]
        );
    }

    #[test]
    fn test_parse_date_overdue() {
        let q = parse_query("due:overdue").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Due(DateFilter::Relative(RelativeDate::Overdue))]
        );
    }

    #[test]
    fn test_parse_date_days_ago() {
        let q = parse_query("created:30-days-ago").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Created(DateFilter::Relative(
                RelativeDate::DaysAgo(30)
            ))]
        );
    }

    #[test]
    fn test_parse_date_comparison() {
        let q = parse_query("created:>2026-01-01").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Created(DateFilter::Absolute {
                op: CompareOp::Gt,
                date: "2026-01-01".to_string(),
            })]
        );
    }

    #[test]
    fn test_parse_relative_date_comparison() {
        let q = parse_query("due:<=today").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Due(DateFilter::RelativeWithOp {
                op: CompareOp::Lte,
                rel: RelativeDate::Today,
            })]
        );
    }

    #[test]
    fn test_timestamp_date_filters_use_local_date() {
        let q = SearchQuery {
            text_terms: Vec::new(),
            filters: vec![
                Filter::Created(DateFilter::Absolute {
                    op: CompareOp::Eq,
                    date: "2026-04-27".to_string(),
                }),
                Filter::Updated(DateFilter::Absolute {
                    op: CompareOp::Eq,
                    date: "2026-04-27".to_string(),
                }),
            ],
            sort: SortOrder::UpdatedDesc,
            limit: None,
        };

        let compiled = compile_query(&q).unwrap();

        assert!(compiled
            .sql
            .contains("date(f.created_at, 'localtime') = ?1"));
        assert!(compiled
            .sql
            .contains("date(f.updated_at, 'localtime') = ?2"));
        assert!(!compiled.sql.contains("substr(f.created_at"));
        assert!(!compiled.sql.contains("substr(f.updated_at"));
        assert_eq!(compiled.params.len(), 2);
    }

    #[test]
    fn test_parse_has_links() {
        let q = parse_query("has:links").unwrap();
        assert_eq!(q.filters, vec![Filter::Has(HasCondition::Links)]);
    }

    #[test]
    fn test_parse_has_due() {
        let q = parse_query("has:due").unwrap();
        assert_eq!(q.filters, vec![Filter::Has(HasCondition::Due)]);
    }

    #[test]
    fn test_parse_has_attachments() {
        let q = parse_query("has:attachments").unwrap();
        assert_eq!(q.filters, vec![Filter::Has(HasCondition::Attachments)]);
    }

    #[test]
    fn test_parse_linked() {
        let q = parse_query("linked:01JQ7V3X").unwrap();
        assert_eq!(q.filters, vec![Filter::Linked("01JQ7V3X".to_string())]);
    }

    #[test]
    fn test_parse_priority_comparison() {
        let q = parse_query("priority:>=medium").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Priority {
                op: CompareOp::Gte,
                value: "medium".to_string(),
                negated: false,
            }]
        );
    }

    #[test]
    fn test_parse_combined_query() {
        let q = parse_query("type:todo status:open #backend API").unwrap();
        assert_eq!(q.text_terms, vec![TextTerm::Word("API".to_string())]);
        assert_eq!(q.filters.len(), 3);
        assert_eq!(
            q.filters[0],
            Filter::Type {
                value: "todo".to_string(),
                negated: false
            }
        );
        assert_eq!(
            q.filters[1],
            Filter::Status {
                value: "open".to_string(),
                negated: false
            }
        );
        assert_eq!(
            q.filters[2],
            Filter::Tag {
                value: "backend".to_string(),
                negated: false
            }
        );
    }

    #[test]
    fn test_parse_by_filter() {
        let q = parse_query("by:alice").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::CreatedBy {
                value: "alice".to_string(),
                negated: false
            }]
        );
    }

    #[test]
    fn test_parse_negated_tag() {
        let q = parse_query("tag:!wip").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Tag {
                value: "wip".to_string(),
                negated: true
            }]
        );
    }

    #[test]
    fn test_parse_date_absolute_eq() {
        let q = parse_query("due:2026-03-01").unwrap();
        assert_eq!(
            q.filters,
            vec![Filter::Due(DateFilter::Absolute {
                op: CompareOp::Eq,
                date: "2026-03-01".to_string(),
            })]
        );
    }

    // ── Integration tests ──────────────────────────────────────────────

    #[test]
    fn test_search_fts() {
        let (_tmp, conn) = setup_test_vault();
        let frag = make_todo("SQLite indexing", "open", vec!["search".to_string()]);
        index::index_fragment(&conn, &frag, &["search".to_string()], &[]).unwrap();

        let mut q = parse_query("SQLite").unwrap();
        q.sort = SortOrder::UpdatedDesc;
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "SQLite indexing");
    }

    #[test]
    fn test_search_type_filter() {
        let (_tmp, conn) = setup_test_vault();
        let todo = make_todo("A todo", "open", vec![]);
        index::index_fragment(&conn, &todo, &[], &[]).unwrap();

        let q = parse_query("type:todo").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);

        let q = parse_query("type:note").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_search_status_negation() {
        let (_tmp, conn) = setup_test_vault();
        let open = make_todo("Open task", "open", vec![]);
        let done = make_todo("Done task", "done", vec![]);
        index::index_fragment(&conn, &open, &[], &[]).unwrap();
        index::index_fragment(&conn, &done, &[], &[]).unwrap();

        let q = parse_query("status:!done").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Open task");
    }

    #[test]
    fn test_search_tag_filter_and() {
        let (_tmp, conn) = setup_test_vault();
        let frag = make_todo("Tagged", "open", vec!["a".to_string(), "b".to_string()]);
        index::index_fragment(&conn, &frag, &["a".to_string(), "b".to_string()], &[]).unwrap();

        let frag2 = make_todo("Only a", "open", vec!["a".to_string()]);
        index::index_fragment(&conn, &frag2, &["a".to_string()], &[]).unwrap();

        let q = parse_query("#a #b").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Tagged");
    }

    #[test]
    fn test_search_negated_tag() {
        let (_tmp, conn) = setup_test_vault();
        let frag1 = make_todo("With wip", "open", vec!["wip".to_string()]);
        let frag2 = make_todo("Without wip", "open", vec!["other".to_string()]);
        index::index_fragment(&conn, &frag1, &["wip".to_string()], &[]).unwrap();
        index::index_fragment(&conn, &frag2, &["other".to_string()], &[]).unwrap();

        let q = parse_query("tag:!wip").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Without wip");
    }

    #[test]
    fn test_search_phrase() {
        let (_tmp, conn) = setup_test_vault();
        let frag1 = make_fragment_with(
            "Exact phrase test",
            "note",
            None,
            None,
            None,
            vec![],
            "This has exact match inside",
        );
        let frag2 = make_fragment_with(
            "Partial",
            "note",
            None,
            None,
            None,
            vec![],
            "This has exact but not match nearby",
        );
        index::index_fragment(&conn, &frag1, &[], &[]).unwrap();
        index::index_fragment(&conn, &frag2, &[], &[]).unwrap();

        let q = parse_query("\"exact match\"").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Exact phrase test");
    }

    #[test]
    fn test_search_priority_gte() {
        let (_tmp, conn) = setup_test_vault();
        let low = make_fragment_with(
            "Low pri",
            "todo",
            Some("open"),
            Some("low"),
            None,
            vec![],
            "",
        );
        let med = make_fragment_with(
            "Med pri",
            "todo",
            Some("open"),
            Some("medium"),
            None,
            vec![],
            "",
        );
        let high = make_fragment_with(
            "High pri",
            "todo",
            Some("open"),
            Some("high"),
            None,
            vec![],
            "",
        );
        index::index_fragment(&conn, &low, &[], &[]).unwrap();
        index::index_fragment(&conn, &med, &[], &[]).unwrap();
        index::index_fragment(&conn, &high, &[], &[]).unwrap();

        let q = parse_query("priority:>=medium").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 2);
        let titles: Vec<&str> = results.iter().map(|r| r.title.as_str()).collect();
        assert!(titles.contains(&"Med pri"));
        assert!(titles.contains(&"High pri"));
    }

    #[test]
    fn test_search_has_links() {
        let (_tmp, conn) = setup_test_vault();
        let frag1 = make_fragment_with("With links", "note", None, None, None, vec![], "body");
        let frag2 = make_fragment_with("No links", "note", None, None, None, vec![], "body");
        index::index_fragment(&conn, &frag1, &[], &["SOME_TARGET_ID".to_string()]).unwrap();
        index::index_fragment(&conn, &frag2, &[], &[]).unwrap();

        let q = parse_query("has:links").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "With links");
    }

    #[test]
    fn test_search_has_due() {
        let (_tmp, conn) = setup_test_vault();
        let with_due = make_fragment_with(
            "Has due",
            "todo",
            Some("open"),
            None,
            Some("2026-03-01"),
            vec![],
            "",
        );
        let no_due = make_fragment_with("No due", "todo", Some("open"), None, None, vec![], "");
        index::index_fragment(&conn, &with_due, &[], &[]).unwrap();
        index::index_fragment(&conn, &no_due, &[], &[]).unwrap();

        let q = parse_query("has:due").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Has due");
    }

    #[test]
    fn test_search_linked() {
        let (_tmp, conn) = setup_test_vault();
        let target = make_fragment_with("Target", "note", None, None, None, vec![], "body");
        let linker = make_fragment_with("Linker", "note", None, None, None, vec![], "body");
        let other = make_fragment_with("Other", "note", None, None, None, vec![], "body");
        index::index_fragment(&conn, &target, &[], &[]).unwrap();
        index::index_fragment(&conn, &linker, &[], &[target.id.clone()]).unwrap();
        index::index_fragment(&conn, &other, &[], &[]).unwrap();

        // Search for fragments linked to target's prefix
        let prefix = &target.id[..8];
        let q = parse_query(&format!("linked:{}", prefix)).unwrap();
        let results = search(&conn, &q).unwrap();
        // Should find both the linker (source) and target (target in reverse)
        let ids: Vec<&str> = results.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&linker.id.as_str()));
    }

    #[test]
    fn test_search_due_date_absolute() {
        let (_tmp, conn) = setup_test_vault();
        let frag1 = make_fragment_with(
            "Due soon",
            "todo",
            Some("open"),
            None,
            Some("2026-03-01"),
            vec![],
            "",
        );
        let frag2 = make_fragment_with(
            "Due later",
            "todo",
            Some("open"),
            None,
            Some("2026-06-01"),
            vec![],
            "",
        );
        index::index_fragment(&conn, &frag1, &[], &[]).unwrap();
        index::index_fragment(&conn, &frag2, &[], &[]).unwrap();

        let q = parse_query("due:<2026-04-01").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Due soon");
    }

    #[test]
    fn test_search_combined_dsl() {
        let (_tmp, conn) = setup_test_vault();
        let frag1 = make_fragment_with(
            "Important API task",
            "todo",
            Some("open"),
            Some("high"),
            None,
            vec!["backend".to_string()],
            "Implement the API endpoint",
        );
        let frag2 = make_fragment_with(
            "API docs",
            "note",
            None,
            None,
            None,
            vec!["backend".to_string()],
            "Document the API",
        );
        index::index_fragment(&conn, &frag1, &["backend".to_string()], &[]).unwrap();
        index::index_fragment(&conn, &frag2, &["backend".to_string()], &[]).unwrap();

        let q = parse_query("type:todo #backend API").unwrap();
        let results = search(&conn, &q).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Important API task");
    }

    // ── Fuzzy integration tests ────────────────────────────────────────

    fn index_simple(
        conn: &Connection,
        title: &str,
        ftype: &str,
        body: &str,
        tags: Vec<String>,
    ) -> String {
        let frag = make_fragment_with(title, ftype, None, None, None, tags.clone(), body);
        let id = frag.id.clone();
        index::index_fragment(&conn, &frag, &tags, &[]).unwrap();
        id
    }

    #[test]
    fn fuzzy_search_subsequence_in_title() {
        let (_tmp, conn) = setup_test_vault();
        index_simple(&conn, "fileserver", "note", "", vec![]);
        index_simple(&conn, "filesharer", "note", "", vec![]);
        index_simple(&conn, "unrelated", "note", "", vec![]);

        let q = parse_query("flsr").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        let titles: Vec<&str> = hits.iter().map(|h| h.item.title.as_str()).collect();
        assert!(titles.contains(&"fileserver"));
        assert!(titles.contains(&"filesharer"));
        assert!(!titles.contains(&"unrelated"));
    }

    #[test]
    fn fuzzy_search_matches_body() {
        let (_tmp, conn) = setup_test_vault();
        index_simple(
            &conn,
            "alpha",
            "note",
            "this body mentions xyzzy somewhere",
            vec![],
        );
        index_simple(&conn, "beta", "note", "no match here", vec![]);

        let q = parse_query("xyzzy").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        let titles: Vec<&str> = hits.iter().map(|h| h.item.title.as_str()).collect();
        assert_eq!(titles, vec!["alpha"]);
    }

    #[test]
    fn fuzzy_search_combines_with_filters() {
        let (_tmp, conn) = setup_test_vault();
        index_simple(&conn, "fileserver", "todo", "", vec!["backend".to_string()]);
        index_simple(&conn, "fileshare", "note", "", vec![]);

        let q = parse_query("type:todo flsr").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.title, "fileserver");
        assert_eq!(hits[0].item.fragment_type, "todo");
    }

    #[test]
    fn fuzzy_search_empty_pattern_returns_filtered_set_in_sql_order() {
        let (_tmp, conn) = setup_test_vault();
        index_simple(&conn, "first", "todo", "", vec![]);
        index_simple(&conn, "second", "note", "", vec![]);
        index_simple(&conn, "third", "todo", "", vec![]);

        let q = parse_query("type:todo").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        assert_eq!(hits.len(), 2);
        let titles: Vec<&str> = hits.iter().map(|h| h.item.title.as_str()).collect();
        // Both todos present; order is updated_at DESC (which is last inserted first when timestamps tie).
        assert!(titles.contains(&"first"));
        assert!(titles.contains(&"third"));
    }

    #[test]
    fn fuzzy_search_phrase_post_filter() {
        let (_tmp, conn) = setup_test_vault();
        index_simple(
            &conn,
            "alpha doc",
            "note",
            "talks about exact thing",
            vec![],
        );
        index_simple(
            &conn,
            "alpha note",
            "note",
            "talks about something else",
            vec![],
        );

        let q = parse_query("alpha \"exact thing\"").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].item.title, "alpha doc");
    }

    #[test]
    fn fuzzy_search_respects_limit() {
        let (_tmp, conn) = setup_test_vault();
        for i in 0..10 {
            index_simple(&conn, &format!("alpha {}", i), "note", "", vec![]);
        }
        let mut q = parse_query("alpha").unwrap();
        q.limit = Some(3);
        let hits = fuzzy_search(&conn, &q).unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[test]
    fn fuzzy_search_title_match_indices_present() {
        let (_tmp, conn) = setup_test_vault();
        index_simple(&conn, "fileserver", "note", "", vec![]);
        let q = parse_query("flsr").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        assert_eq!(hits.len(), 1);
        let indices = &hits[0].title_match_indices;
        assert!(!indices.is_empty());
        for &i in indices {
            assert!(i < hits[0].item.title.chars().count() as u32);
        }
    }

    #[test]
    fn fuzzy_search_excludes_archived_by_default() {
        let (_tmp, conn) = setup_test_vault();
        // Insert an archived fragment manually
        let mut frag = make_fragment_with("findme", "note", None, None, None, vec![], "");
        frag.extra_fields
            .insert("archived".to_string(), serde_json::Value::Bool(true));
        index::index_fragment(&conn, &frag, &[], &[]).unwrap();

        let q = parse_query("findme").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        assert!(hits.is_empty());

        let q = parse_query("findme is:all").unwrap();
        let hits = fuzzy_search(&conn, &q).unwrap();
        assert_eq!(hits.len(), 1);
    }
}
