#[cfg(feature = "wasm-plugins")]
pub mod host;
#[cfg(feature = "wasm-plugins")]
pub mod manager;
#[cfg(feature = "wasm-plugins")]
pub mod runtime;

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ParcError;

/// Plugin manifest parsed from a TOML file alongside the .wasm binary.
#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    /// Path to .wasm file, relative to the manifest file's directory.
    pub wasm: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub read_fragments: bool,
    #[serde(default)]
    pub write_fragments: bool,
    #[serde(default)]
    pub extend_cli: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub render: Vec<String>,
    #[serde(default)]
    pub validate: Vec<String>,
}

impl PluginCapabilities {
    /// Check if this plugin wants to handle the given hook event.
    pub fn allows_hook(&self, event: &str) -> bool {
        self.hooks.iter().any(|h| h == event || h == "*")
    }

    /// Check if this plugin can render the given fragment type.
    pub fn allows_render(&self, type_name: &str) -> bool {
        self.render.iter().any(|r| r == type_name || r == "*")
    }

    /// Check if this plugin can validate the given fragment type.
    pub fn allows_validate(&self, type_name: &str) -> bool {
        self.validate.iter().any(|v| v == type_name || v == "*")
    }
}

/// A discovered plugin: its manifest and the directory it lives in.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    pub manifest_path: PathBuf,
    pub wasm_path: PathBuf,
}

/// Load a plugin manifest from a TOML file.
pub fn load_manifest(path: &Path) -> Result<PluginManifest, ParcError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ParcError::PluginError(format!("failed to read manifest {}: {}", path.display(), e)))?;
    let manifest: PluginManifest = toml::from_str(&content)
        .map_err(|e| ParcError::PluginError(format!("failed to parse manifest {}: {}", path.display(), e)))?;
    Ok(manifest)
}

/// Scan the vault's `plugins/` directory for `.toml` manifest files.
pub fn discover_plugins(vault: &Path) -> Result<Vec<DiscoveredPlugin>, ParcError> {
    let plugins_dir = vault.join("plugins");
    if !plugins_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut discovered = Vec::new();
    let entries = std::fs::read_dir(&plugins_dir)
        .map_err(|e| ParcError::PluginError(format!("failed to read plugins dir: {}", e)))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            match load_manifest(&path) {
                Ok(manifest) => {
                    let dir = path.parent().unwrap_or(&plugins_dir);
                    let wasm_path = dir.join(&manifest.plugin.wasm);
                    discovered.push(DiscoveredPlugin {
                        manifest,
                        manifest_path: path,
                        wasm_path,
                    });
                }
                Err(e) => {
                    eprintln!("Warning: skipping plugin manifest {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(discovered)
}

/// Validate a manifest: name non-empty, wasm file exists, hook names valid.
pub fn validate_manifest(manifest: &PluginManifest, vault: &Path) -> Result<(), ParcError> {
    if manifest.plugin.name.is_empty() {
        return Err(ParcError::PluginError("plugin name cannot be empty".into()));
    }

    let plugins_dir = vault.join("plugins");
    let wasm_path = plugins_dir.join(&manifest.plugin.wasm);
    if !wasm_path.exists() {
        return Err(ParcError::PluginError(format!(
            "wasm file not found: {}",
            wasm_path.display()
        )));
    }

    let valid_hooks = [
        "pre-create", "post-create", "pre-update", "post-update",
        "pre-delete", "post-delete", "*",
    ];
    for hook in &manifest.capabilities.hooks {
        if !valid_hooks.contains(&hook.as_str()) {
            return Err(ParcError::PluginError(format!(
                "invalid hook event: {}",
                hook
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let toml_str = r#"
[plugin]
name = "echo"
version = "0.1.0"
description = "Echo plugin"
wasm = "echo.wasm"

[capabilities]
read_fragments = true
hooks = ["post-create"]
render = ["note"]
validate = ["*"]
"#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.name, "echo");
        assert!(manifest.capabilities.read_fragments);
        assert!(!manifest.capabilities.write_fragments);
        assert!(manifest.capabilities.allows_hook("post-create"));
        assert!(!manifest.capabilities.allows_hook("pre-create"));
        assert!(manifest.capabilities.allows_render("note"));
        assert!(!manifest.capabilities.allows_render("todo"));
        assert!(manifest.capabilities.allows_validate("anything"));
    }

    #[test]
    fn test_discover_plugins_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let plugins = discover_plugins(&vault).unwrap();
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_discover_plugins_with_manifest() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let plugins_dir = vault.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(
            plugins_dir.join("test.toml"),
            r#"
[plugin]
name = "test"
version = "0.1.0"
wasm = "test.wasm"
"#,
        )
        .unwrap();

        let plugins = discover_plugins(&vault).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.plugin.name, "test");
    }

    #[test]
    fn test_validate_manifest_missing_wasm() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let manifest = PluginManifest {
            plugin: PluginMeta {
                name: "test".into(),
                version: "0.1.0".into(),
                description: "".into(),
                wasm: "missing.wasm".into(),
            },
            capabilities: PluginCapabilities::default(),
        };
        assert!(validate_manifest(&manifest, &vault).is_err());
    }

    #[test]
    fn test_validate_manifest_invalid_hook() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let plugins_dir = vault.join("plugins");
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(plugins_dir.join("test.wasm"), b"fake").unwrap();

        let manifest = PluginManifest {
            plugin: PluginMeta {
                name: "test".into(),
                version: "0.1.0".into(),
                description: "".into(),
                wasm: "test.wasm".into(),
            },
            capabilities: PluginCapabilities {
                hooks: vec!["invalid-event".into()],
                ..Default::default()
            },
        };
        assert!(validate_manifest(&manifest, &vault).is_err());
    }
}
