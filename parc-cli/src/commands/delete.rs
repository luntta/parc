use std::path::Path;

use anyhow::Result;

use crate::hooks::{cli_writer, report_warnings};

pub fn run(vault: &Path, id: &str, json: bool) -> Result<()> {
    let mut writer = cli_writer(vault);
    let full_id = writer.delete(id)?.id;
    report_warnings(&mut writer);

    if json {
        let json_val = serde_json::json!({
            "id": full_id,
            "deleted": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!("Deleted {} (moved to trash)", &full_id[..8]);
    }
    Ok(())
}
