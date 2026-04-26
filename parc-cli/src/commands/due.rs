use std::path::Path;

use anyhow::{bail, Result};
use parc_core::config::load_config;
use parc_core::search::{CompareOp, DateFilter, Filter, SearchQuery, SortOrder};

use crate::commands::resurfacing;
use crate::render;

pub fn run(vault: &Path, bucket: Option<String>, json: bool) -> Result<()> {
    let config = load_config(vault)?;
    let today = resurfacing::today_string();
    let next_week = resurfacing::in_days_string(7);

    let due_filter = match bucket.as_deref().unwrap_or("default") {
        "today" => DateFilter::Absolute {
            op: CompareOp::Eq,
            date: today,
        },
        "overdue" => DateFilter::Absolute {
            op: CompareOp::Lt,
            date: today,
        },
        "this-week" | "default" => DateFilter::Absolute {
            op: CompareOp::Lte,
            date: next_week,
        },
        other => bail!("unknown due bucket '{}': expected today, overdue, or this-week", other),
    };

    let mut filters = vec![
        Filter::Type {
            value: "todo".to_string(),
            negated: false,
        },
        Filter::Due(due_filter),
    ];
    filters.extend(resurfacing::unfinished_status_filters());

    let query = SearchQuery {
        text_terms: Vec::new(),
        filters,
        sort: SortOrder::CreatedAsc,
        limit: None,
    };

    let results = resurfacing::run_search(vault, &query)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("No fragments found.");
    } else {
        render::print_table(&results, config.id_display_length);
    }

    Ok(())
}
