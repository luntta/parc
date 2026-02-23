use std::path::Path;

use anyhow::Result;
use parc_core::attachment;
use parc_core::config::load_config;
use parc_core::fragment::read_fragment;
use parc_core::index;
use parc_core::tag;

use crate::render;

pub fn run(vault: &Path, id: &str, json: bool) -> Result<()> {
    let config = load_config(vault)?;
    let fragment = read_fragment(vault, id)?;

    // Query backlinks from index
    let conn = index::open_index(vault)?;
    let backlinks = index::get_backlinks(&conn, &fragment.id)?;

    // Get attachments
    let attachments = attachment::list_attachments(vault, &fragment.id).unwrap_or_default();

    if json {
        let inline_tags = tag::extract_inline_tags(&fragment.body);
        let merged_tags = tag::merge_tags(&fragment.tags, &inline_tags);
        let backlinks_json: Vec<serde_json::Value> = backlinks
            .iter()
            .map(|bl| {
                serde_json::json!({
                    "id": bl.source_id,
                    "type": bl.source_type,
                    "title": bl.source_title,
                })
            })
            .collect();
        let attachments_json: Vec<serde_json::Value> = attachments
            .iter()
            .map(|a| {
                serde_json::json!({
                    "filename": a.filename,
                    "size": a.size,
                })
            })
            .collect();
        let json_val = serde_json::json!({
            "id": fragment.id,
            "type": fragment.fragment_type,
            "title": fragment.title,
            "tags": merged_tags,
            "links": fragment.links,
            "attachments": fragment.attachments,
            "created_at": fragment.created_at.to_rfc3339(),
            "updated_at": fragment.updated_at.to_rfc3339(),
            "created_by": fragment.created_by,
            "extra_fields": fragment.extra_fields,
            "body": fragment.body,
            "backlinks": backlinks_json,
            "attachment_files": attachments_json,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        // Try plugin rendering first
        #[cfg(feature = "wasm-plugins")]
        {
            let mut manager =
                parc_core::plugin::manager::PluginManager::load_all(vault, &config)?;
            if let Ok(Some(rendered)) = manager.render(&fragment) {
                println!("{}", rendered);
                return Ok(());
            }
        }
        render::print_fragment(&fragment, &backlinks, &attachments, config.id_display_length);
    }

    Ok(())
}
