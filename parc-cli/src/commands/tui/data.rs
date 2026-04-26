use std::path::Path;

use anyhow::Result;
use parc_core::config::Config;
use parc_core::search::{CompareOp, DateFilter, Filter, SearchQuery, SearchResult, SortOrder};

use super::{Row, Tab};
use crate::commands::resurfacing;

pub(super) fn load_rows(
    vault: &Path,
    tab: Tab,
    search_input: &str,
    config: &Config,
) -> Result<Vec<Row>> {
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
        Tab::Search => {
            if search_input.trim().is_empty() {
                return Ok(Vec::new());
            }
            let mut query = parc_core::search::parse_query(search_input)?;
            query.limit = Some(200);
            query_rows(vault, query)
        }
    }
}

fn query_rows(vault: &Path, query: SearchQuery) -> Result<Vec<Row>> {
    Ok(resurfacing::run_search(vault, &query)?
        .into_iter()
        .map(Row::from)
        .collect())
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
    Ok(results.into_iter().map(Row::from).collect())
}

fn push_section(rows: &mut Vec<Row>, section: &str, results: impl Iterator<Item = SearchResult>) {
    for mut row in results.map(Row::from) {
        row.section = Some(section.to_string());
        rows.push(row);
    }
}
