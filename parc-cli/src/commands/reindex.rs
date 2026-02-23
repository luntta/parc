use std::path::Path;

use anyhow::Result;
use parc_core::index;

pub fn run(vault: &Path, json: bool) -> Result<()> {
    let count = index::reindex(vault)?;
    if json {
        let json_val = serde_json::json!({
            "fragments_indexed": count,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!("Reindexed {} fragments.", count);
    }
    Ok(())
}
