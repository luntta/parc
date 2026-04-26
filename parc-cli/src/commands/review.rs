use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::date;
use parc_core::search::{CompareOp, DateFilter, Filter, SearchQuery, SortOrder};

use crate::commands::resurfacing;
use crate::render::{self, SearchSection};

pub fn run(vault: &Path, since: Option<String>, json: bool) -> Result<()> {
    let config = load_config(vault)?;
    let window = since.unwrap_or_else(|| config.resurfacing.review_window.clone());
    let window_filter = date::parse_relative_date(&window)
        .map(DateFilter::Relative)
        .unwrap_or(DateFilter::Absolute {
            op: CompareOp::Gte,
            date: window,
        });
    let next_week = resurfacing::in_days_string(7);
    let stale_cutoff = resurfacing::days_ago_string(config.resurfacing.stale_days);

    let edited = resurfacing::run_search(
        vault,
        &SearchQuery {
            text_terms: Vec::new(),
            filters: vec![Filter::Updated(window_filter.clone())],
            sort: SortOrder::UpdatedDesc,
            limit: None,
        },
    )?;

    let created = resurfacing::run_search(
        vault,
        &SearchQuery {
            text_terms: Vec::new(),
            filters: vec![Filter::Created(window_filter.clone())],
            sort: SortOrder::CreatedDesc,
            limit: None,
        },
    )?;

    let decisions = resurfacing::run_search(
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
            limit: None,
        },
    )?;

    let risks = resurfacing::run_search(
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
            limit: None,
        },
    )?;

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
    let due = resurfacing::run_search(
        vault,
        &SearchQuery {
            text_terms: Vec::new(),
            filters: due_filters,
            sort: SortOrder::CreatedAsc,
            limit: None,
        },
    )?;

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
    let stale = resurfacing::run_search(
        vault,
        &SearchQuery {
            text_terms: Vec::new(),
            filters: stale_filters,
            sort: SortOrder::UpdatedAsc,
            limit: None,
        },
    )?;

    let sections = vec![
        SearchSection {
            title: "Edited".to_string(),
            total: edited.len(),
            results: edited,
        },
        SearchSection {
            title: "Created".to_string(),
            total: created.len(),
            results: created,
        },
        SearchSection {
            title: "Decisions accepted".to_string(),
            total: decisions.len(),
            results: decisions,
        },
        SearchSection {
            title: "Risks identified".to_string(),
            total: risks.len(),
            results: risks,
        },
        SearchSection {
            title: "Open todos due soon".to_string(),
            total: due.len(),
            results: due,
        },
        SearchSection {
            title: "Stale todos".to_string(),
            total: stale.len(),
            results: stale,
        },
    ];

    if json {
        let json_val = serde_json::json!({
            "edited": &sections[0].results,
            "created": &sections[1].results,
            "decisions_accepted": &sections[2].results,
            "risks_identified": &sections[3].results,
            "open_todos_due_soon": &sections[4].results,
            "stale_todos": &sections[5].results,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        render::print_sections(&sections, config.id_display_length);
    }

    Ok(())
}
