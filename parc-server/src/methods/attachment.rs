use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::attachment::AttachMode;
use parc_core::fragment;

use crate::jsonrpc::RpcError;
use crate::methods::write;
use crate::router::{extract_params, map_parc_error};

#[derive(Deserialize)]
pub struct AttachParams {
    pub id: String,
    pub path: String,
    #[serde(default)]
    pub mv: bool,
}

pub fn attach(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: AttachParams = extract_params(params)?;
    let source = std::path::Path::new(&p.path);

    if !source.exists() {
        return Err(RpcError::invalid_params(&format!(
            "file not found: {}",
            p.path
        )));
    }
    let size = source.metadata().map(|m| m.len()).unwrap_or(0);
    let mode = if p.mv {
        AttachMode::Move
    } else {
        AttachMode::Copy
    };

    let mut writer = write::writer(vault);
    let (frag, filename) = writer.attach(&p.id, source, mode).map_err(map_parc_error)?;
    let revision = fragment::revision(vault, &frag.id).map_err(map_parc_error)?;

    Ok(write::with_warnings(
        serde_json::json!({
            "id": frag.id,
            "filename": filename,
            "size": size,
            "attachments": frag.attachments,
            "revision": revision,
        }),
        &mut writer,
    ))
}

#[derive(Deserialize)]
pub struct DetachParams {
    pub id: String,
    pub filename: String,
}

pub fn detach(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: DetachParams = extract_params(params)?;
    let mut writer = write::writer(vault);
    let frag = writer.detach(&p.id, &p.filename).map_err(map_parc_error)?;
    let revision = fragment::revision(vault, &frag.id).map_err(map_parc_error)?;

    Ok(write::with_warnings(
        serde_json::json!({
            "id": frag.id,
            "filename": p.filename,
            "detached": true,
            "attachments": frag.attachments,
            "revision": revision,
        }),
        &mut writer,
    ))
}

#[derive(Deserialize)]
pub struct MoveAttachmentParams {
    pub from: String,
    pub to: String,
    pub filename: String,
}

pub fn move_attachment(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: MoveAttachmentParams = extract_params(params)?;
    let mut writer = write::writer(vault);
    let (from_frag, to_frag) = writer
        .move_attachment(&p.from, &p.to, &p.filename)
        .map_err(map_parc_error)?;
    let from_revision = fragment::revision(vault, &from_frag.id).map_err(map_parc_error)?;
    let to_revision = fragment::revision(vault, &to_frag.id).map_err(map_parc_error)?;

    Ok(write::with_warnings(
        serde_json::json!({
            "from": from_frag.id,
            "to": to_frag.id,
            "filename": p.filename,
            "from_attachments": from_frag.attachments,
            "to_attachments": to_frag.attachments,
            "from_revision": from_revision,
            "to_revision": to_revision,
        }),
        &mut writer,
    ))
}

#[derive(Deserialize)]
pub struct AttachmentsParams {
    pub id: String,
}

pub fn attachments(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: AttachmentsParams = extract_params(params)?;
    let conn = parc_core::index::open_index(vault).map_err(map_parc_error)?;
    let infos = parc_core::extract::list_status(&conn, vault, &p.id).map_err(map_parc_error)?;
    serde_json::to_value(infos).map_err(|e| RpcError::internal_error(&e.to_string()))
}

#[derive(Deserialize)]
pub struct AttachmentTextParams {
    pub id: String,
    pub filename: String,
    #[serde(default)]
    pub ready_only: bool,
    pub from: Option<usize>,
    pub to: Option<usize>,
}

pub fn text(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: AttachmentTextParams = extract_params(params)?;
    let full_id = fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;
    let conn = parc_core::index::open_index(vault).map_err(map_parc_error)?;
    if p.ready_only {
        let info = parc_core::extract::list_status(&conn, vault, &full_id)
            .map_err(map_parc_error)?
            .into_iter()
            .find(|a| a.filename == p.filename)
            .ok_or_else(|| RpcError::invalid_params("attachment not found"))?;
        if info.status != "ready" {
            return Ok(serde_json::json!({"id": full_id, "filename": p.filename,
                "status": info.status, "error": info.error, "total_parts": 0, "parts": []}));
        }
    }
    let extracted = parc_core::extract::read_parts(vault, &conn, &full_id, &p.filename)
        .map_err(map_parc_error)?;
    let Some(ext) = extracted else {
        return Ok(serde_json::json!({
            "id": full_id,
            "filename": p.filename,
            "status": "no_text",
            "total_parts": 0,
            "parts": [],
        }));
    };
    let total = ext.parts.len();
    if total == 0 {
        return Ok(serde_json::json!({
            "id": full_id,
            "filename": p.filename,
            "title": ext.title,
            "unit": ext.unit,
            "format": ext.format,
            "status": "ready",
            "total_parts": 0,
            "from": 0,
            "to": 0,
            "parts": [],
        }));
    }
    let from = p.from.unwrap_or(1).max(1).min(total);
    let to = p.to.unwrap_or(total).max(from).min(total);
    let parts: Vec<Value> = (from..=to)
        .filter_map(|part| {
            let text = ext.parts.get(part - 1)?;
            Some(serde_json::json!({
                "part": part,
                "label": ext.unit.format_ref(part, Some(text)),
                "text": text,
            }))
        })
        .collect();
    Ok(serde_json::json!({
        "id": full_id,
        "filename": p.filename,
        "title": ext.title,
        "unit": ext.unit,
        "format": ext.format,
        "status": "ready",
        "total_parts": total,
        "from": from,
        "to": to,
        "parts": parts,
    }))
}

#[derive(Deserialize)]
pub struct AttachmentLocateParams {
    pub id: String,
    pub filename: String,
    pub quote: String,
    pub part_hint: Option<usize>,
}

pub fn locate(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: AttachmentLocateParams = extract_params(params)?;
    let full_id = fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;
    let conn = parc_core::index::open_index(vault).map_err(map_parc_error)?;
    let found =
        parc_core::extract::locate(vault, &conn, &full_id, &p.filename, &p.quote, p.part_hint)
            .map_err(map_parc_error)?;
    match found {
        Some(loc) => Ok(serde_json::json!({
            "found": true,
            "part": loc.part,
            "offset": loc.offset,
            "length": loc.length,
        })),
        None => Ok(serde_json::json!({ "found": false })),
    }
}
