use std::path::Path;

use anyhow::{bail, Context, Result};
use parc_core::config::{get_editor, load_config};
use parc_core::fragment::{parse_fragment, read_fragment, serialize_fragment, validate_fragment};
use parc_core::schema::load_schemas;
use parc_core::secure_fs;

use crate::hooks::{cli_writer, report_warnings};
use crate::render::sanitize_terminal_text;

pub fn run(vault: &Path, id: &str, json: bool) -> Result<()> {
    let config = load_config(vault)?;
    let schemas = load_schemas(vault)?;
    let original = read_fragment(vault, id)?;
    let content = serialize_fragment(&original);
    let editor = get_editor(&config);
    let tmp_path = secure_fs::write_private_temp("parc-edit", ".md", content.as_bytes())?;

    let mut last_error: Option<String> = None;

    loop {
        if let Some(ref err) = last_error {
            eprintln!("Validation error: {}", sanitize_terminal_text(err));
            eprintln!("Re-opening editor. Save empty file to abort.");
        }

        let status = std::process::Command::new(&editor)
            .arg(&tmp_path)
            .status()
            .with_context(|| format!("failed to launch editor: {}", editor))?;

        if !status.success() {
            let _ = std::fs::remove_file(&tmp_path);
            bail!("editor exited with non-zero status");
        }

        let edited_content = std::fs::read_to_string(&tmp_path)?;

        if edited_content.trim().is_empty() {
            let _ = std::fs::remove_file(&tmp_path);
            bail!("aborted: empty content");
        }

        // If content unchanged, abort
        if edited_content == content {
            let _ = std::fs::remove_file(&tmp_path);
            println!("No changes made.");
            return Ok(());
        }

        let frag = match parse_fragment(&edited_content) {
            Ok(frag) => frag,
            Err(e) => {
                last_error = Some(e.to_string());
                continue;
            }
        };
        // Unknown types are accepted (could be a custom type without a schema).
        if let Some(schema) = schemas.resolve(&frag.fragment_type) {
            if let Err(e) = validate_fragment(&frag, schema) {
                last_error = Some(e.to_string());
                continue;
            }
        }

        let mut writer = cli_writer(vault);
        let frag = writer.update(frag).with_context(|| {
            format!("save failed; your edits are kept in {}", tmp_path.display())
        })?;
        let _ = std::fs::remove_file(&tmp_path);
        report_warnings(&mut writer);

        if json {
            let json_val = serde_json::json!({"id": frag.id, "updated": true});
            println!("{}", serde_json::to_string_pretty(&json_val)?);
        } else {
            println!("Updated {}", frag.id);
        }
        return Ok(());
    }
}
