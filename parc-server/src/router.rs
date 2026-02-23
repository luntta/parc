use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::jsonrpc::RpcError;
use crate::methods;

pub struct Router {
    pub vault_path: PathBuf,
}

impl Router {
    pub fn new(vault_path: PathBuf) -> Self {
        Router { vault_path }
    }

    pub fn dispatch(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let vault = &self.vault_path;

        match method {
            // Fragment methods
            "fragment.create" => methods::fragment::create(vault, params),
            "fragment.get" => methods::fragment::get(vault, params),
            "fragment.update" => methods::fragment::update(vault, params),
            "fragment.delete" => methods::fragment::delete(vault, params),
            "fragment.list" => methods::fragment::list(vault, params),
            "fragment.search" => methods::fragment::search(vault, params),

            // Link methods
            "fragment.link" => methods::link::link(vault, params),
            "fragment.unlink" => methods::link::unlink(vault, params),
            "fragment.backlinks" => methods::link::backlinks(vault, params),

            // Attachment methods
            "fragment.attach" => methods::attachment::attach(vault, params),
            "fragment.detach" => methods::attachment::detach(vault, params),
            "fragment.attachments" => methods::attachment::attachments(vault, params),

            // Vault methods
            "vault.info" => methods::vault::info(vault, params),
            "vault.reindex" => methods::vault::reindex(vault, params),
            "vault.doctor" => methods::vault::doctor(vault, params),

            // Schema methods
            "schema.list" => methods::schema::list(vault, params),
            "schema.get" => methods::schema::get(vault, params),

            // Tags
            "tags.list" => methods::tags::list(vault, params),

            // History
            "history.list" => methods::history::list(vault, params),
            "history.get" => methods::history::get(vault, params),
            "history.restore" => methods::history::restore(vault, params),

            _ => Err(RpcError::method_not_found(method)),
        }
    }
}

pub fn extract_params<T: DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(|e| RpcError::invalid_params(&e.to_string()))
}

pub fn map_parc_error(e: parc_core::ParcError) -> RpcError {
    RpcError::internal_error(&e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_unknown_method() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        parc_core::vault::init_vault(&vault).unwrap();
        let router = Router::new(vault);
        let result = router.dispatch("nonexistent.method", Value::Null);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, -32601);
    }
}
