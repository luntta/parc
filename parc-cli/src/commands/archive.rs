use std::path::Path;

use anyhow::Result;

use crate::hooks::{cli_writer, report_warnings};

pub fn run(vault: &Path, id: &str, undo: bool, json: bool) -> Result<()> {
    let mut writer = cli_writer(vault);
    let fragment = writer.set_archived(id, !undo)?;
    report_warnings(&mut writer);
    let full_id = fragment.id;

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
