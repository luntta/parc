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

pub fn reindex(vault_path: &Path, _params: Value) -> Result<Value, RpcError> {
    let count = index::reindex(vault_path).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "indexed": count,
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
        })
        .collect();

    Ok(serde_json::json!({
        "fragments_checked": report.fragments_checked,
        "healthy": report.is_healthy(),
        "findings": findings,
    }))
}
