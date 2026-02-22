use std::path::Path;

use anyhow::Result;
use parc_core::index;

pub fn run(vault: &Path) -> Result<()> {
    let count = index::reindex(vault)?;
    println!("Reindexed {} fragments.", count);
    Ok(())
}
