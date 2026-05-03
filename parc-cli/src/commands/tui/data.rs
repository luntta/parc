use std::path::Path;

use anyhow::Result;
use parc_core::config::Config;
use parc_core::date;
use parc_core::fragment::read_fragment;
use parc_core::fuzzy::{FuzzyEngine, FuzzyHit};
use parc_core::index::open_index;
use parc_core::search::{
    self, CompareOp, DateFilter, Filter, SearchQuery, SearchResult, SortOrder, TextTerm,
};

use super::{Row, Tab};
use crate::commands::resurfacing;

pub(super) struct SearchState {
    engine: FuzzyEngine,
    loaded_filters: Option<Vec<Filter>>,
    stale: bool,
}

impl SearchState {
    pub(super) fn new() -> Self {
        Self {
            engine: FuzzyEngine::new(),
            loaded_filters: None,
            stale: false,
        }
    }

    pub(super) fn mark_stale(&mut self) {
        self.stale = true;
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn load_rows(vault: &Path, tab: Tab, config: &Config) -> Result<Vec<Row>> {
    match tab {
        Tab::Today => load_today_rows(vault, config),
        Tab::List => query_rows(
            vault,
            SearchQuery {
                text_terms: Vec::new(),
                filters: Vec::new(),
                sort: SortOrder::UpdatedDesc,
                limit: Some(200),
            },
        ),
        Tab::Stale => load_stale_rows(vault, config),
        Tab::Due => load_due_rows(vault),
        Tab::Review => load_review_rows(vault, config),
        Tab::Search => Ok(Vec::new()),
    }
}

pub(super) fn load_search_rows(
    vault: &Path,
    search_input: &str,
    state: &mut SearchState,
) -> Result<Vec<Row>> {
    let mut query = search::parse_query(search_input)?;
    query.sort = SortOrder::Score;
    query.limit = Some(200);

    if state.stale || state.loaded_filters.as_ref() != Some(&query.filters) {
        let conn = open_index(vault)?;
        let candidates = search::load_fuzzy_candidates(&conn, &query)?;
        state.engine.set_candidates(candidates);
        state.loaded_filters = Some(query.filters.clone());
        state.stale = false;
    }

    let pattern = fuzzy_pattern(&query);
    state.engine.set_pattern(&pattern);
    state.engine.poll_until_done();

    if search_input.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut hits = state.engine.hits(usize::MAX);
    retain_phrases(&mut hits, &query);
    retain_visible_matches(&mut hits, &query);

    if let Some(limit) = query.limit {
        hits.truncate(limit);
    }

    Ok(hits.into_iter().map(Row::from).collect())
}

fn query_rows(vault: &Path, query: SearchQuery) -> Result<Vec<Row>> {
    let mut rows: Vec<Row> = resurfacing::run_search(vault, &query)?
        .into_iter()
        .map(Row::from)
        .collect();
    hydrate_rows(vault, &mut rows);
    Ok(rows)
}

fn fuzzy_pattern(query: &SearchQuery) -> String {
    query
        .text_terms
        .iter()
        .filter_map(|term| match term {
            TextTerm::Word(word) => Some(word.as_str()),
            TextTerm::Phrase(_) => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn retain_phrases(hits: &mut Vec<FuzzyHit>, query: &SearchQuery) {
    let phrases: Vec<String> = query
        .text_terms
        .iter()
        .filter_map(|term| match term {
            TextTerm::Phrase(phrase) => Some(phrase.to_lowercase()),
            TextTerm::Word(_) => None,
        })
        .collect();
    if phrases.is_empty() {
        return;
    }

    hits.retain(|hit| {
        let title_lc = hit.item.title.to_lowercase();
        let body_lc = hit.item.body.to_lowercase();
        phrases
            .iter()
            .all(|phrase| title_lc.contains(phrase) || body_lc.contains(phrase))
    });
}

fn retain_visible_matches(hits: &mut Vec<FuzzyHit>, query: &SearchQuery) {
    let terms: Vec<String> = query
        .text_terms
        .iter()
        .map(|term| match term {
            TextTerm::Word(word) | TextTerm::Phrase(word) => word.to_lowercase(),
        })
        .filter(|term| !term.is_empty())
        .collect();
    if terms.is_empty() {
        return;
    }

    hits.retain(|hit| {
        if !hit.title_match_indices.is_empty() {
            return true;
        }

        let title_lc = hit.item.title.to_lowercase();
        let body_lc = hit.item.body.to_lowercase();
        terms
            .iter()
            .any(|term| title_lc.contains(term) || body_lc.contains(term))
    });
}

fn load_today_rows(vault: &Path, config: &Config) -> Result<Vec<Row>> {
    let today = resurfacing::today_string();
    let limit = config.resurfacing.today_section_limit;
    let mut rows = Vec::new();

    let touched = resurfacing::merge_unique(vec![
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![Filter::Created(DateFilter::Absolute {
                    op: CompareOp::Eq,
                    date: today.clone(),
                })],
                sort: SortOrder::UpdatedDesc,
                limit: Some(limit),
            },
        )?,
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![Filter::Updated(DateFilter::Absolute {
                    op: CompareOp::Eq,
                    date: today.clone(),
                })],
                sort: SortOrder::UpdatedDesc,
                limit: Some(limit),
            },
        )?,
    ]);
    push_section(&mut rows, "Touched today", touched.into_iter().take(limit));

    let mut due_filters = vec![
        Filter::Type {
            value: "todo".to_string(),
            negated: false,
        },
        Filter::Due(DateFilter::Absolute {
            op: CompareOp::Lte,
            date: today,
        }),
    ];
    due_filters.extend(resurfacing::unfinished_status_filters());
    push_section(
        &mut rows,
        "Due today / overdue",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: due_filters,
                sort: SortOrder::CreatedAsc,
                limit: Some(limit),
            },
        )?
        .into_iter(),
    );

    push_section(
        &mut rows,
        "Open & high priority",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![
                    Filter::Type {
                        value: "todo".to_string(),
                        negated: false,
                    },
                    Filter::Status {
                        value: "open".to_string(),
                        negated: false,
                    },
                    Filter::Priority {
                        op: CompareOp::Gte,
                        value: "high".to_string(),
                        negated: false,
                    },
                ],
                sort: SortOrder::UpdatedDesc,
                limit: Some(limit),
            },
        )?
        .into_iter(),
    );

    hydrate_rows(vault, &mut rows);
    Ok(rows)
}

