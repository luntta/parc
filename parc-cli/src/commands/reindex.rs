use anyhow::Result;
use parc_core::index;
use parc_core::vault::discover_vault;

pub fn run() -> Result<()> {
    let vault = discover_vault()?;
    let count = index::reindex(&vault)?;
    println!("Reindexed {} fragments.", count);
    Ok(())
}
