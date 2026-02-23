use std::path::{Path, PathBuf};

use wasmtime::*;

use crate::error::ParcError;

use super::PluginManifest;

/// Per-instance state stored in the wasmtime Store.
pub struct PluginState {
    pub manifest: PluginManifest,
    pub output_buffer: Vec<u8>,
    pub vault_path: PathBuf,
    pub config_json: String,
}

/// Wraps a wasmtime Engine for compiling and loading plugins.
pub struct WasmRuntime {
    engine: Engine,
}

/// A loaded, instantiated plugin ready to receive calls.
pub struct PluginInstance {
    pub store: Store<PluginState>,
    instance: Instance,
    pub manifest: PluginManifest,
}

/// Result of a validation call.
#[derive(Debug)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

impl WasmRuntime {
    pub fn new() -> Result<Self, ParcError> {
        let engine = Engine::default();
        Ok(WasmRuntime { engine })
    }

    /// Load and instantiate a plugin from its manifest and wasm bytes.
    pub fn load_plugin(
        &self,
        manifest: PluginManifest,
        wasm_bytes: &[u8],
        vault_path: &Path,
        config_json: &str,
    ) -> Result<PluginInstance, ParcError> {
        let module = Module::new(&self.engine, wasm_bytes).map_err(|e| {
            ParcError::PluginError(format!(
                "failed to compile plugin '{}': {}",
                manifest.plugin.name, e
            ))
        })?;

        let state = PluginState {
            manifest: manifest.clone(),
            output_buffer: Vec::new(),
            vault_path: vault_path.to_path_buf(),
            config_json: config_json.to_string(),
        };

        let mut store = Store::new(&self.engine, state);
        let mut linker = Linker::new(&self.engine);

        // Register host functions
        super::host::register_host_functions(&mut linker)?;

        let instance = linker.instantiate(&mut store, &module).map_err(|e| {
            ParcError::PluginError(format!(
                "failed to instantiate plugin '{}': {}",
                manifest.plugin.name, e
            ))
        })?;

        // Call parc_plugin_init if it exists
        if let Ok(init_fn) = instance.get_typed_func::<(i32, i32), i32>(&mut store, "parc_plugin_init") {
            let config_bytes = config_json.as_bytes();
            let (ptr, len) = write_to_guest(&mut store, &instance, config_bytes)?;
            let _result = init_fn.call(&mut store, (ptr, len)).map_err(|e| {
                ParcError::PluginError(format!(
                    "plugin '{}' init failed: {}",
                    manifest.plugin.name, e
                ))
            })?;
        }

        Ok(PluginInstance {
            store,
            instance,
            manifest,
        })
    }
}

impl PluginInstance {
    /// Call an event handler: `parc_on_event(event_ptr, event_len, fragment_ptr, fragment_len) -> i32`
    pub fn call_event(
        &mut self,
        event: &str,
        fragment_json: &str,
    ) -> Result<Option<String>, ParcError> {
        let func = match self
            .instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut self.store, "parc_on_event")
        {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        self.store.data_mut().output_buffer.clear();

        let (event_ptr, event_len) =
            write_to_guest(&mut self.store, &self.instance, event.as_bytes())?;
        let (frag_ptr, frag_len) =
            write_to_guest(&mut self.store, &self.instance, fragment_json.as_bytes())?;

        let result = func
            .call(&mut self.store, (event_ptr, event_len, frag_ptr, frag_len))
            .map_err(|e| {
                ParcError::PluginError(format!(
                    "plugin '{}' event handler failed: {}",
                    self.manifest.plugin.name, e
                ))
            })?;

        let _ = free_from_guest(&mut self.store, &self.instance, event_ptr, event_len);
        let _ = free_from_guest(&mut self.store, &self.instance, frag_ptr, frag_len);

        if result == 0 {
            Ok(None)
        } else {
            let output = String::from_utf8_lossy(&self.store.data().output_buffer).to_string();
            if output.is_empty() {
                Ok(None)
            } else {
                Ok(Some(output))
            }
        }
    }

    /// Call a validation handler: `parc_validate(fragment_ptr, fragment_len) -> i32`
    pub fn call_validate(
        &mut self,
        fragment_json: &str,
    ) -> Result<ValidationResult, ParcError> {
        let func = match self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "parc_validate")
        {
            Ok(f) => f,
            Err(_) => {
                return Ok(ValidationResult {
                    valid: true,
                    errors: vec![],
                })
            }
        };

        self.store.data_mut().output_buffer.clear();