fn load_stale_rows(vault: &Path, config: &Config) -> Result<Vec<Row>> {
    let cutoff = resurfacing::days_ago_string(config.resurfacing.stale_days);
    let mut groups = Vec::new();

    for fragment_type in ["todo", "decision", "risk"] {
        let mut filters = vec![
            Filter::Type {
                value: fragment_type.to_string(),
                negated: false,
            },
            Filter::Updated(DateFilter::Absolute {
                op: CompareOp::Lt,
                date: cutoff.clone(),
            }),
        ];
        filters.extend(resurfacing::unfinished_status_filters());
        groups.push(resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters,
                sort: SortOrder::UpdatedAsc,
                limit: Some(200),
            },
        )?);
    }

    let mut results = resurfacing::merge_unique(groups);
    results.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
    let mut rows: Vec<Row> = results.into_iter().map(Row::from).collect();
    hydrate_rows(vault, &mut rows);
    Ok(rows)
}

fn load_due_rows(vault: &Path) -> Result<Vec<Row>> {
    let next_week = resurfacing::in_days_string(7);
    let mut filters = vec![
        Filter::Type {
            value: "todo".to_string(),
            negated: false,
        },
        Filter::Due(DateFilter::Absolute {
            op: CompareOp::Lte,
            date: next_week,
        }),
    ];
    filters.extend(resurfacing::unfinished_status_filters());

    let mut rows = query_rows(
        vault,
        SearchQuery {
            text_terms: Vec::new(),
            filters,
            sort: SortOrder::CreatedAsc,
            limit: Some(200),
        },
    )?;
    for row in &mut rows {
        row.section = due_section(row);
    }
    Ok(rows)
}

fn due_section(row: &Row) -> Option<String> {
    let due = row.due.as_deref()?;
    let today = resurfacing::today_string();
    let section = if due < today.as_str() {
        "Overdue"
    } else if due == today.as_str() {
        "Due today"
    } else {
        "Due soon"
    };
    Some(section.to_string())
}

