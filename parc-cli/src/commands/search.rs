use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::index::open_index;
use parc_core::search::{self, SearchParams, SortOrder};

use crate::render;

#[allow(clippy::too_many_arguments)]
pub fn run(
    vault: &Path,
    query: Vec<String>,
    type_filter: Option<String>,
    status: Option<String>,
    tags: Vec<String>,
    json: bool,
    sort: Option<String>,
    limit: Option<usize>,
) -> Result<()> {
    let config = load_config(vault)?;
    let conn = open_index(vault)?;

    let query_str = if query.is_empty() {
        None
    } else {
        Some(query.join(" "))
    };

    let sort_order = match sort.as_deref() {
        Some("updated-asc") => SortOrder::UpdatedAsc,
        Some("created") => SortOrder::CreatedDesc,
        Some("created-asc") => SortOrder::CreatedAsc,
        _ => SortOrder::UpdatedDesc,
    };

    let params = SearchParams {
        query: query_str,
        type_filter,
        status_filter: status,
        tag_filter: tags,
        sort: sort_order,
        limit,
    };

    let results = search::search(&conn, &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("No fragments found.");
    } else {
        render::print_table(&results, config.id_display_length);
    }

    Ok(())
}
