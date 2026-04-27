use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use wasmtime::*;

use crate::error::ParcError;

use super::PluginManifest;

/// Hard cap on linear-memory growth per plugin instance. Plugins that try to
/// grow past this fail their `memory.grow` and the call returns an error,
/// rather than the host being OOM-killed.
const MAX_PLUGIN_MEMORY_BYTES: usize = 64 * 1024 * 1024; // 64 MiB

/// Hard cap on table size per plugin instance.
const MAX_PLUGIN_TABLE_ELEMENTS: usize = 100_000;

/// Hard cap on bytes a plugin can return through host output buffers.
pub const MAX_PLUGIN_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MiB

/// Each guest call gets this many epoch ticks before it is interrupted. The
/// ticker thread (see `WasmRuntime::new`) increments the epoch every 100ms,
/// so a deadline of 50 ≈ 5 seconds wall clock.
const PLUGIN_CALL_EPOCH_DEADLINE: u64 = 50;

/// Per-instance limiter enforcing `MAX_PLUGIN_MEMORY_BYTES` and table size.
pub struct PluginLimits;

impl ResourceLimiter for PluginLimits {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= MAX_PLUGIN_MEMORY_BYTES)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= MAX_PLUGIN_TABLE_ELEMENTS)
    }
}

/// Per-instance state stored in the wasmtime Store.
pub struct PluginState {
    pub manifest: PluginManifest,
    pub output_buffer: Vec<u8>,
    pub output_truncated: bool,
    pub vault_path: PathBuf,
    pub config_json: String,
    pub limits: PluginLimits,
}

impl PluginState {
    pub fn clear_output(&mut self) {
        self.output_buffer.clear();
        self.output_truncated = false;
    }

    pub fn append_output(&mut self, bytes: &[u8]) -> bool {
        let remaining = MAX_PLUGIN_OUTPUT_BYTES.saturating_sub(self.output_buffer.len());
        if bytes.len() > remaining {
            self.output_buffer.extend_from_slice(&bytes[..remaining]);
            self.output_truncated = true;
            false
        } else {
            self.output_buffer.extend_from_slice(bytes);
            true
        }
    }

    pub fn replace_output(&mut self, bytes: &[u8]) -> Option<i32> {
        self.clear_output();
        if !self.append_output(bytes) {
            return None;
        }
        i32::try_from(bytes.len()).ok()
    }

    pub fn output_string(&self) -> Result<String, ParcError> {
        if self.output_truncated {
            return Err(ParcError::PluginError(format!(
                "plugin output exceeded {} bytes",
                MAX_PLUGIN_OUTPUT_BYTES
            )));
        }
        Ok(String::from_utf8_lossy(&self.output_buffer).to_string())
    }
}

/// Wraps a wasmtime Engine for compiling and loading plugins.
pub struct WasmRuntime {
    engine: Engine,
    /// Tells the epoch ticker thread to exit when the runtime drops.
    ticker_stop: Arc<AtomicBool>,
}

