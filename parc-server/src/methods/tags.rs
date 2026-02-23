use std::path::Path;

use serde_json::Value;

use parc_core::index;
use parc_core::tag;

use crate::jsonrpc::RpcError;
use crate::router::map_parc_error;

pub fn list(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let conn = index::open_index(vault).map_err(map_parc_error)?;
    let tags = tag::aggregate_tags(&conn).map_err(map_parc_error)?;

    let items: Vec<Value> = tags
        .iter()
        .map(|t| {
            serde_json::json!({
                "tag": t.tag,
                "count": t.count,
            })
        })
        .collect();

    Ok(Value::Array(items))
}
