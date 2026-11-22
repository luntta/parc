use std::path::Path;

use serde_json::Value;

use parc_core::doctor;
use parc_core::index;
use parc_core::vault;

use crate::jsonrpc::RpcError;
use crate::router::map_parc_error;

pub fn info(vault_path: &Path, _params: Value) -> Result<Value, RpcError> {
    let info = vault::vault_info(vault_path).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "path": info.path.to_string_lossy(),
        "scope": format!("{:?}", info.scope).to_lowercase(),
        "fragment_count": info.fragment_count,
    }))
}

/// Every link in the index as `{ "source", "target" }` full IDs, for drawing
/// the whole vault as a graph without one request per fragment.
pub fn links(vault_path: &Path, _params: Value) -> Result<Value, RpcError> {
    let conn = index::open_index(vault_path).map_err(map_parc_error)?;
    let edges = index::all_links(&conn).map_err(map_parc_error)?;
    Ok(Value::Array(
        edges
            .into_iter()
            .map(|(source, target)| serde_json::json!({ "source": source, "target": target }))
            .collect(),
    ))
}

/// Rebuild the index from scratch. Every method already reads an index synced
/// with the files, so this is only for recovery; `problems` lists the files
/// that could not be indexed.
pub fn reindex(vault_path: &Path, _params: Value) -> Result<Value, RpcError> {
    let report = index::reindex(vault_path).map_err(map_parc_error)?;
    let problems: Vec<Value> = report
        .problems
        .iter()
        .map(|p| serde_json::json!({ "file": format!("fragments/{}", p.file_name), "reason": p.reason }))
        .collect();

    Ok(serde_json::json!({
        "indexed": report.added,
        "problems": problems,
    }))
}

pub fn doctor(vault_path: &Path, _params: Value) -> Result<Value, RpcError> {
    let report = doctor::run_doctor(vault_path).map_err(map_parc_error)?;

    let findings: Vec<Value> = report
        .findings
        .iter()
        .map(|f| match f {
            doctor::DoctorFinding::BrokenLink {
                source_id,
                source_title,
                target_ref,
            } => serde_json::json!({
                "type": "broken_link",
                "source_id": source_id,
                "source_title": source_title,
                "target_ref": target_ref,
            }),
            doctor::DoctorFinding::AmbiguousLink {
                source_id,
                source_title,
                target_ref,
                matches,
            } => serde_json::json!({
                "type": "ambiguous_link",
                "source_id": source_id,
                "source_title": source_title,
                "target_ref": target_ref,
                "matches": matches,
            }),
            doctor::DoctorFinding::OrphanFragment { id, title } => serde_json::json!({
                "type": "orphan",
                "id": id,
                "title": title,
            }),
            doctor::DoctorFinding::SchemaViolation { id, title, message } => serde_json::json!({
                "type": "schema_violation",
                "id": id,
                "title": title,
                "message": message,
            }),
            doctor::DoctorFinding::AttachmentMismatch {
                fragment_id,
                detail,
            } => serde_json::json!({
                "type": "attachment_mismatch",
                "fragment_id": fragment_id,
                "detail": detail,
            }),
            doctor::DoctorFinding::VaultSizeWarning { total_bytes } => serde_json::json!({
                "type": "vault_size_warning",
                "total_bytes": total_bytes,
            }),
            doctor::DoctorFinding::PluginIssue {
                plugin_name,
                detail,
            } => serde_json::json!({
                "type": "plugin_issue",
                "plugin_name": plugin_name,
                "detail": detail,
            }),
            doctor::DoctorFinding::UnreadableFragment { file_name, reason } => serde_json::json!({
                "type": "unreadable_fragment",
                "file": format!("fragments/{}", file_name),
                "reason": reason,
            }),
        })
        .collect();

    Ok(serde_json::json!({
        "fragments_checked": report.fragments_checked,
        "healthy": report.is_healthy(),
        "findings": findings,
    }))
}
