use std::path::Path;

use serde::Deserialize;
use tauri::State;

use parc_core::attachment as att;
use parc_core::fragment;

use crate::dto::AttachmentInfoDto;
use crate::error::GuiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AttachParams {
    pub id: String,
    pub path: String,
}

#[tauri::command]
pub fn attach_file(
    state: State<'_, AppState>,
    params: AttachParams,
) -> Result<AttachmentInfoDto, GuiError> {
    let vault = state.vault_path();
    let source = Path::new(&params.path);

    if !source.exists() {
        return Err(GuiError::Other(format!("file not found: {}", params.path)));
    }

    let size = source.metadata().map(|m| m.len()).unwrap_or(0);
    let filename = att::attach_file(&vault, &params.id, source, false)?;
    let full_id = fragment::resolve_id(&vault, &params.id)?;

    let attach_path = vault
        .join("attachments")
        .join(&full_id)
        .join(&filename);

    Ok(AttachmentInfoDto {
        filename,
        size,
        path: attach_path.to_string_lossy().to_string(),
    })
}

#[derive(Debug, Deserialize)]
pub struct DetachParams {
    pub id: String,
    pub filename: String,
}

#[tauri::command]
pub fn detach_file(
    state: State<'_, AppState>,
    params: DetachParams,
) -> Result<bool, GuiError> {
    let vault = state.vault_path();
    att::detach_file(&vault, &params.id, &params.filename)?;
    Ok(true)
}

#[tauri::command]
pub fn list_attachments(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<AttachmentInfoDto>, GuiError> {
    let vault = state.vault_path();
    let infos = att::list_attachments(&vault, &id)?;

    Ok(infos
        .into_iter()
        .map(|a| AttachmentInfoDto {
            filename: a.filename,
            size: a.size,
            path: a.path.to_string_lossy().to_string(),
        })
        .collect())
}

#[tauri::command]
pub fn get_attachment_path(
    state: State<'_, AppState>,
    id: String,
    filename: String,
) -> Result<String, GuiError> {
    let vault = state.vault_path();
    let full_id = fragment::resolve_id(&vault, &id)?;
    let path = vault
        .join("attachments")
        .join(&full_id)
        .join(&filename);

    if !path.exists() {
        return Err(GuiError::Other(format!(
            "attachment '{}' not found",
            filename
        )));
    }

    Ok(path.to_string_lossy().to_string())
}
