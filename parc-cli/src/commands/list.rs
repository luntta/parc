use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::index::open_index;
use parc_core::search::{self, Filter, SearchQuery, SortOrder};

use crate::render;

pub fn run(
    vault: &Path,
    type_name: Option<String>,
    status: Option<String>,
    tags: Vec<String>,
    json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let config = load_config(vault)?;
    let conn = open_index(vault)?;

    let mut filters = Vec::new();
    if let Some(t) = type_name {
        filters.push(Filter::Type {
            value: t,
            negated: false,
        });
    }
    if let Some(s) = status {
        filters.push(Filter::Status {
            value: s,
            negated: false,
        });
    }
    for tag in tags {
        filters.push(Filter::Tag {
            value: tag,
            negated: false,
        });
    }

    let query = SearchQuery {
        text_terms: Vec::new(),
        filters,
        sort: SortOrder::UpdatedDesc,
        limit,
    };

    let results = search::search(&conn, &query)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("No fragments found.");
    } else {
        render::print_table(&results, config.id_display_length);
    }

    Ok(())
}
