use std::path::Path;

use anyhow::Result;
use parc_core::import::{self, ImportStatus};
use parc_core::index;

use crate::render::sanitize_terminal_text;

pub fn run(vault: &Path, file: &str, dry_run: bool, json: bool) -> Result<()> {
    let content = std::fs::read_to_string(file)?;
    let results = import::import_json(vault, &content, dry_run)?;

    // Re-index if not dry run
    if !dry_run {
        index::reindex(vault)?;
    }

    let created = results
        .iter()
        .filter(|r| matches!(r.status, ImportStatus::Created))
        .count();
    let errors = results
        .iter()
        .filter(|r| matches!(r.status, ImportStatus::Error(_)))
        .count();
    let skipped = results
        .iter()
        .filter(|r| matches!(r.status, ImportStatus::Skipped(_)))
        .count();

    if json {
        let json_results: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                let (status, detail) = match &r.status {
                    ImportStatus::Created => ("created", None),
                    ImportStatus::Skipped(s) => ("skipped", Some(s.as_str())),
                    ImportStatus::Error(e) => ("error", Some(e.as_str())),
                };
                let mut val = serde_json::json!({
                    "id": r.id,
                    "title": r.title,
                    "status": status,
                });
                if let Some(d) = detail {
                    val["detail"] = serde_json::Value::String(d.to_string());
                }
                val
            })
            .collect();
        let json_val = serde_json::json!({
            "dry_run": dry_run,
            "created": created,
            "skipped": skipped,
            "errors": errors,
            "results": json_results,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        let prefix = if dry_run { "[dry-run] " } else { "" };
        for r in &results {
            match &r.status {
                ImportStatus::Created => {
                    println!(
                        "{}Created {} \"{}\"",
                        prefix,
                        &r.id[..8.min(r.id.len())],
                        sanitize_terminal_text(&r.title)
                    );
                }
                ImportStatus::Skipped(reason) => {
                    println!(
                        "{}Skipped \"{}\": {}",
                        prefix,
                        sanitize_terminal_text(&r.title),
                        sanitize_terminal_text(reason)
                    );
                }
                ImportStatus::Error(err) => {
                    println!(
                        "{}Error \"{}\": {}",
                        prefix,
                        sanitize_terminal_text(&r.title),
                        sanitize_terminal_text(err)
                    );
                }
            }
        }
        println!(
            "\n{}Summary: {} created, {} skipped, {} errors.",
            prefix, created, skipped, errors
        );
    }

    Ok(())
}
