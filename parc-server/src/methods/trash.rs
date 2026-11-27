use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::fragment;

use crate::jsonrpc::RpcError;
use crate::methods::{fragment::saved_json, write};
use crate::router::{extract_params, map_parc_error};

/// List soft-deleted fragments in `<vault>/trash/`, newest first.
pub fn list(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let trash_dir = vault.join("trash");
    if !trash_dir.is_dir() {
        return Ok(Value::Array(Vec::new()));
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&trash_dir)
        .map_err(|e| RpcError::internal_error(&e.to_string()))?
    {
        let entry = entry.map_err(|e| RpcError::internal_error(&e.to_string()))?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if fragment::validate_id(stem).is_err() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(frag) = fragment::parse_fragment(&content) {
                entries.push(frag);
            }
        }
    }

    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(Value::Array(
        entries
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "type": f.fragment_type,
                    "title": f.title,
                    "tags": f.tags,
                    "status": f.extra_fields.get("status").and_then(Value::as_str),
                    "created_at": f.created_at.to_rfc3339(),
                    "updated_at": f.updated_at.to_rfc3339(),
                })
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct RestoreParams {
    pub id: String,
}

/// Move a fragment out of the trash (the undo of `fragment.delete`).
pub fn restore(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: RestoreParams = extract_params(params)?;
    let mut writer = write::writer(vault);
    let frag = writer.restore_from_trash(&p.id).map_err(map_parc_error)?;
    saved_json(vault, &frag)
}

#[derive(Deserialize, Default)]
pub struct PurgeParams {
    pub id: Option<String>,
}

/// Permanently delete one trashed fragment (`id`) or empty the whole trash,
/// removing its attachments and history directories as well.
pub fn purge(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: PurgeParams = if params.is_null() {
        PurgeParams::default()
    } else {
        extract_params(params)?
    };
    let mut writer = write::writer(vault);

    if let Some(id) = p.id {
        let purged = writer.purge(&id).map_err(|e| match e {
            parc_core::ParcError::FragmentNotFound(msg) => RpcError::invalid_params(&msg),
            other => map_parc_error(other),
        })?;
        return Ok(serde_json::json!({ "purged": purged }));
    }

    let count = writer.empty_trash().map_err(map_parc_error)?;
    Ok(serde_json::json!({ "purged_count": count }))
}
