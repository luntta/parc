use std::path::Path;

use anyhow::Result;
use parc_core::doctor::{self, DoctorFinding};

pub fn run(vault: &Path, json: bool) -> Result<()> {
    if !json {
        println!("Checking vault health...");
        println!();
    }

    let report = doctor::run_doctor(vault)?;

    if json {
        let findings: Vec<serde_json::Value> = report
            .findings
            .iter()
            .map(|f| match f {
                DoctorFinding::BrokenLink {
                    source_id,
                    source_title,
                    target_ref,
                } => serde_json::json!({
                    "type": "broken_link",
                    "source_id": source_id,
                    "source_title": source_title,
                    "target_ref": target_ref,
                }),
                DoctorFinding::OrphanFragment { id, title } => serde_json::json!({
                    "type": "orphan",
                    "id": id,
                    "title": title,
                }),
                DoctorFinding::SchemaViolation { id, title, message } => serde_json::json!({
                    "type": "schema_violation",
                    "id": id,
                    "title": title,
                    "message": message,
                }),
                DoctorFinding::AttachmentMismatch { fragment_id, detail } => serde_json::json!({
                    "type": "attachment_mismatch",
                    "fragment_id": fragment_id,
                    "detail": detail,
                }),
                DoctorFinding::VaultSizeWarning { total_bytes } => serde_json::json!({
                    "type": "vault_size_warning",
                    "total_bytes": total_bytes,
                }),
                DoctorFinding::PluginIssue { plugin_name, detail } => serde_json::json!({
                    "type": "plugin_issue",
                    "plugin_name": plugin_name,
                    "detail": detail,
                }),
            })
            .collect();

        let json_val = serde_json::json!({
            "fragments_checked": report.fragments_checked,
            "healthy": report.is_healthy(),
            "findings": findings,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        for finding in &report.findings {
            match finding {
                DoctorFinding::BrokenLink {
                    source_id,
                    source_title,
                    target_ref,
                } => {
                    println!(
                        "\u{2717} Broken link: {} \"{}\" \u{2192} {} (not found)",
                        &source_id[..8.min(source_id.len())],
                        source_title,
                        target_ref
                    );
                }
                DoctorFinding::SchemaViolation { id, title, message } => {
                    println!(
                        "\u{2717} Schema violation: {} \"{}\" \u{2014} {}",
                        &id[..8.min(id.len())],
                        title,
                        message
                    );
                }
                DoctorFinding::OrphanFragment { id, title } => {
                    println!(
                        "! Orphan: {} \"{}\" (no links in or out)",
                        &id[..8.min(id.len())],
                        title
                    );
                }
                DoctorFinding::AttachmentMismatch {
                    fragment_id,
                    detail,
                } => {
                    println!(
                        "\u{2717} Attachment: {} \u{2014} {}",
                        &fragment_id[..8.min(fragment_id.len())],
                        detail
                    );
                }
                DoctorFinding::VaultSizeWarning { total_bytes } => {
                    let size_mb = *total_bytes as f64 / (1024.0 * 1024.0);
                    println!(
                        "! Vault size: {:.1} MB (exceeds 500 MB warning threshold)",
                        size_mb
                    );
                }
                DoctorFinding::PluginIssue { plugin_name, detail } => {
                    println!(
                        "\u{2717} Plugin '{}': {}",
                        plugin_name, detail
                    );
                }
            }
        }

        if report.findings.is_empty() {
            println!("Checked {} fragments: no issues found.", report.fragments_checked);
        } else {
            println!();
            println!(
                "Checked {} fragments: {} issues found.",
                report.fragments_checked,
                report.findings.len()
            );
        }
    }

    if !report.is_healthy() {
        std::process::exit(1);
    }

    Ok(())
}
