use std::path::Path;

use anyhow::Result;
use parc_core::config::Config;
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
