use std::path::Path;

use anyhow::Result;
use parc_core::fragment::{delete_fragment, read_fragment};
use parc_core::hook::{self, HookEvent};
use parc_core::index;

use crate::hooks::CliHookRunner;

pub fn run(vault: &Path, id: &str, json: bool) -> Result<()> {
    let runner = CliHookRunner;

    // Read fragment before deleting so we can pass it to hooks
    let fragment = read_fragment(vault, id)?;

    #[cfg(feature = "wasm-plugins")]
    let mut plugin_manager = {
        let config = parc_core::config::load_config(vault)?;
        parc_core::plugin::manager::PluginManager::load_all(vault, &config)
            .unwrap_or_else(|e| {
                eprintln!("Warning: failed to load plugins: {}", e);
                parc_core::plugin::manager::PluginManager::empty().unwrap()
            })
    };

    // Run pre-delete hooks
    #[cfg(feature = "wasm-plugins")]
    let _ = hook::run_pre_hooks_with_plugins(&runner, vault, HookEvent::PreDelete, &fragment, &mut plugin_manager)?;
    #[cfg(not(feature = "wasm-plugins"))]
    let _ = hook::run_pre_hooks(&runner, vault, HookEvent::PreDelete, &fragment)?;

    let full_id = delete_fragment(vault, &fragment.id)?;

    // Remove from index
    let conn = index::open_index(vault)?;
    index::remove_from_index(&conn, &full_id)?;

    // Run post-delete hooks
    #[cfg(feature = "wasm-plugins")]
    hook::run_post_hooks_with_plugins(&runner, vault, HookEvent::PostDelete, &fragment, &mut plugin_manager);
    #[cfg(not(feature = "wasm-plugins"))]
    hook::run_post_hooks(&runner, vault, HookEvent::PostDelete, &fragment);

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
