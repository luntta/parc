use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::search::{CompareOp, DateFilter, Filter, SearchQuery, SortOrder};

use crate::commands::resurfacing;
use crate::render;

pub fn run(
    vault: &Path,
    days: Option<u64>,
    types: Vec<String>,
    json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let config = load_config(vault)?;
    let stale_days = days.unwrap_or(config.resurfacing.stale_days);
    let cutoff = resurfacing::days_ago_string(stale_days);
    let types = if types.is_empty() {
        vec![
            "todo".to_string(),
            "decision".to_string(),
            "risk".to_string(),
        ]
    } else {
        types
    };

    let mut groups = Vec::new();
    for fragment_type in types {
        let mut filters = vec![
            Filter::Type {
                value: fragment_type,
                negated: false,
            },
            Filter::Updated(DateFilter::Absolute {
                op: CompareOp::Lt,
                date: cutoff.clone(),
            }),
        ];
        filters.extend(resurfacing::unfinished_status_filters());

        let query = SearchQuery {
            text_terms: Vec::new(),
            filters,
            sort: SortOrder::UpdatedAsc,
            limit,
        };
        groups.push(resurfacing::run_search(vault, &query)?);
    }

    let mut results = resurfacing::merge_unique(groups);
    results.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
    if let Some(limit) = limit {
        results.truncate(limit);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("No fragments found.");
    } else {
        render::print_table(&results, config.id_display_length);
    }

    Ok(())
}
