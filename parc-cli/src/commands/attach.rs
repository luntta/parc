use std::path::Path;

use anyhow::Result;
use parc_core::attachment;
use parc_core::fragment;
use parc_core::index;

pub fn run_attach(vault: &Path, id: &str, file: &Path, mv: bool) -> Result<()> {
    let full_id = fragment::resolve_id(vault, id)?;
    let filename = attachment::attach_file(vault, &full_id, file, mv)?;

    // Re-index
    let frag = fragment::read_fragment(vault, &full_id)?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;

    println!(
        "Attached '{}' to {}",
        filename,
        &full_id[..8.min(full_id.len())]
    );
    Ok(())
}

pub fn run_detach(vault: &Path, id: &str, filename: &str) -> Result<()> {
    let full_id = fragment::resolve_id(vault, id)?;
    attachment::detach_file(vault, &full_id, filename)?;

    // Re-index
    let frag = fragment::read_fragment(vault, &full_id)?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;

    println!(
        "Detached '{}' from {}",
        filename,
        &full_id[..8.min(full_id.len())]
    );
    Ok(())
}

pub fn run_attachments(vault: &Path, id: &str) -> Result<()> {
    let full_id = fragment::resolve_id(vault, id)?;
    let attachments = attachment::list_attachments(vault, &full_id)?;

    if attachments.is_empty() {
        println!(
            "No attachments for fragment {}.",
            &full_id[..8.min(full_id.len())]
        );
        return Ok(());
    }

    println!("{:<30}  {:>8}", "FILENAME", "SIZE");
    for a in &attachments {
        let size = format_size(a.size);
        println!("{:<30}  {:>8}", a.filename, size);
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
