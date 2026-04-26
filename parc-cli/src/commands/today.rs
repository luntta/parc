use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::search::{CompareOp, DateFilter, Filter, SearchQuery, SortOrder};

use crate::commands::resurfacing;
use crate::render::{self, SearchSection};

pub fn run(vault: &Path, json: bool) -> Result<()> {
    let config = load_config(vault)?;
    let limit = config.resurfacing.today_section_limit;
    let today = resurfacing::today_string();

    let touched_created = SearchQuery {
        text_terms: Vec::new(),
        filters: vec![Filter::Created(DateFilter::Absolute {
            op: CompareOp::Eq,
            date: today.clone(),
        })],
        sort: SortOrder::UpdatedDesc,
        limit: Some(limit),
    };
    let touched_updated = SearchQuery {
        text_terms: Vec::new(),
        filters: vec![Filter::Updated(DateFilter::Absolute {
            op: CompareOp::Eq,
            date: today.clone(),
        })],
        sort: SortOrder::UpdatedDesc,
        limit: Some(limit),
    };
    let touched = resurfacing::merge_unique(vec![
        resurfacing::run_search(vault, &touched_created)?,
        resurfacing::run_search(vault, &touched_updated)?,
    ]);

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
    let due = resurfacing::run_search(
        vault,
        &SearchQuery {
            text_terms: Vec::new(),
            filters: due_filters,
            sort: SortOrder::CreatedAsc,
            limit: Some(limit),
        },
    )?;

    let high_priority = resurfacing::run_search(
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
    )?;

    let (touched_results, touched_total) = resurfacing::cap_results(touched, limit);
    let sections = vec![
        SearchSection {
            title: "Touched today".to_string(),
            total: touched_total,
            results: touched_results,
        },
        SearchSection {
            title: "Due today / overdue".to_string(),
            total: due.len(),
            results: due,
        },
        SearchSection {
            title: "Open & high priority".to_string(),
            total: high_priority.len(),
            results: high_priority,
        },
    ];

    if json {
        let json_val = serde_json::json!({
            "touched_today": &sections[0].results,
            "due_today_overdue": &sections[1].results,
            "open_high_priority": &sections[2].results,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        render::print_sections(&sections, config.id_display_length);
    }

    Ok(())
}
