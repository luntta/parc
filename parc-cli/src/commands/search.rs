use std::io::IsTerminal;
use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::fuzzy::FuzzyHit;
use parc_core::index::open_index;
use parc_core::search::{self, parse_query, SearchResult, SortOrder};

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
        Some("updated") => SortOrder::UpdatedDesc,
        Some("updated-asc") => SortOrder::UpdatedAsc,
        Some("created") => SortOrder::CreatedDesc,
        Some("created-asc") => SortOrder::CreatedAsc,
        Some("random") => SortOrder::Random,
        Some("score") | None => SortOrder::Score,
        Some(other) => anyhow::bail!("unknown sort '{}'", other),
    };

    search_query.limit = limit;

    let hits = search::fuzzy_search(&conn, &search_query)?;

    if json {
        let results: Vec<SearchResult> = hits.iter().map(hit_to_result).collect();
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if hits.is_empty() {
        println!("No fragments found.");
    } else {
        let highlight = std::io::stdout().is_terminal();
        render::print_fuzzy_table(&hits, config.id_display_length, highlight);
    }

    Ok(())
}

fn hit_to_result(hit: &FuzzyHit) -> SearchResult {
    SearchResult {
        id: hit.item.id.clone(),
        fragment_type: hit.item.fragment_type.clone(),
        title: hit.item.title.clone(),
        status: hit.item.status.clone(),
        tags: hit.item.tags.clone(),
        updated_at: hit.item.updated_at.clone(),
        snippet: None,
    }
}
