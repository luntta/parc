use std::path::Path;

use anyhow::Result;
use parc_core::plugin;
use parc_core::secure_fs;

pub fn run_list(vault: &Path, json: bool) -> Result<()> {
    let discovered = plugin::discover_plugins(vault)?;

    if json {
        let items: Vec<serde_json::Value> = discovered
            .iter()
            .map(|d| {
                let caps = &d.manifest.capabilities;
                serde_json::json!({
                    "name": d.manifest.plugin.name,
                    "version": d.manifest.plugin.version,
                    "description": d.manifest.plugin.description,
                    "wasm": d.manifest.plugin.wasm,
                    "wasm_exists": d.wasm_path.exists(),
                    "capabilities": {
                        "read_fragments": caps.read_fragments,
                        "write_fragments": caps.write_fragments,
                        "extend_cli": caps.extend_cli,
                        "hooks": caps.hooks,
                        "render": caps.render,
                        "validate": caps.validate,
                    }
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if discovered.is_empty() {
        println!("No plugins installed.");
    } else {
        println!(
            "{:<20} {:<10} {:<30} {}",
            "NAME", "VERSION", "CAPABILITIES", "DESCRIPTION"
        );
        for d in &discovered {
            let caps = format_caps(&d.manifest.capabilities);
            println!(
                "{:<20} {:<10} {:<30} {}",
                d.manifest.plugin.name,
                d.manifest.plugin.version,
                caps,
                d.manifest.plugin.description,
            );
        }
    }

    Ok(())
}

pub fn run_info(vault: &Path, name: &str, json: bool) -> Result<()> {
    let discovered = plugin::discover_plugins(vault)?;
    let found = discovered
        .iter()
        .find(|d| d.manifest.plugin.name == name)
        .ok_or_else(|| anyhow::anyhow!("plugin '{}' not found", name))?;

    let m = &found.manifest;
    let caps = &m.capabilities;

    if json {
        let val = serde_json::json!({
            "name": m.plugin.name,
            "version": m.plugin.version,
            "description": m.plugin.description,
            "wasm": m.plugin.wasm,
            "manifest_path": found.manifest_path.display().to_string(),
            "wasm_path": found.wasm_path.display().to_string(),
            "wasm_exists": found.wasm_path.exists(),
            "capabilities": {
                "read_fragments": caps.read_fragments,
                "write_fragments": caps.write_fragments,
                "extend_cli": caps.extend_cli,
                "hooks": caps.hooks,
                "render": caps.render,
                "validate": caps.validate,
            }
        });
        println!("{}", serde_json::to_string_pretty(&val)?);
    } else {
        println!("Name:        {}", m.plugin.name);
        println!("Version:     {}", m.plugin.version);
        println!("Description: {}", m.plugin.description);
        println!("WASM:        {}", found.wasm_path.display());
        println!("WASM exists: {}", found.wasm_path.exists());
        println!("Manifest:    {}", found.manifest_path.display());
        println!();
        println!("Capabilities:");
        println!("  read_fragments:  {}", caps.read_fragments);
        println!("  write_fragments: {}", caps.write_fragments);
        if !caps.extend_cli.is_empty() {
            println!("  extend_cli:      {}", caps.extend_cli.join(", "));
        }
        if !caps.hooks.is_empty() {
            println!("  hooks:           {}", caps.hooks.join(", "));
        }
        if !caps.render.is_empty() {
            println!("  render:          {}", caps.render.join(", "));
        }
        if !caps.validate.is_empty() {
            println!("  validate:        {}", caps.validate.join(", "));
        }
    }

    Ok(())
}

pub fn run_install(vault: &Path, wasm_path: &str, manifest_path: Option<&str>) -> Result<()> {
    let wasm_src = std::path::Path::new(wasm_path);
    if !wasm_src.exists() {
        anyhow::bail!("wasm file not found: {}", wasm_path);
    }

    // Determine manifest path: explicit or same name with .toml extension
    let manifest_src = if let Some(mp) = manifest_path {
        std::path::PathBuf::from(mp)
    } else {
        wasm_src.with_extension("toml")
    };

    if !manifest_src.exists() {
        anyhow::bail!(
            "manifest not found: {} (use --manifest to specify)",
            manifest_src.display()
        );
    }

    // Parse and validate manifest
    let manifest = plugin::load_manifest(&manifest_src)?;
    plugin::validate_manifest_metadata(&manifest)?;

    let plugins_dir = vault.join("plugins");
    secure_fs::create_private_dir_all(&plugins_dir)?;

    // Copy files
    let dest_wasm = plugin::resolve_plugin_wasm_path(&manifest, &plugins_dir)?;
    let dest_manifest = plugins_dir.join(plugin::plugin_manifest_filename(&manifest.plugin.name)?);

    if dest_wasm.exists() || dest_manifest.exists() {
        anyhow::bail!("plugin '{}' is already installed", manifest.plugin.name);
    }

    secure_fs::copy_private_new(wasm_src, &dest_wasm)?;
    secure_fs::copy_private_new(&manifest_src, &dest_manifest)?;

    // Validate the installed plugin
    if let Err(e) = plugin::validate_manifest(&manifest, vault) {
        // Clean up on failure
        let _ = std::fs::remove_file(&dest_wasm);
        let _ = std::fs::remove_file(&dest_manifest);
        anyhow::bail!("plugin validation failed: {}", e);
    }

    println!(
        "Installed plugin '{}' v{}",
        manifest.plugin.name, manifest.plugin.version
    );
    Ok(())
}

pub fn run_remove(vault: &Path, name: &str, force: bool) -> Result<()> {
    let discovered = plugin::discover_plugins(vault)?;
    let found = discovered
        .iter()
        .find(|d| d.manifest.plugin.name == name)
        .ok_or_else(|| anyhow::anyhow!("plugin '{}' not found", name))?;

    if !force {
        eprintln!(
            "Removing plugin '{}' v{}",
            found.manifest.plugin.name, found.manifest.plugin.version
        );
    }

    // Remove wasm file
    if found.wasm_path.exists() {
        std::fs::remove_file(&found.wasm_path)?;
    }

    // Remove manifest
    if found.manifest_path.exists() {
        std::fs::remove_file(&found.manifest_path)?;
    }

    println!("Removed plugin '{}'", name);
    Ok(())
}

fn format_caps(caps: &plugin::PluginCapabilities) -> String {
    let mut parts = Vec::new();
    if caps.read_fragments {
        parts.push("read");
    }
    if caps.write_fragments {
        parts.push("write");
    }
    if !caps.extend_cli.is_empty() {
        parts.push("cli");
    }
    if !caps.hooks.is_empty() {
        parts.push("hooks");
    }
    if !caps.render.is_empty() {
        parts.push("render");
    }
    if !caps.validate.is_empty() {
        parts.push("validate");
    }
    parts.join(", ")
}
