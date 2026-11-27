use std::path::Path;

use anyhow::Result;
use parc_core::index;

use crate::render::sanitize_terminal_text;

pub fn run(vault: &Path, json: bool) -> Result<()> {
    let report = index::reindex(vault)?;
    let conn = index::open_index(vault)?;
    let extract_report = parc_core::extract::fill(vault, &conn)?;
    if json {
        let problems: Vec<serde_json::Value> = report
            .problems
            .iter()
            .map(|p| {
                serde_json::json!({
                    "file": format!("fragments/{}", p.file_name),
                    "reason": p.reason,
                })
            })
            .collect();
        let json_val = serde_json::json!({
            "fragments_indexed": report.added,
            "attachments_extracted": extract_report.extracted,
            "attachments_errors": extract_report.errors,
            "problems": problems,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!("Reindexed {} fragments.", report.added);
        if extract_report.extracted > 0 || extract_report.errors > 0 {
            println!(
                "Extracted text from {} attachment(s){}.",
                extract_report.extracted,
                if extract_report.errors > 0 {
                    format!(" ({} failed)", extract_report.errors)
                } else {
                    String::new()
                }
            );
        }
        if !report.problems.is_empty() {
            println!(
                "{} {} not indexed:",
                report.problems.len(),
                if report.problems.len() == 1 {
                    "file was"
                } else {
                    "files were"
                }
            );
            for problem in &report.problems {
                println!(
                    "  fragments/{}: {}",
                    sanitize_terminal_text(&problem.file_name),
                    sanitize_terminal_text(&problem.reason)
                );
            }
        }
    }
    Ok(())
}
