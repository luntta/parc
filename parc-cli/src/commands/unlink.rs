use std::path::Path;

use anyhow::Result;

use crate::hooks::{cli_writer, report_warnings};

pub fn run(vault: &Path, id_a: &str, id_b: &str, json: bool) -> Result<()> {
    let mut writer = cli_writer(vault);
    let change = writer.unlink(id_a, id_b)?;
    report_warnings(&mut writer);

    if !change.changed {
        println!("Not linked.");
        return Ok(());
    }

    if json {
        let json_val = serde_json::json!({
            "id_a": change.a,
            "id_b": change.b,
            "unlinked": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!("Unlinked {} \u{2194} {}", &change.a[..8], &change.b[..8]);
    }
    Ok(())
}
