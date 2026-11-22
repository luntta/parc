use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::schema::{load_schemas, FieldType, Schema};

use crate::jsonrpc::RpcError;
use crate::router::{extract_params, map_parc_error};

/// A schema as clients see it. `type` keeps its original debug spelling for
/// existing clients; `kind`, `values` and `toggle` are what a form needs.
fn schema_to_json(s: &Schema) -> Value {
    let fields: Vec<Value> = s
        .fields
        .iter()
        .map(|f| {
            let (kind, values) = match &f.field_type {
                FieldType::String => ("string", None),
                FieldType::Date => ("date", None),
                FieldType::Enum(values) => ("enum", Some(values.clone())),
                FieldType::ListOfStrings => ("list", None),
            };
            serde_json::json!({
                "name": f.name,
                "type": format!("{:?}", f.field_type),
                "kind": kind,
                "values": values,
                "required": f.required,
                "default": f.default,
                "toggle": f.toggle.as_ref().map(|(off, on)| [off, on]),
            })
        })
        .collect();

    serde_json::json!({
        "name": s.name,
        "alias": s.alias,
        "editor_skip": s.editor_skip,
        "fields": fields,
        // The one-key toggle (the TUI's `t`), including the status open/done fallback.
        "toggle": s.toggle().map(|t| serde_json::json!({ "field": t.field, "off": t.off, "on": t.on })),
    })
}

pub fn list(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let registry = load_schemas(vault).map_err(map_parc_error)?;
    Ok(Value::Array(
        registry.list().iter().map(|s| schema_to_json(s)).collect(),
    ))
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

    Ok(schema_to_json(schema))
}
