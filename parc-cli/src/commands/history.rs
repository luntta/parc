use std::path::Path;

use anyhow::Result;
use parc_core::fragment;
use parc_core::history;
use parc_core::index;

pub fn run(
    vault: &Path,
    id: &str,
    show: Option<String>,
    diff: bool,
    diff_timestamp: Option<String>,
    restore: Option<String>,
) -> Result<()> {
    let full_id = fragment::resolve_id(vault, id)?;

    if let Some(timestamp) = restore {
        let restored = history::restore_version(vault, &full_id, &timestamp)?;

        // Re-index the restored version
        let conn = index::open_index(vault)?;
        index::index_fragment_auto(&conn, &restored, vault)?;

        println!(
            "Restored {} to version {}",
            &full_id[..8.min(full_id.len())],
            timestamp
        );
        return Ok(());
    }

    if let Some(timestamp) = show {
        let version = history::read_version(vault, &full_id, &timestamp)?;
        let skin = termimad::MadSkin::default();

        println!("--- Version {} ---", timestamp);
        println!("Title: {}", version.title);
        println!();
        if !version.body.is_empty() {
            skin.print_text(&version.body);
        }
        return Ok(());
    }

    if diff {
        let diff_output =
            history::diff_versions(vault, &full_id, diff_timestamp.as_deref())?;
        if diff_output.is_empty() {
            println!("No differences.");
        } else {
            // Print with color
            for line in diff_output.lines() {
                if line.starts_with('+') && !line.starts_with("+++") {
                    println!("\x1b[32m{}\x1b[0m", line);
                } else if line.starts_with('-') && !line.starts_with("---") {
                    println!("\x1b[31m{}\x1b[0m", line);
                } else if line.starts_with("@@") {
                    println!("\x1b[36m{}\x1b[0m", line);
                } else {
                    println!("{}", line);
                }
            }
        }
        return Ok(());
    }

    // Default: list versions
    let versions = history::list_versions(vault, &full_id)?;

    if versions.is_empty() {
        println!(
            "No history for fragment {}.",
            &full_id[..8.min(full_id.len())]
        );
        return Ok(());
    }

    println!(
        "History for {} ({} versions):\n",
        &full_id[..8.min(full_id.len())],
        versions.len()
    );
    println!("{:<28}  {:>8}", "TIMESTAMP", "SIZE");

    for v in &versions {
        let size = format_size(v.size);
        println!("{:<28}  {:>8}", v.timestamp, size);
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
