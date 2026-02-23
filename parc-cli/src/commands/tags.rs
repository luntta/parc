use std::path::Path;

use anyhow::Result;
use parc_core::index::open_index;
use parc_core::tag;

pub fn run(vault: &Path, json: bool) -> Result<()> {
    let conn = open_index(vault)?;
    let tags = tag::aggregate_tags(&conn)?;

    if json {
        let json_val: Vec<serde_json::Value> = tags
            .iter()
            .map(|t| {
                serde_json::json!({
                    "tag": t.tag,
                    "count": t.count,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else if tags.is_empty() {
        println!("No tags found.");
    } else {
        println!("{:<30}  {:>5}", "TAG", "COUNT");
        for t in &tags {
            println!("{:<30}  {:>5}", t.tag, t.count);
        }
    }

    Ok(())
}
