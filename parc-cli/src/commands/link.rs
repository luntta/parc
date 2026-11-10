use std::path::Path;

use anyhow::Result;

use crate::hooks::{cli_writer, report_warnings};

pub fn run(vault: &Path, id_a: &str, id_b: &str, json: bool) -> Result<()> {
    let mut writer = cli_writer(vault);
    let change = writer.link(id_a, id_b)?;
    report_warnings(&mut writer);

    if !change.changed {
        println!("Already linked.");
        return Ok(());
    }

    if json {
        let json_val = serde_json::json!({
            "id_a": change.a,
            "id_b": change.b,
            "linked": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!("Linked {} \u{2194} {}", &change.a[..8], &change.b[..8]);
    }
    Ok(())
}
