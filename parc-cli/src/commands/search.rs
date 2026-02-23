use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::index::open_index;
use parc_core::search::{self, parse_query, SortOrder};

use crate::render;

pub fn run(
    vault: &Path,
    query: Vec<String>,
    json: bool,
    sort: Option<String>,
    limit: Option<usize>,
) -> Result<()> {
    let config = load_config(vault)?;
    let conn = open_index(vault)?;

    let query_str = query.join(" ");
    let mut search_query = parse_query(&query_str)?;

    search_query.sort = match sort.as_deref() {
        Some("updated-asc") => SortOrder::UpdatedAsc,
        Some("created") => SortOrder::CreatedDesc,
        Some("created-asc") => SortOrder::CreatedAsc,
        _ => SortOrder::UpdatedDesc,
    };

    search_query.limit = limit;

    let results = search::search(&conn, &search_query)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("No fragments found.");
    } else {
        render::print_table(&results, config.id_display_length);
    }

    Ok(())
}
