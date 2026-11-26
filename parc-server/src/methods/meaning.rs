use std::path::Path;

use serde_json::{json, Value};

use parc_core::{embed, index};

use crate::jsonrpc::RpcError;
use crate::router::map_parc_error;

/// `meaning.status`: the meaning model recall can use, where its files go and
/// whether they are there, and how much of this vault it has read. Each file
/// comes with its pinned URL, size and SHA-256, for a client that downloads
/// it (the desktop app, on request). Never loads the model.
pub fn status(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let spec = embed::MODEL;
    let dir = embed::model_dir(spec);
    let installed = dir.as_deref().is_some_and(|d| embed::is_installed(spec, d));
    let conn = index::open_index(vault).map_err(map_parc_error)?;
    let coverage = embed::coverage(&conn, spec).map_err(map_parc_error)?;
    let files: Vec<Value> = spec
        .files
        .iter()
        .map(|f| json!({ "name": f.name, "size": f.size, "sha256": f.sha256, "url": spec.url(f) }))
        .collect();
    Ok(json!({
        "model": {
            "id": spec.id,
            "key": spec.key(),
            "label": spec.label,
            "repo": spec.repo,
            "revision": spec.revision,
            "license": spec.license,
            "size": spec.size(),
            "files": files,
        },
        "dir": dir,
        "installed": installed,
        "loaded": embed::is_loaded(),
        "error": embed::load_error(),
        "coverage": coverage,
    }))
}
