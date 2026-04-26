use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::attachment as att;
use parc_core::fragment;

use crate::jsonrpc::RpcError;
use crate::router::{extract_params, map_parc_error};

#[derive(Deserialize)]
pub struct AttachParams {
    pub id: String,
    pub path: String,
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

    let filename = att::attach_file(vault, &p.id, source, false).map_err(map_parc_error)?;
    let full_id = fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;

    let size = source.metadata().map(|m| m.len()).unwrap_or(0);

    Ok(serde_json::json!({
        "id": full_id,
        "filename": filename,
        "size": size,
    }))
}

#[derive(Deserialize)]
pub struct DetachParams {
    pub id: String,
    pub filename: String,
}

pub fn detach(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: DetachParams = extract_params(params)?;
    let full_id = fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;
    att::detach_file(vault, &p.id, &p.filename).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "id": full_id,
        "filename": p.filename,
        "detached": true,
    }))
}

#[derive(Deserialize)]
pub struct AttachmentsParams {
    pub id: String,
}

pub fn attachments(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: AttachmentsParams = extract_params(params)?;
    let infos = att::list_attachments(vault, &p.id).map_err(map_parc_error)?;

    let items: Vec<Value> = infos
        .iter()
        .map(|a| {
            serde_json::json!({
                "filename": a.filename,
                "size": a.size,
            })
        })
        .collect();

    Ok(Value::Array(items))
}
