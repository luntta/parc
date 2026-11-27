use std::collections::BTreeMap;
use std::path::Path;

use crate::jsonrpc::RpcError;
use crate::methods::write;
use crate::router::map_parc_error;
use parc_core::{inbox, index, vault as vault_mod};
use serde_json::{json, Value};

pub fn sweep(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let mut writer = write::writer(vault);
    let report = inbox::sweep_deferred(&mut writer).map_err(map_parc_error)?;
    let mut result = json!(report);
    result["inbox_path"] = json!(vault_mod::inbox_dir(vault));
    Ok(write::with_warnings(result, &mut writer))
}

/// One index sync and one ordered query per vault, not one sync per source.
pub fn attachments(vault: &Path) -> Result<Value, RpcError> {
    fn query(vault: &Path) -> Result<Value, parc_core::ParcError> {
        let conn = index::open_index(vault)?;
        let mut stmt = conn.prepare(
            "SELECT a.fragment_id, a.filename, a.size, a.status, a.unit, a.parts, a.error
            FROM attachment_files a JOIN fragments f ON f.id=a.fragment_id
            WHERE f.type='source' AND f.archived=0 ORDER BY a.fragment_id, a.filename",
        )?;
        let mut grouped = BTreeMap::<String, Vec<Value>>::new();
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            grouped.entry(r.get(0)?).or_default().push(json!({
                "filename": r.get::<_, String>(1)?, "size": r.get::<_, i64>(2)?,
                "status": r.get::<_, String>(3)?, "unit": r.get::<_, Option<String>>(4)?,
                "parts": r.get::<_, i64>(5)?, "error": r.get::<_, Option<String>>(6)?
            }));
        }
        Ok(json!(grouped))
    }
    query(vault).map_err(map_parc_error)
}
