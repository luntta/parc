use serde::Deserialize;
use tauri::State;

use parc_core::index;
use parc_core::search;

use crate::dto::SearchResultDto;
use crate::error::GuiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub query: String,
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn search_fragments(
    state: State<'_, AppState>,
    params: SearchParams,
) -> Result<Vec<SearchResultDto>, GuiError> {
    let vault = state.vault_path();

    let mut query = search::parse_query(&params.query)?;
    if let Some(limit) = params.limit {
        query.limit = Some(limit);
    }

    let conn = index::open_index(&vault)?;
    let results = search::search(&conn, &query)?;

    Ok(results
        .into_iter()
        .map(|r| SearchResultDto {
            id: r.id,
            fragment_type: r.fragment_type,
            title: r.title,
            status: r.status,
            tags: r.tags,
            updated_at: r.updated_at,
            snippet: r.snippet,
        })
        .collect())
}
