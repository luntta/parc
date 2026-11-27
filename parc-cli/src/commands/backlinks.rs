use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;
use parc_core::fragment::read_fragment;
use parc_core::index;

use parc_core::display::{sanitize_terminal_line, short_id};

pub fn run(vault: &Path, id: &str, json: bool) -> Result<()> {
    let config = load_config(vault)?;
    let fragment = read_fragment(vault, id)?;
    let conn = index::open_index(vault)?;
    let backlinks = index::get_backlinks(&conn, &fragment.id)?;

    if json {
        let json_val: Vec<serde_json::Value> = backlinks
            .iter()
            .map(|bl| {
                serde_json::json!({
                    "id": bl.source_id,
                    "type": bl.source_type,
                    "title": bl.source_title,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_val)?);
        return Ok(());
    }

    if backlinks.is_empty() {
        println!("No backlinks found.");
        return Ok(());
    }

    let id_len = config.id_display_length;
    let target = sanitize_terminal_line(short_id(&fragment.id, id_len));
    println!(
        "BACKLINKS TO {} \"{}\"",
        target,
        sanitize_terminal_line(&fragment.title)
    );
    println!();
    println!("{:<width$}  {:<10}  TITLE", "ID", "TYPE", width = id_len);

    for bl in &backlinks {
        let short = sanitize_terminal_line(short_id(&bl.source_id, id_len));
        println!(
            "{:<width$}  {:<10}  {}",
            short,
            sanitize_terminal_line(&bl.source_type),
            sanitize_terminal_line(&bl.source_title),
            width = id_len
        );
    }

    Ok(())
}
