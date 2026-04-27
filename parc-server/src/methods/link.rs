use std::path::Path;

use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

use parc_core::fragment::{read_fragment, write_fragment};
use parc_core::index;

use crate::jsonrpc::RpcError;
use crate::methods::validation::validate_fragment_for_write;
use crate::router::{extract_params, map_parc_error};

#[derive(Deserialize)]
pub struct LinkParams {
    pub id_a: String,
    pub id_b: String,
}

pub fn link(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: LinkParams = extract_params(params)?;

    let mut frag_a = read_fragment(vault, &p.id_a).map_err(map_parc_error)?;
    let mut frag_b = read_fragment(vault, &p.id_b).map_err(map_parc_error)?;

    if frag_a.id == frag_b.id {
        return Err(RpcError::invalid_params("cannot link a fragment to itself"));
    }

    let a_changed = !frag_a.links.contains(&frag_b.id);
    let b_changed = !frag_b.links.contains(&frag_a.id);

    if a_changed {
        frag_a.links.push(frag_b.id.clone());
        frag_a.updated_at = Utc::now();
    }

    if b_changed {
        frag_b.links.push(frag_a.id.clone());
        frag_b.updated_at = Utc::now();
    }

    if a_changed {
        validate_fragment_for_write(vault, &frag_a)?;
    }
    if b_changed {
        validate_fragment_for_write(vault, &frag_b)?;
    }

    if a_changed {
        write_fragment(vault, &frag_a).map_err(map_parc_error)?;
    }
    if b_changed {
        write_fragment(vault, &frag_b).map_err(map_parc_error)?;
    }

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    index::index_fragment_auto(&conn, &frag_a, vault).map_err(map_parc_error)?;
    index::index_fragment_auto(&conn, &frag_b, vault).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "linked": [frag_a.id, frag_b.id],
    }))
}

pub fn unlink(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: LinkParams = extract_params(params)?;

    let mut frag_a = read_fragment(vault, &p.id_a).map_err(map_parc_error)?;
    let mut frag_b = read_fragment(vault, &p.id_b).map_err(map_parc_error)?;

    let a_had_b = frag_a.links.contains(&frag_b.id);
    let b_had_a = frag_b.links.contains(&frag_a.id);

    if a_had_b {
        frag_a.links.retain(|l| l != &frag_b.id);
        frag_a.updated_at = Utc::now();
    }

    if b_had_a {
        frag_b.links.retain(|l| l != &frag_a.id);
        frag_b.updated_at = Utc::now();
    }

    if a_had_b {
        validate_fragment_for_write(vault, &frag_a)?;
    }
    if b_had_a {
        validate_fragment_for_write(vault, &frag_b)?;
    }

    if a_had_b {
        write_fragment(vault, &frag_a).map_err(map_parc_error)?;
    }
    if b_had_a {
        write_fragment(vault, &frag_b).map_err(map_parc_error)?;
    }

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    index::index_fragment_auto(&conn, &frag_a, vault).map_err(map_parc_error)?;
    index::index_fragment_auto(&conn, &frag_b, vault).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "unlinked": [frag_a.id, frag_b.id],
    }))
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