impl Drop for WasmRuntime {
    fn drop(&mut self) {
        self.ticker_stop.store(true, Ordering::SeqCst);
    }
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
        // Enable epoch-based interruption. Combined with the ticker thread
        // below and `set_epoch_deadline` per call, this lets us stop runaway
        // plugins (infinite loops, pathological wasm) without trusting them
        // to cooperate.
        let mut config = Config::new();
        config.epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|e| {
            ParcError::PluginError(format!("failed to build wasmtime engine: {}", e))
        })?;

        let ticker_stop = Arc::new(AtomicBool::new(false));
        let ticker_engine = engine.clone();
        let ticker_flag = Arc::clone(&ticker_stop);
        thread::spawn(move || {
            while !ticker_flag.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(100));
                ticker_engine.increment_epoch();
            }
        });

        Ok(WasmRuntime {
            engine,
            ticker_stop,
        })
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
            output_truncated: false,
            vault_path: vault_path.to_path_buf(),
            config_json: config_json.to_string(),
            limits: PluginLimits,
        };

        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
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
        if let Ok(init_fn) =
            instance.get_typed_func::<(i32, i32), i32>(&mut store, "parc_plugin_init")
        {
            let config_bytes = config_json.as_bytes();
            let (ptr, len) = write_to_guest(&mut store, &instance, config_bytes)?;
            store.set_epoch_deadline(PLUGIN_CALL_EPOCH_DEADLINE);
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

        self.store.data_mut().clear_output();

        let (event_ptr, event_len) =
            write_to_guest(&mut self.store, &self.instance, event.as_bytes())?;
        let (frag_ptr, frag_len) =
            write_to_guest(&mut self.store, &self.instance, fragment_json.as_bytes())?;

        self.store.set_epoch_deadline(PLUGIN_CALL_EPOCH_DEADLINE);
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
            let output = self.store.data().output_string()?;
            if output.is_empty() {
                Ok(None)
            } else {
                Ok(Some(output))
            }
        }
    }

    /// Call a validation handler: `parc_validate(fragment_ptr, fragment_len) -> i32`
    pub fn call_validate(&mut self, fragment_json: &str) -> Result<ValidationResult, ParcError> {
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

        self.store.data_mut().clear_output();

        let (ptr, len) = write_to_guest(&mut self.store, &self.instance, fragment_json.as_bytes())?;

        self.store.set_epoch_deadline(PLUGIN_CALL_EPOCH_DEADLINE);
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
            let output = self.store.data().output_string()?;
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
    pub fn call_render(&mut self, fragment_json: &str) -> Result<Option<String>, ParcError> {
        let func = match self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut self.store, "parc_render")
        {
            Ok(f) => f,
            Err(_) => return Ok(None),
        };

        self.store.data_mut().clear_output();

        let (ptr, len) = write_to_guest(&mut self.store, &self.instance, fragment_json.as_bytes())?;

        self.store.set_epoch_deadline(PLUGIN_CALL_EPOCH_DEADLINE);
        let _result = func.call(&mut self.store, (ptr, len)).map_err(|e| {
            ParcError::PluginError(format!(
                "plugin '{}' render failed: {}",
                self.manifest.plugin.name, e
            ))
        })?;

        let _ = free_from_guest(&mut self.store, &self.instance, ptr, len);

        let output = self.store.data().output_string()?;
        if output.is_empty() {
            Ok(None)
        } else {
            Ok(Some(output))
        }
    }

    /// Call a command handler: `parc_command(cmd_ptr, cmd_len, args_ptr, args_len) -> i32`
    pub fn call_command(&mut self, cmd: &str, args_json: &str) -> Result<String, ParcError> {
        let func = self
            .instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut self.store, "parc_command")
            .map_err(|_| {
                ParcError::PluginError(format!(
                    "plugin '{}' does not export parc_command",
                    self.manifest.plugin.name
                ))
            })?;

        self.store.data_mut().clear_output();

        let (cmd_ptr, cmd_len) = write_to_guest(&mut self.store, &self.instance, cmd.as_bytes())?;
        let (args_ptr, args_len) =
            write_to_guest(&mut self.store, &self.instance, args_json.as_bytes())?;

        self.store.set_epoch_deadline(PLUGIN_CALL_EPOCH_DEADLINE);
        func.call(&mut self.store, (cmd_ptr, cmd_len, args_ptr, args_len))
            .map_err(|e| {
                ParcError::PluginError(format!(
                    "plugin '{}' command '{}' failed: {}",
                    self.manifest.plugin.name, cmd, e
                ))
            })?;

        let _ = free_from_guest(&mut self.store, &self.instance, cmd_ptr, cmd_len);
        let _ = free_from_guest(&mut self.store, &self.instance, args_ptr, args_len);

        let output = self.store.data().output_string()?;
        Ok(output)
    }
}

/// Allocate guest memory and write bytes into it.
fn write_to_guest(
    store: &mut Store<PluginState>,
    instance: &Instance,
    data: &[u8],
) -> Result<(i32, i32), ParcError> {
    if data.len() > i32::MAX as usize {
        return Err(ParcError::PluginError(
            "payload too large for wasm32 plugin".into(),
        ));
    }

    let alloc_fn = instance
        .get_typed_func::<i32, i32>(&mut *store, "parc_alloc")
        .map_err(|_| ParcError::PluginError("plugin does not export parc_alloc".into()))?;

    let len = data.len() as i32;
    store.set_epoch_deadline(PLUGIN_CALL_EPOCH_DEADLINE);
    let ptr = alloc_fn
        .call(&mut *store, len)
        .map_err(|e| ParcError::PluginError(format!("parc_alloc failed: {}", e)))?;

    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or_else(|| ParcError::PluginError("plugin has no 'memory' export".into()))?;

    // The plugin returned `ptr` itself, so we cannot trust it. Bounds-check
    // (ptr, ptr+len) against the actual memory size before slicing — otherwise
    // a buggy or hostile plugin can panic the host.
    let mem_size = memory.data_size(&mut *store);
    let start = ptr as usize;
    let end = start
        .checked_add(data.len())
        .ok_or_else(|| ParcError::PluginError("parc_alloc returned ptr+len overflow".into()))?;
    if ptr < 0 || end > mem_size {
        return Err(ParcError::PluginError(format!(
            "parc_alloc returned out-of-bounds region: ptr={} len={} mem_size={}",
            ptr,
            data.len(),
            mem_size
        )));
    }

    memory.data_mut(&mut *store)[start..end].copy_from_slice(data);

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
        free_fn
            .call(&mut *store, (ptr, len))
            .map_err(|e| ParcError::PluginError(format!("parc_free failed: {}", e)))?;
    }
    Ok(())
}
