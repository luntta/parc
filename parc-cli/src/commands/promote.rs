use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use parc_core::fragment;
use parc_core::index;
use serde_json::Value;

#[allow(clippy::too_many_arguments)]
pub fn run(
    vault: &Path,
    id: &str,
    new_type: &str,
    tags: Vec<String>,
    links: Vec<String>,
    due: Option<String>,
    priority: Option<String>,
    status: Option<String>,
    assignee: Option<String>,
    json: bool,
) -> Result<()> {
    let mut overrides = BTreeMap::new();

    if let Some(s) = status {
        overrides.insert("status".to_string(), Value::String(s));
    }
    if let Some(d) = due {
        let resolved = parc_core::date::resolve_due_date(&d)?;
        overrides.insert("due".to_string(), Value::String(resolved));
    }
    if let Some(p) = priority {
        overrides.insert("priority".to_string(), Value::String(p));
    }
    if let Some(a) = assignee {
        overrides.insert("assignee".to_string(), Value::String(a));
    }

    let mut promoted = fragment::promote_fragment(vault, id, new_type, overrides)?;

    let mut changed = false;
    for tag in tags {
        if !promoted.tags.contains(&tag) {
            promoted.tags.push(tag);
            changed = true;
        }
    }
    for link in links {
        if !promoted.links.contains(&link) {
            promoted.links.push(link);
            changed = true;
        }
    }

    if changed {
        promoted.updated_at = Utc::now();
        fragment::write_fragment(vault, &promoted)?;
    }

    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &promoted, vault)?;

    if json {
        let json_val = serde_json::json!({
            "id": promoted.id,
            "type": promoted.fragment_type,
            "title": promoted.title,
            "promoted": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!(
            "Promoted {} to {}",
            &promoted.id[..8],
            promoted.fragment_type
        );
    }

    Ok(())
}
