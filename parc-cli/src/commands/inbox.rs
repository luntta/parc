use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::{inbox, index, search};

use crate::hooks::{cli_writer, report_warnings};
use crate::render;

pub fn run(vault: &Path, json: bool) -> Result<()> {
    let mut writer = cli_writer(vault);
    let report = inbox::sweep(&mut writer)?;
    report_warnings(&mut writer);

    let config = load_config(vault)?;
    let conn = index::open_index(vault)?;
    let query = search::parse_query("type:source status:!processed")?;
    let items = search::search(&conn, &query)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "inbox_path": parc_core::vault::inbox_dir(vault),
                "swept": report,
                "items": items,
            }))?
        );
        return Ok(());
    }

    if !report.created.is_empty() {
        println!(
            "Swept {} {} from inbox.",
            report.created.len(),
            if report.created.len() == 1 {
                "file"
            } else {
                "files"
            }
        );
    }
    if !report.waiting.is_empty() {
        println!(
            "Waiting on {} {} still settling.",
            report.waiting.len(),
            if report.waiting.len() == 1 {
                "file"
            } else {
                "files"
            }
        );
    }
    if !report.skipped.is_empty() {
        println!("Skipped {} inbox entries.", report.skipped.len());
    }
    for failure in &report.failed {
        eprintln!(
            "Could not import {}: {}",
            render::sanitize_terminal_text(&failure.filename),
            render::sanitize_terminal_text(&failure.error)
        );
    }
    if items.is_empty()
        && report.failed.is_empty()
        && report.skipped.is_empty()
        && report.waiting.is_empty()
    {
        println!(
            "Inbox is empty. Drop files in {} to add sources.",
            parc_core::vault::inbox_dir(vault).display()
        );
    } else if !items.is_empty() {
        render::print_table(&items, config.id_display_length);
    }
    Ok(())
}
