use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;
use chrono::{Duration, Local};
use parc_core::index::open_index;
use parc_core::schema::load_schemas;
use parc_core::search::{self, Filter, SearchQuery, SearchResult};

pub fn today_string() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

pub fn days_ago_string(days: u64) -> String {
    (Local::now().date_naive() - Duration::days(days as i64))
        .format("%Y-%m-%d")
        .to_string()
}

pub fn in_days_string(days: i64) -> String {
    (Local::now().date_naive() + Duration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

pub fn run_search(vault: &Path, query: &SearchQuery) -> Result<Vec<SearchResult>> {
    let conn = open_index(vault)?;
    Ok(search::search(&conn, query)?)
}

pub fn merge_unique(groups: Vec<Vec<SearchResult>>) -> Vec<SearchResult> {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();

    for group in groups {
        for result in group {
            if seen.insert(result.id.clone()) {
                merged.push(result);
            }
        }
    }

    merged
}

pub fn cap_results(results: Vec<SearchResult>, limit: usize) -> (Vec<SearchResult>, usize) {
    let total = results.len();
    (results.into_iter().take(limit).collect(), total)
}

/// Filters that leave out settled work: every status some type's schema
/// counts as done or closed (see `SchemaRegistry::settled_statuses`).
pub fn unfinished_status_filters(vault: &Path) -> Vec<Filter> {
    let settled = load_schemas(vault)
        .map(|schemas| schemas.settled_statuses())
        .unwrap_or_else(|_| {
            ["done", "cancelled", "resolved", "accepted", "discarded"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        });
    settled
        .into_iter()
        .map(|status| Filter::Status {
            value: status,
            negated: true,
        })
        .collect()
}
