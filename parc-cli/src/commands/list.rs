use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::index::open_index;
use parc_core::search::{self, SearchParams, SortOrder};

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

    let params = SearchParams {
        query: None,
        type_filter: type_name,
        status_filter: status,
        tag_filter: tags,
        sort: SortOrder::UpdatedDesc,
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
