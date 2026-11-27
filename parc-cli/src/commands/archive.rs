use std::path::Path;

use anyhow::Result;
use parc_core::display::short_id;

use crate::hooks::{cli_writer, report_warnings};

pub fn run(vault: &Path, id: &str, undo: bool, json: bool) -> Result<()> {
    let id_len = parc_core::config::load_config(vault)?.id_display_length;
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
        println!("Unarchived {}", short_id(&full_id, id_len));
    } else {
        println!("Archived {}", short_id(&full_id, id_len));
    }

    Ok(())
}
