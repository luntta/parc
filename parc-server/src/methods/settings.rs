//! Vault settings: what config.yml holds for everyone who uses the vault.
//! Today that is which types are on (`types:`); hook and plugin switches are
//! deliberately not reachable from here.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::config::{
    load_config, save_attachments, save_types, AttachmentTextMode, AttachmentsConfig,
};
use parc_core::schema::load_schemas;

use crate::jsonrpc::RpcError;
use crate::router::{extract_params, map_parc_error};

/// `{ "types": { "default", "disabled" }, "attachments": { "text" } }`.
pub fn get(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let config = load_config(vault).map_err(map_parc_error)?;
    let types = config.types;
    let registry = load_schemas(vault).map_err(map_parc_error)?;
    let default = types.default_type(&registry).map(|s| s.name.clone());
    Ok(serde_json::json!({
        "types": { "default": default, "disabled": types.disabled },
        "attachments": { "text": config.attachments.text },
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypesInput {
    pub default: Option<String>,
    pub disabled: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentsInput {
    pub text: Option<AttachmentTextMode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    pub types: Option<TypesInput>,
    pub attachments: Option<AttachmentsInput>,
}

/// Change the settings given and keep the rest. Returns what `settings.get` does.
pub fn update(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: UpdateParams = extract_params(params)?;
    if let Some(input) = p.types {
        let mut types = load_config(vault).map_err(map_parc_error)?.types;
        if let Some(default) = input.default {
            types.default = Some(default);
        }
        if let Some(disabled) = input.disabled {
            if disabled.len() > 256 {
                return Err(RpcError::invalid_params("too many types"));
            }
            types.disabled = disabled;
        }
        save_types(vault, &types).map_err(map_parc_error)?;
    }
    if let Some(input) = p.attachments {
        if let Some(text) = input.text {
            save_attachments(vault, &AttachmentsConfig { text }).map_err(map_parc_error)?;
        }
    }
    get(vault, Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_changes_only_what_it_names() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        parc_core::vault::init_vault(&vault).unwrap();
        let got = update(
            &vault,
            serde_json::json!({ "types": { "disabled": ["risk"] } }),
        )
        .unwrap();
        assert_eq!(got["types"]["default"], "note");
        assert_eq!(got["types"]["disabled"], serde_json::json!(["risk"]));
        let got = update(
            &vault,
            serde_json::json!({ "types": { "default": "todo" } }),
        )
        .unwrap();
        assert_eq!(got["types"]["disabled"], serde_json::json!(["risk"]));
        assert_eq!(got["types"]["default"], "todo");
        assert!(update(
            &vault,
            serde_json::json!({ "types": { "disabled": ["todo"] } })
        )
        .is_err());
        assert!(update(&vault, serde_json::json!({ "hooks": { "enabled": true } })).is_err());
    }
}
