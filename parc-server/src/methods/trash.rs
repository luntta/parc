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

/// Permanently delete one trashed fragment (`id`) or empty the whole trash.
pub fn purge(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: PurgeParams = if params.is_null() {
        PurgeParams::default()
    } else {
        extract_params(params)?
    };
    let trash_dir = vault.join("trash");

    if let Some(id) = p.id {
        fragment::validate_id(&id).map_err(map_parc_error)?;
        let path = trash_dir.join(format!("{}.md", id));
        let meta = std::fs::symlink_metadata(&path)
            .map_err(|_| RpcError::invalid_params(&format!("fragment '{}' not found in trash", id)))?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(RpcError::invalid_params("refusing to purge non-file trash entry"));
        }
        std::fs::remove_file(&path).map_err(|e| RpcError::internal_error(&e.to_string()))?;
        return Ok(serde_json::json!({ "purged": id }));
    }

    let mut count = 0usize;
    if trash_dir.is_dir() {
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
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                if !meta.file_type().is_symlink() && meta.is_file() {
                    std::fs::remove_file(&path)
                        .map_err(|e| RpcError::internal_error(&e.to_string()))?;
                    count += 1;
                }
            }
        }
    }

    Ok(serde_json::json!({ "purged_count": count }))
}
