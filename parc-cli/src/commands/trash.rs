use std::path::Path;

use anyhow::Result;
use parc_core::fragment;
use parc_core::index;
use parc_core::secure_fs;

use crate::render::sanitize_terminal_text;

pub fn run(
    vault: &Path,
    purge: bool,
    purge_id: Option<String>,
    restore: Option<String>,
    json: bool,
) -> Result<()> {
    if let Some(id) = restore {
        return run_restore(vault, &id, json);
    }

    if purge {
        return run_purge(vault, purge_id.as_deref(), json);
    }

    // Default: list trashed fragments
    run_list(vault, json)
}

fn run_list(vault: &Path, json: bool) -> Result<()> {
    let trash_dir = vault.join("trash");
    if !trash_dir.is_dir() {
        if json {
            println!("[]");
        } else {
            println!("Trash is empty.");
        }
        return Ok(());
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&trash_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(frag) = fragment::parse_fragment(&content) {
                    entries.push(frag);
                }
            }
        }
    }

    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    if json {
        let json_val: Vec<serde_json::Value> = entries
            .iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "type": f.fragment_type,
                    "title": f.title,
                    "updated_at": f.updated_at.to_rfc3339(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else if entries.is_empty() {
        println!("Trash is empty.");
    } else {
        println!(
            "{:<10}  {:<10}  {:<40}  {}",
            "ID", "TYPE", "TITLE", "DELETED"
        );
        for f in &entries {
            let short_id = &f.id[..8.min(f.id.len())];
            let safe_title = sanitize_terminal_text(&f.title);
            let title = if safe_title.chars().count() > 40 {
                format!("{}...", safe_title.chars().take(37).collect::<String>())
            } else {
                safe_title
            };
            println!(
                "{:<10}  {:<10}  {:<40}  {}",
                short_id,
                sanitize_terminal_text(&f.fragment_type),
                title,
                f.updated_at.format("%Y-%m-%d %H:%M")
            );
        }
        println!("\n{} trashed fragment(s).", entries.len());
    }

    Ok(())
}

fn run_purge(vault: &Path, id: Option<&str>, json: bool) -> Result<()> {
    let trash_dir = vault.join("trash");

    if let Some(prefix) = id {
        // Purge a specific fragment
        let upper = prefix.to_uppercase();
        let mut found = false;
        if trash_dir.is_dir() {
            for entry in std::fs::read_dir(&trash_dir)? {
                let entry = entry?;
                let path = entry.path();
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem.starts_with(&upper) {
                        std::fs::remove_file(&path)?;
                        if json {
                            let json_val = serde_json::json!({
                                "purged": stem,
                            });
                            println!("{}", serde_json::to_string_pretty(&json_val)?);
                        } else {
                            println!("Purged {}", &stem[..8.min(stem.len())]);
                        }
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            anyhow::bail!("fragment '{}' not found in trash", prefix);
        }
    } else {
        // Purge all
        let mut count = 0;
        if trash_dir.is_dir() {
            for entry in std::fs::read_dir(&trash_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "md") {
                    std::fs::remove_file(&path)?;
                    count += 1;
                }
            }
        }
        if json {
            let json_val = serde_json::json!({
                "purged_count": count,
            });
            println!("{}", serde_json::to_string_pretty(&json_val)?);
        } else {
            println!("Purged {} fragment(s) from trash.", count);
        }
    }

    Ok(())
}

fn run_restore(vault: &Path, id: &str, json: bool) -> Result<()> {
    let upper = id.to_uppercase();
    let trash_dir = vault.join("trash");
    let fragments_dir = vault.join("fragments");

    if !trash_dir.is_dir() {
        anyhow::bail!("fragment '{}' not found in trash", id);
    }

    for entry in std::fs::read_dir(&trash_dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if stem.starts_with(&upper) {
                if fragment::validate_id(stem).is_err() {
                    continue;
                }
                let dest = fragments_dir.join(format!("{}.md", stem));
                secure_fs::rename_private_file(&path, &dest)?;

                // Re-index
                let frag = fragment::read_fragment(vault, stem)?;
                let conn = index::open_index(vault)?;
                index::index_fragment_auto(&conn, &frag, vault)?;

                if json {
                    let json_val = serde_json::json!({
                        "restored": stem,
                    });
                    println!("{}", serde_json::to_string_pretty(&json_val)?);
                } else {
                    println!("Restored {}", &stem[..8.min(stem.len())]);
                }
                return Ok(());
            }
        }
    }

    anyhow::bail!("fragment '{}' not found in trash", id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_skips_invalid_trash_stems() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        parc_core::vault::init_vault(&vault).unwrap();

        let invalid = vault.join("trash").join("01BAD.md");
        parc_core::secure_fs::write_private(&invalid, "---\nid: 01BAD\ntype: note\n---\n").unwrap();

        let result = run_restore(&vault, "01", false);

        assert!(result.is_err());
        assert!(invalid.exists());
        assert!(!vault.join("fragments").join("01BAD.md").exists());
    }
}
