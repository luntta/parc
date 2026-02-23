use std::path::Path;

use crate::config::Config;
use crate::error::ParcError;
use crate::fragment::Fragment;
use crate::hook::HookEvent;

use super::runtime::{PluginInstance, ValidationResult, WasmRuntime};
use super::discover_plugins;

/// Describes a command provided by a plugin.
#[derive(Debug, Clone)]
pub struct PluginCommand {
    pub plugin_name: String,
    pub command: String,
    pub description: String,
}

/// Manages all loaded plugin instances and dispatches calls to them.
pub struct PluginManager {
    pub plugins: Vec<PluginInstance>,
    runtime: WasmRuntime,
}

impl PluginManager {
    /// Discover and load all plugins from the vault's plugins/ directory.
    /// Plugins that fail to load are skipped with a warning.
    pub fn load_all(vault: &Path, config: &Config) -> Result<Self, ParcError> {
        let runtime = WasmRuntime::new()?;
        let discovered = discover_plugins(vault)?;
        let mut plugins = Vec::new();

        for disc in discovered {
            if !disc.wasm_path.exists() {
                eprintln!(
                    "Warning: plugin '{}' wasm not found at {}",
                    disc.manifest.plugin.name,
                    disc.wasm_path.display()
                );
                continue;
            }

            let wasm_bytes = match std::fs::read(&disc.wasm_path) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!(
                        "Warning: failed to read plugin '{}' wasm: {}",
                        disc.manifest.plugin.name, e
                    );
                    continue;
                }
            };

            let config_json = plugin_config_json(config, &disc.manifest.plugin.name);

            match runtime.load_plugin(disc.manifest.clone(), &wasm_bytes, vault, &config_json) {
                Ok(instance) => plugins.push(instance),
                Err(e) => {
                    eprintln!(
                        "Warning: failed to load plugin '{}': {}",
                        disc.manifest.plugin.name, e
                    );
                }
            }
        }

        Ok(PluginManager { plugins, runtime })
    }

    /// Create an empty plugin manager (no plugins loaded).
    pub fn empty() -> Result<Self, ParcError> {
        Ok(PluginManager {
            plugins: Vec::new(),
            runtime: WasmRuntime::new()?,
        })
    }

    /// Dispatch a pre-hook event. Plugins with matching hook capabilities
    /// are called in order. Each may modify the fragment.
    pub fn dispatch_pre_event(
        &mut self,
        event: HookEvent,
        fragment: &Fragment,
    ) -> Result<Fragment, ParcError> {
        let event_str = event.prefix();
        let mut current = fragment.clone();

        for plugin in &mut self.plugins {
            if !plugin.manifest.capabilities.allows_hook(event_str) {
                continue;
            }

            let frag_json = serde_json::to_string(&current).map_err(|e| {
                ParcError::PluginError(format!("failed to serialize fragment: {}", e))
            })?;

            if let Some(output) = plugin.call_event(event_str, &frag_json)? {
                // Try to parse the output as a modified fragment
                if let Ok(modified) = serde_json::from_str::<Fragment>(&output) {
                    current = modified;
                }
            }
        }

        Ok(current)
    }

    /// Dispatch a post-hook event. Errors from individual plugins are logged but not propagated.
    pub fn dispatch_post_event(&mut self, event: HookEvent, fragment: &Fragment) {
        let event_str = event.prefix();

        for plugin in &mut self.plugins {
            if !plugin.manifest.capabilities.allows_hook(event_str) {
                continue;
            }

            let frag_json = match serde_json::to_string(fragment) {
                Ok(j) => j,
                Err(_) => continue,
            };

            if let Err(e) = plugin.call_event(event_str, &frag_json) {
                eprintln!(
                    "Warning: plugin '{}' post-event failed: {}",
                    plugin.manifest.plugin.name, e
                );
            }
        }
    }

    /// Run validation across all plugins with matching validate capability.
    pub fn validate(&mut self, fragment: &Fragment) -> Result<Vec<String>, ParcError> {
        let mut errors = Vec::new();

        for plugin in &mut self.plugins {
            if !plugin
                .manifest
                .capabilities
                .allows_validate(&fragment.fragment_type)
            {
                continue;
            }

            let frag_json = serde_json::to_string(fragment).map_err(|e| {
                ParcError::PluginError(format!("failed to serialize fragment: {}", e))
            })?;

            let result: ValidationResult = plugin.call_validate(&frag_json)?;
            if !result.valid {
                for err in result.errors {
                    errors.push(format!("[{}] {}", plugin.manifest.plugin.name, err));
                }
            }
        }

        Ok(errors)
    }

    /// Try to render a fragment using a plugin. Returns the first successful render.
    pub fn render(&mut self, fragment: &Fragment) -> Result<Option<String>, ParcError> {
        for plugin in &mut self.plugins {
            if !plugin
                .manifest
                .capabilities
                .allows_render(&fragment.fragment_type)
            {
                continue;
            }

            let frag_json = serde_json::to_string(fragment).map_err(|e| {
                ParcError::PluginError(format!("failed to serialize fragment: {}", e))
            })?;

            if let Some(rendered) = plugin.call_render(&frag_json)? {
                return Ok(Some(rendered));
            }
        }

        Ok(None)
    }

    /// List all commands provided by loaded plugins.
    pub fn list_commands(&self) -> Vec<PluginCommand> {
        let mut commands = Vec::new();
        for plugin in &self.plugins {
            for cmd in &plugin.manifest.capabilities.extend_cli {
                commands.push(PluginCommand {
                    plugin_name: plugin.manifest.plugin.name.clone(),
                    command: cmd.clone(),
                    description: plugin.manifest.plugin.description.clone(),
                });
            }
        }
        commands
    }

    /// Execute a plugin command by plugin name and command name.
    pub fn execute_command(
        &mut self,
        plugin_name: &str,
        cmd: &str,
        args: &[String],
    ) -> Result<String, ParcError> {
        let plugin = self
            .plugins
            .iter_mut()
            .find(|p| p.manifest.plugin.name == plugin_name)
            .ok_or_else(|| {
                ParcError::PluginError(format!("plugin '{}' not found", plugin_name))
            })?;

        let args_json = serde_json::to_string(args).unwrap_or_else(|_| "[]".into());
        plugin.call_command(cmd, &args_json)
    }

    /// Get a reference to the runtime (for loading additional plugins).
    pub fn runtime(&self) -> &WasmRuntime {
        &self.runtime
    }
}

/// Extract plugin-specific config as JSON string.
fn plugin_config_json(config: &Config, plugin_name: &str) -> String {
    if let Some(val) = config.plugins.get(plugin_name) {
        serde_json::to_string(val).unwrap_or_else(|_| "{}".into())
    } else {
        "{}".into()
    }
}
