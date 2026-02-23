use std::path::Path;

use anyhow::Result;

pub fn run_add(vault: &Path, source_path: &str) -> Result<()> {
    let path = Path::new(source_path);
    if !path.exists() {
        anyhow::bail!("file not found: {}", source_path);
    }

    let name = parc_core::schema::add_schema(vault, path)?;
    println!("Added schema '{}'", name);
    Ok(())
}
