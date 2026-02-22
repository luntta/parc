use std::path::Path;

use anyhow::Result;
use parc_core::fragment::delete_fragment;
use parc_core::index;

pub fn run(vault: &Path, id: &str) -> Result<()> {
    let full_id = delete_fragment(vault, id)?;

    // Remove from index
    let conn = index::open_index(vault)?;
    index::remove_from_index(&conn, &full_id)?;

    println!("Deleted {} (moved to trash)", &full_id[..8]);
    Ok(())
}
