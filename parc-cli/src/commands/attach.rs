use std::path::Path;

use anyhow::Result;
use parc_core::attachment;
use parc_core::fragment;
use parc_core::index;

use crate::render::sanitize_terminal_text;

pub fn run_attach(vault: &Path, id: &str, file: &Path, mv: bool, json: bool) -> Result<()> {
    let full_id = fragment::resolve_id(vault, id)?;
    let filename = attachment::attach_file(vault, &full_id, file, mv)?;

    // Re-index
    let frag = fragment::read_fragment(vault, &full_id)?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;

    if json {
        let json_val = serde_json::json!({
            "id": full_id,
            "filename": filename,
            "attached": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!(
            "Attached '{}' to {}",
            sanitize_terminal_text(&filename),
            &full_id[..8.min(full_id.len())]
        );
    }
    Ok(())
}

pub fn run_detach(vault: &Path, id: &str, filename: &str, json: bool) -> Result<()> {
    let full_id = fragment::resolve_id(vault, id)?;
    attachment::detach_file(vault, &full_id, filename)?;

    // Re-index
    let frag = fragment::read_fragment(vault, &full_id)?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;

    if json {
        let json_val = serde_json::json!({
            "id": full_id,
            "filename": filename,
            "detached": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!(
            "Detached '{}' from {}",
            sanitize_terminal_text(filename),
            &full_id[..8.min(full_id.len())]
        );
    }
    Ok(())
}

pub fn run_attachments(vault: &Path, id: &str, json: bool) -> Result<()> {
    let full_id = fragment::resolve_id(vault, id)?;
    let attachments = attachment::list_attachments(vault, &full_id)?;

    if json {
        let json_val: Vec<serde_json::Value> = attachments
            .iter()
            .map(|a| {
                serde_json::json!({
                    "filename": a.filename,
                    "size": a.size,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else if attachments.is_empty() {
        println!(
            "No attachments for fragment {}.",
            &full_id[..8.min(full_id.len())]
        );
    } else {
        println!("{:<30}  {:>8}", "FILENAME", "SIZE");
        for a in &attachments {
            let size = format_size(a.size);
            println!("{:<30}  {:>8}", sanitize_terminal_text(&a.filename), size);
        }
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