        let (ptr, len) =
            write_to_guest(&mut self.store, &self.instance, fragment_json.as_bytes())?;

        let result = func.call(&mut self.store, (ptr, len)).map_err(|e| {
            ParcError::PluginError(format!(
                "plugin '{}' validate failed: {}",
                self.manifest.plugin.name, e
            ))
        })?;

        let _ = free_from_guest(&mut self.store, &self.instance, ptr, len);

        if result == 0 {
            Ok(ValidationResult {
                valid: true,
                errors: vec![],
            })
        } else {
            let output = String::from_utf8_lossy(&self.store.data().output_buffer).to_string();
            let errors: Vec<String> = if output.is_empty() {
                vec!["validation failed".into()]
            } else {
                output.lines().map(|l| l.to_string()).collect()
            };
            Ok(ValidationResult {
                valid: false,
                errors,
            })
        }
    }

    /// Call a render handler: `parc_render(fragment_ptr, fragment_len) -> i32`
    pub fn call_render(
        &mut self,
        fragment_json: &str,
    ) -> Result<Option<String>, ParcError> {
        let func = match self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "parc_render")
        {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        self.store.data_mut().output_buffer.clear();

        let (ptr, len) =
            write_to_guest(&mut self.store, &self.instance, fragment_json.as_bytes())?;

        let _result = func.call(&mut self.store, (ptr, len)).map_err(|e| {
            ParcError::PluginError(format!(
                "plugin '{}' render failed: {}",
                self.manifest.plugin.name, e
            ))
        })?;

        let _ = free_from_guest(&mut self.store, &self.instance, ptr, len);

        let output = String::from_utf8_lossy(&self.store.data().output_buffer).to_string();
        if output.is_empty() {
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }

    /// Call a command handler: `parc_command(cmd_ptr, cmd_len, args_ptr, args_len) -> i32`
    pub fn call_command(
        &mut self,
        cmd: &str,
        args_json: &str,
    ) -> Result<String, ParcError> {
        let func = self
            .instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut self.store, "parc_command")
            .map_err(|_| {
                ParcError::PluginError(format!(
                    "plugin '{}' does not export parc_command",
                    self.manifest.plugin.name
                ))
            })?;

        self.store.data_mut().output_buffer.clear();

        let (cmd_ptr, cmd_len) =
            write_to_guest(&mut self.store, &self.instance, cmd.as_bytes())?;
        let (args_ptr, args_len) =
            write_to_guest(&mut self.store, &self.instance, args_json.as_bytes())?;

        func.call(&mut self.store, (cmd_ptr, cmd_len, args_ptr, args_len))
            .map_err(|e| {
                ParcError::PluginError(format!(
                    "plugin '{}' command '{}' failed: {}",
                    self.manifest.plugin.name, cmd, e
                ))
            })?;

        let _ = free_from_guest(&mut self.store, &self.instance, cmd_ptr, cmd_len);
        let _ = free_from_guest(&mut self.store, &self.instance, args_ptr, args_len);

        let output = String::from_utf8_lossy(&self.store.data().output_buffer).to_string();
        Ok(output)
    }
}

/// Allocate guest memory and write bytes into it.
fn write_to_guest(
    store: &mut Store<PluginState>,
    instance: &Instance,
    data: &[u8],
) -> Result<(i32, i32), ParcError> {
    let alloc_fn = instance
        .get_typed_func::<i32, i32>(&mut *store, "parc_alloc")
        .map_err(|_| ParcError::PluginError("plugin does not export parc_alloc".into()))?;

    let len = data.len() as i32;
    let ptr = alloc_fn.call(&mut *store, len).map_err(|e| {
        ParcError::PluginError(format!("parc_alloc failed: {}", e))
    })?;

    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| ParcError::PluginError("plugin has no 'memory' export".into()))?;

    memory.data_mut(&mut *store)[ptr as usize..ptr as usize + data.len()].copy_from_slice(data);

    Ok((ptr, len))
}

/// Free guest memory.
fn free_from_guest(
    store: &mut Store<PluginState>,
    instance: &Instance,
    ptr: i32,
    len: i32,
) -> Result<(), ParcError> {
    if let Ok(free_fn) = instance.get_typed_func::<(i32, i32), ()>(&mut *store, "parc_free") {
        free_fn.call(&mut *store, (ptr, len)).map_err(|e| {
            ParcError::PluginError(format!("parc_free failed: {}", e))
        })?;
    }
    Ok(())
}
