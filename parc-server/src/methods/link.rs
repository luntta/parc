use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::index;

use crate::jsonrpc::RpcError;
use crate::methods::write;
use crate::router::{extract_params, map_parc_error};

#[derive(Deserialize)]
pub struct LinkParams {
    pub id_a: String,
    pub id_b: String,
}

pub fn link(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: LinkParams = extract_params(params)?;
    let mut writer = write::writer(vault);
    let change = writer.link(&p.id_a, &p.id_b).map_err(map_parc_error)?;
    let result = serde_json::json!({
        "linked": [change.a, change.b],
    });
    Ok(write::with_warnings(result, &mut writer))
}

pub fn unlink(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: LinkParams = extract_params(params)?;
    let mut writer = write::writer(vault);
    let change = writer.unlink(&p.id_a, &p.id_b).map_err(map_parc_error)?;
    let result = serde_json::json!({
        "unlinked": [change.a, change.b],
    });
    Ok(write::with_warnings(result, &mut writer))
}

#[derive(Deserialize)]
pub struct BacklinksParams {
    pub id: String,
}

pub fn backlinks(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: BacklinksParams = extract_params(params)?;
    let full_id = parc_core::fragment::resolve_id(vault, &p.id).map_err(map_parc_error)?;

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    let links = index::get_backlinks(&conn, &full_id).map_err(map_parc_error)?;

    let items: Vec<Value> = links
        .iter()
        .map(|bl| {
            serde_json::json!({
                "id": bl.source_id,
                "type": bl.source_type,
                "title": bl.source_title,
            })
        })
        .collect();

    Ok(Value::Array(items))
}
