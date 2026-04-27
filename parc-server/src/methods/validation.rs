use std::path::Path;

use parc_core::fragment::Fragment;

use crate::jsonrpc::RpcError;

pub fn validate_fragment_for_write(vault: &Path, fragment: &Fragment) -> Result<(), RpcError> {
    parc_core::fragment::validate_fragment_in_vault(vault, fragment)
        .map_err(|e| RpcError::invalid_params(&e.to_string()))
}
