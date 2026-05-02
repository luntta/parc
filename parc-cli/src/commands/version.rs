use anyhow::Result;

use super::update::GITHUB_REPO;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run(json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "name": "parc",
                "version": CURRENT_VERSION,
                "repo": GITHUB_REPO,
            }))?
        );
    } else {
        println!("parc {}", CURRENT_VERSION);
    }

    Ok(())
}
