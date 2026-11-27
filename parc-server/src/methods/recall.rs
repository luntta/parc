use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::recall::{self, RecallQuery, Seed};
use parc_core::{embed, fragment, index};

use crate::jsonrpc::RpcError;
use crate::router::{extract_params, map_parc_error};

/// `recall.context`: the fragments related to a saved fragment (`id`) or to
/// unsaved text (`text`, with `tags` and, for edits to a saved fragment, its
/// `id`). Cheap enough to call while the user types. Uses the meaning model
/// when it is installed (a model that fails to load is reported by
/// `meaning.status`; recall goes on without it).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextParams {
    /// A fragment ID or unique prefix.
    pub id: Option<String>,
    /// Draft title and body together.
    pub text: Option<String>,
    /// The draft's tags (its inline #tags count without being listed).
    #[serde(default)]
    pub tags: Vec<String>,
    pub limit: Option<usize>,
}

pub fn context(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let conn = index::open_index(vault).map_err(map_parc_error)?;
    context_on(vault, &conn, params)
}

pub(crate) fn context_on(
    vault: &Path,
    conn: &index::Connection,
    params: Value,
) -> Result<Value, RpcError> {
    let p: ContextParams = extract_params(params)?;
    if p.limit == Some(0) {
        return Err(RpcError::invalid_params("limit must be at least 1"));
    }
    let id =
        p.id.as_deref()
            .map(|prefix| fragment::resolve_id(vault, prefix))
            .transpose()
            .map_err(map_parc_error)?;
    let seed = match (id, p.text) {
        (id, Some(text)) => Seed::Draft {
            text,
            tags: p.tags,
            id,
        },
        (Some(_), None) if !p.tags.is_empty() => {
            return Err(RpcError::invalid_params(
                "tags describe a draft: send them with text",
            ))
        }
        (Some(id), None) => Seed::Fragment(id),
        (None, None) => return Err(RpcError::invalid_params("give an id, text, or both")),
    };
    let query = RecallQuery {
        seed,
        as_of: None,
        limit: p.limit.unwrap_or(recall::DEFAULT_LIMIT),
    };
    let model = embed::shared().ok().flatten();
    let result = recall::context(conn, &query, model.as_deref()).map_err(map_parc_error)?;
    serde_json::to_value(result).map_err(|e| RpcError::internal_error(&e.to_string()))
}
