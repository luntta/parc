use anyhow::Result;
use parc_core::vault;

pub fn run(global: bool) -> Result<()> {
    let path = if global {
        vault::global_vault_path()?
    } else {
        std::env::current_dir()?.join(".parc")
    };

    vault::init_vault(&path)?;

    let scope = if global { "global" } else { "local" };
    println!("Initialized {} vault at {}", scope, path.display());

    // Initialize the index
    parc_core::index::init_index(&path)?;

    Ok(())
}
