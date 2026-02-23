use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::schema::load_schemas;

use crate::jsonrpc::RpcError;
use crate::router::{extract_params, map_parc_error};

pub fn list(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let registry = load_schemas(vault).map_err(map_parc_error)?;
    let schemas = registry.list();

    let items: Vec<Value> = schemas
        .iter()
        .map(|s| {
            let fields: Vec<Value> = s
                .fields
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "name": f.name,
                        "type": format!("{:?}", f.field_type),
                        "required": f.required,
                        "default": f.default,
                    })
                })
                .collect();

            serde_json::json!({
                "name": s.name,
                "alias": s.alias,
                "fields": fields,
            })
        })
        .collect();

    Ok(Value::Array(items))
}

#[derive(Deserialize)]
pub struct GetParams {
    #[serde(rename = "type")]
    pub type_name: String,
}

pub fn get(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    let registry = load_schemas(vault).map_err(map_parc_error)?;

    let schema = registry
        .resolve(&p.type_name)
        .ok_or_else(|| RpcError::invalid_params(&format!("unknown type: {}", p.type_name)))?;

    let fields: Vec<Value> = schema
        .fields
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name,
                "type": format!("{:?}", f.field_type),
                "required": f.required,
                "default": f.default,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "name": schema.name,
        "alias": schema.alias,
        "editor_skip": schema.editor_skip,
        "fields": fields,
    }))
}
