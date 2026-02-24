use serde::Deserialize;
use tauri::State;

use parc_core::fragment;
use parc_core::history as hist;
use parc_core::index;

use crate::dto::{DiffDto, FragmentDto, VersionEntryDto};
use crate::error::GuiError;
use crate::state::AppState;

#[tauri::command]
pub fn list_versions(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<VersionEntryDto>, GuiError> {
    let vault = state.vault_path();
    let full_id = fragment::resolve_id(&vault, &id)?;
    let versions = hist::list_versions(&vault, &full_id)?;

    Ok(versions
        .into_iter()
        .map(|v| VersionEntryDto {
            timestamp: v.timestamp,
            size: v.size,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct GetVersionParams {
    pub id: String,
    pub timestamp: String,
}

#[tauri::command]
pub fn get_version(
    state: State<'_, AppState>,
    params: GetVersionParams,
) -> Result<FragmentDto, GuiError> {
    let vault = state.vault_path();
    let full_id = fragment::resolve_id(&vault, &params.id)?;
    let frag = hist::read_version(&vault, &full_id, &params.timestamp)?;
    Ok(FragmentDto::from(&frag))
}

#[derive(Debug, Deserialize)]
pub struct RestoreVersionParams {
    pub id: String,
    pub timestamp: String,
}

#[tauri::command]
pub fn restore_version(
    state: State<'_, AppState>,
    params: RestoreVersionParams,
) -> Result<FragmentDto, GuiError> {
    let vault = state.vault_path();
    let full_id = fragment::resolve_id(&vault, &params.id)?;
    let frag = hist::restore_version(&vault, &full_id, &params.timestamp)?;

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &frag, &vault)?;

    Ok(FragmentDto::from(&frag))
}

#[derive(Debug, Deserialize)]
pub struct DiffVersionsParams {
    pub id: String,
    pub timestamp: Option<String>,
}

#[tauri::command]
pub fn diff_versions(
    state: State<'_, AppState>,
    params: DiffVersionsParams,
) -> Result<DiffDto, GuiError> {
    let vault = state.vault_path();
    let diff = hist::diff_versions(
        &vault,
        &params.id,
        params.timestamp.as_deref(),
    )?;
    Ok(DiffDto { diff })
}
