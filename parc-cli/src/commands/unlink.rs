use anyhow::Result;
use chrono::Utc;
use parc_core::fragment::{read_fragment, write_fragment};
use parc_core::index;
use parc_core::vault::discover_vault;

pub fn run(id_a: &str, id_b: &str) -> Result<()> {
    let vault = discover_vault()?;

    let mut frag_a = read_fragment(&vault, id_a)?;
    let mut frag_b = read_fragment(&vault, id_b)?;

    let a_had_b = frag_a.links.contains(&frag_b.id);
    let b_had_a = frag_b.links.contains(&frag_a.id);

    if !a_had_b && !b_had_a {
        println!("Not linked.");
        return Ok(());
    }

    if a_had_b {
        frag_a.links.retain(|l| l != &frag_b.id);
        frag_a.updated_at = Utc::now();
        write_fragment(&vault, &frag_a)?;
    }

    if b_had_a {
        frag_b.links.retain(|l| l != &frag_a.id);
        frag_b.updated_at = Utc::now();
        write_fragment(&vault, &frag_b)?;
    }

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &frag_a, &vault)?;
    index::index_fragment_auto(&conn, &frag_b, &vault)?;

    println!(
        "Unlinked {} \u{2194} {}",
        &frag_a.id[..8],
        &frag_b.id[..8]
    );
    Ok(())
}
