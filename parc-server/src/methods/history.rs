use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::fragment;
use parc_core::history as hist;
use parc_core::index;

use crate::jsonrpc::RpcError;
use crate::router::{extract_params, map_parc_error};

#[derive(Deserialize)]
pub struct ListParams {
    pub id: String,
}

pub fn list(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: ListParams = extract_params(params)?;
    let full_id = fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;
    let versions = hist::list_versions(vault, &full_id).map_err(map_parc_error)?;

    let items: Vec<Value> = versions
        .iter()
        .map(|v| {
            serde_json::json!({
                "timestamp": v.timestamp,
                "size": v.size,
            })
        })
        .collect();

    Ok(Value::Array(items))
}

#[derive(Deserialize)]
pub struct GetParams {
    pub id: String,
    pub timestamp: String,
}

pub fn get(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    let full_id = fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;
    let frag = hist::read_version(vault, &full_id, &p.timestamp).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "id": frag.id,
        "type": frag.fragment_type,
        "title": frag.title,
        "tags": frag.tags,
        "body": frag.body,
        "created_at": frag.created_at.to_rfc3339(),
        "updated_at": frag.updated_at.to_rfc3339(),
    }))
}

#[derive(Deserialize)]
pub struct RestoreParams {
    pub id: String,
    pub timestamp: String,
}

pub fn restore(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: RestoreParams = extract_params(params)?;
    let full_id = fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;
    let frag = hist::restore_version(vault, &full_id, &p.timestamp).map_err(map_parc_error)?;

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    index::index_fragment_auto(&conn, &frag, vault).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "id": frag.id,
        "type": frag.fragment_type,
        "title": frag.title,
        "restored_from": p.timestamp,
    }))
}
