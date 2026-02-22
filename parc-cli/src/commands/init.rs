use std::path::Path;

use anyhow::Result;
use parc_core::vault;

pub fn run(global: bool, explicit_vault: Option<&Path>) -> Result<()> {
    let path = if let Some(p) = explicit_vault {
        if p.ends_with(".parc") {
            p.to_path_buf()
        } else {
            p.join(".parc")
        }
    } else if global {
        vault::global_vault_path()?
    } else {
        std::env::current_dir()?.join(".parc")
    };

    vault::init_vault(&path)?;

    let scope = if global {
        "global"
    } else if explicit_vault.is_some() {
        "new"
    } else {
        "local"
    };
    println!("Initialized {} vault at {}", scope, path.display());

    // Initialize the index
    parc_core::index::init_index(&path)?;

    Ok(())
}
