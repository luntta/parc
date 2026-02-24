use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;

use parc_core::{doctor, index, vault};

use crate::dto::{DoctorFindingDto, DoctorReportDto, VaultInfoDto};
use crate::error::GuiError;
use crate::state::AppState;

#[tauri::command]
pub fn vault_info(state: State<'_, AppState>) -> Result<VaultInfoDto, GuiError> {
    let vault_path = state.vault_path();
    let info = vault::vault_info(&vault_path)?;

    Ok(VaultInfoDto {
        path: info.path.to_string_lossy().to_string(),
        scope: info.scope.to_string(),
        fragment_count: info.fragment_count,
    })
}

#[tauri::command]
pub fn reindex(state: State<'_, AppState>) -> Result<usize, GuiError> {
    let vault_path = state.vault_path();
    let count = index::reindex(&vault_path)?;
    Ok(count)
}

#[tauri::command]
pub fn doctor(state: State<'_, AppState>) -> Result<DoctorReportDto, GuiError> {
    let vault_path = state.vault_path();
    let report = doctor::run_doctor(&vault_path)?;

    let findings = report
        .findings
        .iter()
        .map(|f| {
            let (finding_type, details) = match f {
                doctor::DoctorFinding::BrokenLink {
                    source_id,
                    source_title,
                    target_ref,
                } => {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("source_id".into(), serde_json::json!(source_id));
                    d.insert("source_title".into(), serde_json::json!(source_title));
                    d.insert("target_ref".into(), serde_json::json!(target_ref));
                    ("broken_link".into(), d)
                }
                doctor::DoctorFinding::OrphanFragment { id, title } => {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("id".into(), serde_json::json!(id));
                    d.insert("title".into(), serde_json::json!(title));
                    ("orphan".into(), d)
                }
                doctor::DoctorFinding::SchemaViolation { id, title, message } => {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("id".into(), serde_json::json!(id));
                    d.insert("title".into(), serde_json::json!(title));
                    d.insert("message".into(), serde_json::json!(message));
                    ("schema_violation".into(), d)
                }
                doctor::DoctorFinding::AttachmentMismatch {
                    fragment_id,
                    detail,
                } => {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("fragment_id".into(), serde_json::json!(fragment_id));
                    d.insert("detail".into(), serde_json::json!(detail));
                    ("attachment_mismatch".into(), d)
                }
                doctor::DoctorFinding::VaultSizeWarning { total_bytes } => {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("total_bytes".into(), serde_json::json!(total_bytes));
                    ("vault_size_warning".into(), d)
                }
                doctor::DoctorFinding::PluginIssue {
                    plugin_name,
                    detail,
                } => {
                    let mut d = std::collections::BTreeMap::new();
                    d.insert("plugin_name".into(), serde_json::json!(plugin_name));
                    d.insert("detail".into(), serde_json::json!(detail));
                    ("plugin_issue".into(), d)
                }
            };
            DoctorFindingDto {
                finding_type,
                details,
            }
        })
        .collect();

    Ok(DoctorReportDto {
        fragments_checked: report.fragments_checked,
        healthy: report.is_healthy(),
        findings,
    })
}

#[derive(Debug, Deserialize)]
pub struct SwitchVaultParams {
    pub path: String,
}

#[tauri::command]
pub fn switch_vault(
    state: State<'_, AppState>,
    params: SwitchVaultParams,
) -> Result<VaultInfoDto, GuiError> {
    let path = PathBuf::from(&params.path);
    let resolved = vault::resolve_vault(Some(&path))?;
    state.set_vault_path(resolved.clone());

    let info = vault::vault_info(&resolved)?;
    Ok(VaultInfoDto {
        path: info.path.to_string_lossy().to_string(),
        scope: info.scope.to_string(),
        fragment_count: info.fragment_count,
    })
}

#[tauri::command]
pub fn list_vaults() -> Result<Vec<VaultInfoDto>, GuiError> {
    let vaults = vault::discover_all_vaults()?;
    Ok(vaults
        .into_iter()
        .map(|info| VaultInfoDto {
            path: info.path.to_string_lossy().to_string(),
            scope: info.scope.to_string(),
            fragment_count: info.fragment_count,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
pub struct InitVaultParams {
    pub path: String,
}

#[tauri::command]
pub fn init_vault(
    state: State<'_, AppState>,
    params: InitVaultParams,
) -> Result<VaultInfoDto, GuiError> {
    let path = PathBuf::from(&params.path);
    let vault_path = if path.ends_with(".parc") {
        path
    } else {
        path.join(".parc")
    };

    vault::init_vault(&vault_path)?;
    state.set_vault_path(vault_path.clone());

    let info = vault::vault_info(&vault_path)?;
    Ok(VaultInfoDto {
        path: info.path.to_string_lossy().to_string(),
        scope: info.scope.to_string(),
        fragment_count: info.fragment_count,
    })
}
