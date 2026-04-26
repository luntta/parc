use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::search::{Filter, SearchQuery, SortOrder};

use crate::commands::resurfacing;
use crate::render;

pub fn run(
    vault: &Path,
    limit: Option<usize>,
    type_name: Option<String>,
    include_done: bool,
    json: bool,
) -> Result<()> {
    let config = load_config(vault)?;
    let mut filters = Vec::new();

    if let Some(t) = type_name {
        filters.push(Filter::Type {
            value: t.clone(),
            negated: false,
        });
        if !include_done && t != "note" {
            filters.extend(resurfacing::unfinished_status_filters());
        }
    } else {
        filters.push(Filter::Type {
            value: "todo".to_string(),
            negated: true,
        });
    }

    let query = SearchQuery {
        text_terms: Vec::new(),
        filters,
        sort: SortOrder::Random,
        limit: Some(limit.unwrap_or(1)),
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
