use chrono::Utc;
use serde::Deserialize;
use tauri::State;

use parc_core::fragment::{read_fragment, resolve_id, write_fragment};
use parc_core::index;

use crate::dto::BacklinkDto;
use crate::error::GuiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LinkParams {
    pub id_a: String,
    pub id_b: String,
}

#[tauri::command]
pub fn link_fragments(
    state: State<'_, AppState>,
    params: LinkParams,
) -> Result<Vec<String>, GuiError> {
    let vault = state.vault_path();

    let mut frag_a = read_fragment(&vault, &params.id_a)?;
    let mut frag_b = read_fragment(&vault, &params.id_b)?;

    if frag_a.id == frag_b.id {
        return Err(GuiError::Other("cannot link a fragment to itself".into()));
    }

    if !frag_a.links.contains(&frag_b.id) {
        frag_a.links.push(frag_b.id.clone());
        frag_a.updated_at = Utc::now();
        write_fragment(&vault, &frag_a)?;
    }

    if !frag_b.links.contains(&frag_a.id) {
        frag_b.links.push(frag_a.id.clone());
        frag_b.updated_at = Utc::now();
        write_fragment(&vault, &frag_b)?;
    }

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &frag_a, &vault)?;
    index::index_fragment_auto(&conn, &frag_b, &vault)?;

    Ok(vec![frag_a.id, frag_b.id])
}

#[tauri::command]
pub fn unlink_fragments(
    state: State<'_, AppState>,
    params: LinkParams,
) -> Result<Vec<String>, GuiError> {
    let vault = state.vault_path();

    let mut frag_a = read_fragment(&vault, &params.id_a)?;
    let mut frag_b = read_fragment(&vault, &params.id_b)?;

    if frag_a.links.contains(&frag_b.id) {
        frag_a.links.retain(|l| l != &frag_b.id);
        frag_a.updated_at = Utc::now();
        write_fragment(&vault, &frag_a)?;
    }

    if frag_b.links.contains(&frag_a.id) {
        frag_b.links.retain(|l| l != &frag_a.id);
        frag_b.updated_at = Utc::now();
        write_fragment(&vault, &frag_b)?;
    }

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &frag_a, &vault)?;
    index::index_fragment_auto(&conn, &frag_b, &vault)?;

    Ok(vec![frag_a.id, frag_b.id])
}

#[tauri::command]
pub fn get_backlinks(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<BacklinkDto>, GuiError> {
    let vault = state.vault_path();
    let full_id = resolve_id(&vault, &id)?;

    let conn = index::open_index(&vault)?;
    let links = index::get_backlinks(&conn, &full_id)?;

    Ok(links
        .into_iter()
        .map(|bl| BacklinkDto {
            id: bl.source_id,
            fragment_type: bl.source_type,
            title: bl.source_title,
        })
        .collect())
}
