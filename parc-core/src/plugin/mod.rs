#[cfg(feature = "wasm-plugins")]
pub mod host;
#[cfg(feature = "wasm-plugins")]
pub mod manager;
#[cfg(feature = "wasm-plugins")]
pub mod runtime;

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ParcError;

const MAX_PLUGIN_NAME_LEN: usize = 64;
const MAX_PLUGIN_CAPABILITY_NAME_LEN: usize = 64;

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
    let content = std::fs::read_to_string(path).map_err(|e| {
        ParcError::PluginError(format!("failed to read manifest {}: {}", path.display(), e))
    })?;
    let manifest: PluginManifest = toml::from_str(&content).map_err(|e| {
        ParcError::PluginError(format!(
            "failed to parse manifest {}: {}",
            path.display(),
            e
        ))
    })?;
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
            if is_symlink(&path)? {
                eprintln!(
                    "Warning: skipping symlinked plugin manifest {}",
                    path.display()
                );
                continue;
            }
            match load_manifest(&path) {
                Ok(manifest) => match validate_manifest(&manifest, vault) {
                    Ok(()) => {
                        let wasm_path = resolve_plugin_wasm_path(&manifest, &plugins_dir)?;
                        discovered.push(DiscoveredPlugin {
                            manifest,
                            manifest_path: path,
                            wasm_path,
                        });
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: skipping invalid plugin manifest {}: {}",
                            path.display(),
                            e
                        );
                    }
                },
                Err(e) => {
                    eprintln!(
                        "Warning: skipping plugin manifest {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    Ok(discovered)
}

/// Validate a manifest: safe identifiers, wasm file exists, hook names valid.
pub fn validate_manifest(manifest: &PluginManifest, vault: &Path) -> Result<(), ParcError> {
    validate_manifest_metadata(manifest)?;

    let plugins_dir = vault.join("plugins");
    let wasm_path = resolve_plugin_wasm_path(manifest, &plugins_dir)?;
    let metadata = std::fs::symlink_metadata(&wasm_path).map_err(|e| {
        ParcError::PluginError(format!(
            "failed to read plugin wasm metadata {}: {}",
            wasm_path.display(),
            e
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ParcError::PluginError(format!(
            "wasm file is not a regular file: {}",
            wasm_path.display()
        )));
    }

    Ok(())
}

pub fn validate_manifest_metadata(manifest: &PluginManifest) -> Result<(), ParcError> {
    validate_plugin_name(&manifest.plugin.name)?;
    validate_wasm_filename(&manifest.plugin.wasm)?;

    let valid_hooks = [
        "pre-create",
        "post-create",
        "pre-update",
        "post-update",
        "pre-delete",
        "post-delete",
        "*",
    ];
    for hook in &manifest.capabilities.hooks {
        if !valid_hooks.contains(&hook.as_str()) {
            return Err(ParcError::PluginError(format!(
                "invalid hook event: {}",
                hook
            )));
        }
    }

    for cmd in &manifest.capabilities.extend_cli {
        validate_capability_name(cmd, "CLI command")?;
    }
    for type_name in &manifest.capabilities.render {
        validate_capability_filter(type_name, "render")?;
    }
    for type_name in &manifest.capabilities.validate {
        validate_capability_filter(type_name, "validate")?;
    }

    Ok(())
}

pub fn plugin_manifest_filename(name: &str) -> Result<String, ParcError> {
    validate_plugin_name(name)?;
    Ok(format!("{}.toml", name))
}

pub fn resolve_plugin_wasm_path(
    manifest: &PluginManifest,
    plugins_dir: &Path,
) -> Result<PathBuf, ParcError> {
    validate_wasm_filename(&manifest.plugin.wasm)?;
    Ok(plugins_dir.join(&manifest.plugin.wasm))
}

fn validate_plugin_name(name: &str) -> Result<(), ParcError> {
    validate_identifier(name, "plugin name", MAX_PLUGIN_NAME_LEN)
}

fn validate_capability_filter(value: &str, label: &str) -> Result<(), ParcError> {
    if value == "*" {
        return Ok(());
    }
    validate_capability_name(value, label)
}

fn validate_capability_name(value: &str, label: &str) -> Result<(), ParcError> {
    validate_identifier(value, label, MAX_PLUGIN_CAPABILITY_NAME_LEN)
}

fn validate_identifier(value: &str, label: &str, max_len: usize) -> Result<(), ParcError> {
    if value.is_empty() || value.len() > max_len {
        return Err(ParcError::PluginError(format!(
            "{} must be 1-{} characters",
            label, max_len
        )));
    }
    let first = value.as_bytes()[0];
    if !first.is_ascii_alphanumeric() {
        return Err(ParcError::PluginError(format!(
            "{} '{}' must start with a letter or digit",
            label, value
        )));
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(ParcError::PluginError(format!(
            "{} '{}' must contain only letters, digits, '_' or '-'",
            label, value
        )));
    }
    Ok(())
}

fn validate_wasm_filename(wasm: &str) -> Result<(), ParcError> {
    if wasm.is_empty() {
        return Err(ParcError::PluginError(
            "plugin wasm path cannot be empty".into(),
        ));
    }
    if wasm.contains('/') || wasm.contains('\\') || wasm.contains('\0') {
        return Err(ParcError::PluginError(format!(
            "plugin wasm '{}' must be a file name, not a path",
            wasm
        )));
    }
    let path = Path::new(wasm);
    if path.file_name().and_then(|n| n.to_str()) != Some(wasm) {
        return Err(ParcError::PluginError(format!(
            "invalid plugin wasm file name '{}'",
            wasm
        )));
    }
    if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
        return Err(ParcError::PluginError(format!(
            "plugin wasm '{}' must end with .wasm",
            wasm
        )));
    }
    Ok(())
}

fn is_symlink(path: &Path) -> Result<bool, ParcError> {
    Ok(std::fs::symlink_metadata(path)
        .map_err(|e| {
            ParcError::PluginError(format!(
                "failed to read plugin path metadata {}: {}",
                path.display(),
                e
            ))
        })?
        .file_type()
        .is_symlink())
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
        std::fs::write(plugins_dir.join("test.wasm"), b"fake").unwrap();

        let plugins = discover_plugins(&vault).unwrap();
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].manifest.plugin.name, "test");
    }

    #[test]
    fn test_validate_manifest_rejects_path_traversal() {
        let manifest = PluginManifest {
            plugin: PluginMeta {
                name: "../evil".into(),
                version: "0.1.0".into(),
                description: "".into(),
                wasm: "../evil.wasm".into(),
            },
            capabilities: PluginCapabilities::default(),
        };

        assert!(validate_manifest_metadata(&manifest).is_err());
    }

    #[test]
    fn test_discover_skips_manifest_with_external_wasm_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let plugins_dir = vault.join("plugins");
        std::fs::write(
            plugins_dir.join("bad.toml"),
            r#"
[plugin]
name = "bad"
version = "0.1.0"
wasm = "../outside.wasm"
"#,
        )
        .unwrap();

        let plugins = discover_plugins(&vault).unwrap();
        assert!(plugins.is_empty());
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