fn load_review_rows(vault: &Path, config: &Config) -> Result<Vec<Row>> {
    let window = config.resurfacing.review_window.clone();
    let window_filter = date::parse_relative_date(&window)
        .map(DateFilter::Relative)
        .unwrap_or(DateFilter::Absolute {
            op: CompareOp::Gte,
            date: window,
        });
    let next_week = resurfacing::in_days_string(7);
    let stale_cutoff = resurfacing::days_ago_string(config.resurfacing.stale_days);

    let mut rows = Vec::new();

    push_section(
        &mut rows,
        "Edited",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![Filter::Updated(window_filter.clone())],
                sort: SortOrder::UpdatedDesc,
                limit: Some(200),
            },
        )?
        .into_iter(),
    );

    push_section(
        &mut rows,
        "Created",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![Filter::Created(window_filter.clone())],
                sort: SortOrder::CreatedDesc,
                limit: Some(200),
            },
        )?
        .into_iter(),
    );

    push_section(
        &mut rows,
        "Decisions accepted",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![
                    Filter::Type {
                        value: "decision".to_string(),
                        negated: false,
                    },
                    Filter::Status {
                        value: "accepted".to_string(),
                        negated: false,
                    },
                    Filter::Updated(window_filter.clone()),
                ],
                sort: SortOrder::UpdatedDesc,
                limit: Some(200),
            },
        )?
        .into_iter(),
    );

    push_section(
        &mut rows,
        "Risks identified",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![
                    Filter::Type {
                        value: "risk".to_string(),
                        negated: false,
                    },
                    Filter::Created(window_filter),
                ],
                sort: SortOrder::CreatedDesc,
                limit: Some(200),
            },
        )?
        .into_iter(),
    );

    let mut due_filters = vec![
        Filter::Type {
            value: "todo".to_string(),
            negated: false,
        },
        Filter::Due(DateFilter::Absolute {
            op: CompareOp::Lte,
            date: next_week,
        }),
    ];
    due_filters.extend(resurfacing::unfinished_status_filters());
    push_section(
        &mut rows,
        "Open todos due soon",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: due_filters,
                sort: SortOrder::CreatedAsc,
                limit: Some(200),
            },
        )?
        .into_iter(),
    );

    let mut stale_filters = vec![
        Filter::Type {
            value: "todo".to_string(),
            negated: false,
        },
        Filter::Updated(DateFilter::Absolute {
            op: CompareOp::Lt,
            date: stale_cutoff,
        }),
    ];
    stale_filters.extend(resurfacing::unfinished_status_filters());
    push_section(
        &mut rows,
        "Stale todos",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: stale_filters,
                sort: SortOrder::UpdatedAsc,
                limit: Some(200),
            },
        )?
        .into_iter(),
    );

    hydrate_rows(vault, &mut rows);
    Ok(rows)
}

fn push_section(rows: &mut Vec<Row>, section: &str, results: impl Iterator<Item = SearchResult>) {
    for mut row in results.map(Row::from) {
        row.section = Some(section.to_string());
        rows.push(row);
    }
}

fn hydrate_rows(vault: &Path, rows: &mut [Row]) {
    for row in rows {
        let Ok(fragment) = read_fragment(vault, &row.id) else {
            continue;
        };
        row.priority = string_field(&fragment.extra_fields, "priority");
        row.due = string_field(&fragment.extra_fields, "due");
        row.assignee = string_field(&fragment.extra_fields, "assignee");
        if row.tags.is_empty() {
            row.tags = fragment.tags;
        }
    }
}

fn string_field(
    fields: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<String> {
    fields
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parc_core::fuzzy::FuzzyItem;

    fn hit(id: &str, title: &str, body: &str, title_match_indices: Vec<u32>) -> FuzzyHit {
        FuzzyHit {
            item: FuzzyItem {
                id: id.to_string(),
                title: title.to_string(),
                body: body.to_string(),
                fragment_type: "note".to_string(),
                status: None,
                priority: None,
                due: None,
                assignee: None,
                tags: Vec::new(),
                created_at: "2026-05-03T00:00:00Z".to_string(),
                updated_at: "2026-05-03T00:00:00Z".to_string(),
            },
            score: 1,
            title_match_indices,
        }
    }

    #[test]
    fn retain_visible_matches_drops_body_only_fuzzy_noise() {
        let query = search::parse_query("tui").unwrap();
        let mut hits = vec![
            hit("exact-title", "TUI launcher", "", Vec::new()),
            hit("exact-body", "Launcher", "mentions tui here", Vec::new()),
            hit("title-fuzzy", "Path traversal via ID", "", vec![2, 8, 16]),
            hit("body-fuzzy", "Unrelated", "the ugly issue", Vec::new()),
        ];

        retain_visible_matches(&mut hits, &query);

        let ids = hits.into_iter().map(|hit| hit.item.id).collect::<Vec<_>>();
        assert_eq!(ids, vec!["exact-title", "exact-body", "title-fuzzy"]);
    }

    #[test]
    fn retain_visible_matches_keeps_structured_filter_results() {
        let query = search::parse_query("#tui").unwrap();
        let mut hits = vec![hit("tag-only", "No visible term", "", Vec::new())];

        retain_visible_matches(&mut hits, &query);

        assert_eq!(hits.len(), 1);
    }
}
