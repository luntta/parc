use tauri::State;

use parc_core::index;
use parc_core::tag;

use crate::dto::TagCountDto;
use crate::error::GuiError;
use crate::state::AppState;

#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<Vec<TagCountDto>, GuiError> {
    let vault = state.vault_path();
    let conn = index::open_index(&vault)?;
    let tags = tag::aggregate_tags(&conn)?;

    Ok(tags
        .into_iter()
        .map(|t| TagCountDto {
            tag: t.tag,
            count: t.count,
        })
        .collect())
}
