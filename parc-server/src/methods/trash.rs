use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::jsonrpc::RpcError;
use crate::methods::{fragment::saved_json, write};
use crate::router::{extract_params, map_parc_error};

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
