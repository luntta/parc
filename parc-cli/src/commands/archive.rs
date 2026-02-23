use std::path::Path;

use anyhow::Result;
use chrono::Utc;
use parc_core::fragment::{read_fragment, write_fragment};
use parc_core::index;
use serde_json::Value;

pub fn run(vault: &Path, id: &str, undo: bool, json: bool) -> Result<()> {
    let mut fragment = read_fragment(vault, id)?;
    let full_id = fragment.id.clone();

    if undo {
        fragment.extra_fields.remove("archived");
    } else {
        fragment
            .extra_fields
            .insert("archived".to_string(), Value::Bool(true));
    }

    fragment.updated_at = Utc::now();
    write_fragment(vault, &fragment)?;

    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &fragment, vault)?;

    if json {
        let json_val = serde_json::json!({
            "id": full_id,
            "archived": !undo,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else if undo {
        println!("Unarchived {}", &full_id[..8.min(full_id.len())]);
    } else {
        println!("Archived {}", &full_id[..8.min(full_id.len())]);
    }

    Ok(())
}
