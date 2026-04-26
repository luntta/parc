use std::path::Path;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use parc_core::config::{get_editor, load_config};
use parc_core::fragment::{
    self, parse_fragment, read_fragment, serialize_fragment, validate_fragment,
};
use parc_core::hook::{self, HookEvent};
use parc_core::index;
use parc_core::schema::load_schemas;

use crate::hooks::CliHookRunner;

pub fn run(vault: &Path, id: &str, json: bool) -> Result<()> {
    let config = load_config(vault)?;
    let schemas = load_schemas(vault)?;
    let original = read_fragment(vault, id)?;
    let full_id = original.id.clone();
    let runner = CliHookRunner;

    #[cfg(feature = "wasm-plugins")]
    let mut plugin_manager = parc_core::plugin::manager::PluginManager::load_all(vault, &config)
        .unwrap_or_else(|e| {
            eprintln!("Warning: failed to load plugins: {}", e);
            parc_core::plugin::manager::PluginManager::empty().unwrap()
        });

    let content = serialize_fragment(&original);
    let editor = get_editor(&config);
    let tmp_path = std::env::temp_dir().join(format!("parc-edit-{}.md", &full_id[..8]));
    std::fs::write(&tmp_path, &content)?;

    let mut last_error: Option<String> = None;

    loop {
        if let Some(ref err) = last_error {
            eprintln!("Validation error: {}", err);
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

        match parse_fragment(&edited_content) {
            Ok(mut frag) => {
                let schema = schemas.resolve(&frag.fragment_type);
                if let Some(s) = schema {
                    match validate_fragment(&frag, s) {
                        Ok(()) => {
                            frag.updated_at = Utc::now();

                            // Run pre-update hooks
                            #[cfg(feature = "wasm-plugins")]
                            let frag = hook::run_pre_hooks_with_plugins(
                                &runner,
                                vault,
                                HookEvent::PreUpdate,
                                &frag,
                                &mut plugin_manager,
                            )?;
                            #[cfg(not(feature = "wasm-plugins"))]
                            let frag =
                                hook::run_pre_hooks(&runner, vault, HookEvent::PreUpdate, &frag)?;

                            fragment::write_fragment(vault, &frag)?;

                            let conn = index::open_index(vault)?;
                            index::index_fragment_auto(&conn, &frag, vault)?;

                            // Run post-update hooks
                            #[cfg(feature = "wasm-plugins")]
                            hook::run_post_hooks_with_plugins(
                                &runner,
                                vault,
                                HookEvent::PostUpdate,
                                &frag,
                                &mut plugin_manager,
                            );
                            #[cfg(not(feature = "wasm-plugins"))]
                            hook::run_post_hooks(&runner, vault, HookEvent::PostUpdate, &frag);

                            let _ = std::fs::remove_file(&tmp_path);
                            if json {
                                let json_val = serde_json::json!({"id": frag.id, "updated": true});
                                println!("{}", serde_json::to_string_pretty(&json_val)?);
                            } else {
                                println!("Updated {}", frag.id);
                            }
                            return Ok(());
                        }
                        Err(e) => {
                            last_error = Some(e.to_string());
                        }
                    }
                } else {
                    // No schema found — accept anyway (could be custom type)
                    frag.updated_at = Utc::now();

                    #[cfg(feature = "wasm-plugins")]
                    let frag = hook::run_pre_hooks_with_plugins(
                        &runner,
                        vault,
                        HookEvent::PreUpdate,
                        &frag,
                        &mut plugin_manager,
                    )?;
                    #[cfg(not(feature = "wasm-plugins"))]
                    let frag = hook::run_pre_hooks(&runner, vault, HookEvent::PreUpdate, &frag)?;

                    fragment::write_fragment(vault, &frag)?;

                    let conn = index::open_index(vault)?;
                    index::index_fragment_auto(&conn, &frag, vault)?;

                    #[cfg(feature = "wasm-plugins")]
                    hook::run_post_hooks_with_plugins(
                        &runner,
                        vault,
                        HookEvent::PostUpdate,
                        &frag,
                        &mut plugin_manager,
                    );
                    #[cfg(not(feature = "wasm-plugins"))]
                    hook::run_post_hooks(&runner, vault, HookEvent::PostUpdate, &frag);

                    let _ = std::fs::remove_file(&tmp_path);
                    println!("Updated {}", frag.id);
                    return Ok(());
                }
            }
            Err(e) => {
                last_error = Some(e.to_string());
            }
        }
    }
}
