use anyhow::Result;
use parc_core::config::load_config;
use parc_core::fragment::read_fragment;
use parc_core::index;
use parc_core::tag;
use parc_core::vault::discover_vault;

use crate::render;

pub fn run(id: &str, json: bool) -> Result<()> {
    let vault = discover_vault()?;
    let config = load_config(&vault)?;
    let fragment = read_fragment(&vault, id)?;

    // Query backlinks from index
    let conn = index::open_index(&vault)?;
    let backlinks = index::get_backlinks(&conn, &fragment.id)?;

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
        let json_val = serde_json::json!({
            "id": fragment.id,
            "type": fragment.fragment_type,
            "title": fragment.title,
            "tags": merged_tags,
            "links": fragment.links,
            "created_at": fragment.created_at.to_rfc3339(),
            "updated_at": fragment.updated_at.to_rfc3339(),
            "created_by": fragment.created_by,
            "extra_fields": fragment.extra_fields,
            "body": fragment.body,
            "backlinks": backlinks_json,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        render::print_fragment(&fragment, &backlinks, config.id_display_length);
    }

    Ok(())
}
