use std::path::Path;

use anyhow::{bail, Result};
use parc_core::export;
use parc_core::fragment;
use parc_core::index::open_index;
use parc_core::search::{self, parse_query, SortOrder};
use parc_core::secure_fs;

pub fn run(
    vault: &Path,
    format: &str,
    output: Option<&str>,
    query_terms: Vec<String>,
) -> Result<()> {
    // Collect fragments via search or list all
    let fragments = if query_terms.is_empty() {
        // List all fragments
        let ids = fragment::list_fragment_ids(vault)?;
        let mut frags = Vec::new();
        for id in &ids {
            if let Ok(f) = fragment::read_fragment(vault, id) {
                frags.push(f);
            }
        }
        frags
    } else {
        let query_str = query_terms.join(" ");
        let mut query = parse_query(&query_str)?;
        query.sort = SortOrder::UpdatedDesc;
        let conn = open_index(vault)?;
        let results = search::search(&conn, &query)?;
        // Read full fragments from search results
        let mut frags = Vec::new();
        for r in &results {
            if let Ok(f) = fragment::read_fragment(vault, &r.id) {
                frags.push(f);
            }
        }
        frags
    };

    match format {
        "json" => {
            let content = export::export_json(&fragments)?;
            write_output(output, &content)?;
        }
        "csv" => {
            let content = export::export_csv(&fragments)?;
            write_output(output, &content)?;
        }
        "html" => {
            let files = export::export_html(&fragments)?;
            let dir = output.unwrap_or("parc-export");
            secure_fs::create_private_dir_all(Path::new(dir))?;
            for (filename, content) in &files {
                let path = Path::new(dir).join(filename);
                secure_fs::write_private(&path, content)?;
            }
            println!("Exported {} files to {}/", files.len(), dir);
            return Ok(());
        }
        _ => bail!("unknown format '{}': expected json, csv, or html", format),
    }

    Ok(())
}

fn write_output(output: Option<&str>, content: &str) -> Result<()> {
    if let Some(path) = output {
        secure_fs::write_private(Path::new(path), content)?;
        println!("Exported to {}", path);
    } else {
        print!("{}", content);
    }
    Ok(())
}
