use std::path::Path;

use anyhow::Result;
use parc_core::vault::{self, VaultScope};

pub enum VaultSubcommand {
    List { json: bool },
}

pub fn run(vault: &Path, subcommand: Option<VaultSubcommand>, json: bool) -> Result<()> {
    match subcommand {
        Some(VaultSubcommand::List { json }) => run_list(vault, json),
        None => run_info(vault, json),
    }
}

fn run_info(vault: &Path, json: bool) -> Result<()> {
    let info = vault::vault_info(vault)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("Active vault: {}", info.path.display());
        println!("Scope:        {}", info.scope);
        println!("Fragments:    {}", info.fragment_count);
    }

    Ok(())
}

fn run_list(active_vault: &Path, json: bool) -> Result<()> {
    let vaults = vault::discover_all_vaults()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&vaults)?);
        return Ok(());
    }

    if vaults.is_empty() {
        println!("No vaults found.");
        return Ok(());
    }

    println!("{:<10} {:<50} FRAGMENTS", "SCOPE", "PATH");

    for v in &vaults {
        let active_marker = if v.path == active_vault { " *" } else { "" };
        let scope_str = match v.scope {
            VaultScope::Local => format!("local{}", active_marker),
            VaultScope::Global => format!("global{}", active_marker),
        };
        println!(
            "{:<10} {:<50} {}",
            scope_str,
            v.path.display(),
            v.fragment_count
        );
    }

    Ok(())
}
