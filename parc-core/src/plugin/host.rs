use wasmtime::*;

use crate::error::ParcError;

use super::runtime::PluginState;

/// Bounds-check a (ptr, len) pair from guest WebAssembly. Returns the
/// `start..end` byte range only if both are non-negative and the range fits
/// inside `mem_len`. Centralized so every host import handler has identical
/// guard logic — overflow-safe and signed-pointer-safe.
fn guest_range(ptr: i32, len: i32, mem_len: usize) -> Option<std::ops::Range<usize>> {
    if ptr < 0 || len < 0 {
        return None;
    }
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    if end > mem_len {
        return None;
    }
    Some(start..end)
}

/// Register all host functions under the "parc_host" namespace.
pub fn register_host_functions(linker: &mut Linker<PluginState>) -> Result<(), ParcError> {
    // parc_host_output(ptr: i32, len: i32)
    linker
        .func_wrap(
            "parc_host",
            "parc_host_output",
            |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };
                let data = memory.data(&caller);
                let range = match guest_range(ptr, len, data.len()) {
                    Some(r) => r,
                    None => return,
                };
                let bytes = data[range].to_vec();
                caller.data_mut().output_buffer.extend_from_slice(&bytes);
            },
        )
        .map_err(|e| ParcError::PluginError(format!("failed to link parc_host_output: {}", e)))?;

    // parc_host_log(level: i32, ptr: i32, len: i32)
    linker
        .func_wrap(
            "parc_host",
            "parc_host_log",
            |mut caller: Caller<'_, PluginState>, level: i32, ptr: i32, len: i32| {
                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return,
                };
                let data = memory.data(&caller);
                let range = match guest_range(ptr, len, data.len()) {
                    Some(r) => r,
                    None => return,
                };
                let msg = String::from_utf8_lossy(&data[range]).to_string();
                let plugin_name = caller.data().manifest.plugin.name.clone();
                let level_str = match level {
                    0 => "DEBUG",
                    1 => "INFO",
                    2 => "WARN",
                    3 => "ERROR",
                    _ => "LOG",
                };
                eprintln!("[{}][{}] {}", plugin_name, level_str, msg);
            },
        )
        .map_err(|e| ParcError::PluginError(format!("failed to link parc_host_log: {}", e)))?;

    // parc_host_fragment_get(id_ptr: i32, id_len: i32) -> i32
    linker
        .func_wrap(
            "parc_host",
            "parc_host_fragment_get",
            |mut caller: Caller<'_, PluginState>, id_ptr: i32, id_len: i32| -> i32 {
                if !caller.data().manifest.capabilities.read_fragments {
                    return -1;
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return -1,
                };
                let data = memory.data(&caller);
                let range = match guest_range(id_ptr, id_len, data.len()) {
                    Some(r) => r,
                    None => return -1,
                };
                let id = String::from_utf8_lossy(&data[range]).to_string();
                let vault_path = caller.data().vault_path.clone();

                match crate::fragment::read_fragment(&vault_path, &id) {
                    Ok(frag) => match serde_json::to_string(&frag) {
                        Ok(json) => {
                            let bytes = json.as_bytes();
                            let len = bytes.len() as i32;
                            caller.data_mut().output_buffer.clear();
                            caller.data_mut().output_buffer.extend_from_slice(bytes);
                            len
                        }
                        Err(_) => -1,
                    },
                    Err(_) => -1,
                }
            },
        )
        .map_err(|e| {
            ParcError::PluginError(format!("failed to link parc_host_fragment_get: {}", e))
        })?;

    // parc_host_fragment_search(query_ptr: i32, query_len: i32) -> i32
    linker
        .func_wrap(
            "parc_host",
            "parc_host_fragment_search",
            |mut caller: Caller<'_, PluginState>, query_ptr: i32, query_len: i32| -> i32 {
                if !caller.data().manifest.capabilities.read_fragments {
                    return -1;
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return -1,
                };
                let data = memory.data(&caller);
                let range = match guest_range(query_ptr, query_len, data.len()) {
                    Some(r) => r,
                    None => return -1,
                };
                let query = String::from_utf8_lossy(&data[range]).to_string();
                let vault_path = caller.data().vault_path.clone();

                let parsed = match crate::search::parse_query(&query) {
                    Ok(q) => q,
                    Err(_) => return -1,
                };

                match crate::index::open_index(&vault_path) {
                    Ok(conn) => match crate::search::search(&conn, &parsed) {
                        Ok(results) => match serde_json::to_string(&results) {
                            Ok(json) => {
                                let bytes = json.as_bytes();
                                let len = bytes.len() as i32;
                                caller.data_mut().output_buffer.clear();
                                caller.data_mut().output_buffer.extend_from_slice(bytes);
                                len
                            }
                            Err(_) => -1,
                        },
                        Err(_) => -1,
                    },
                    Err(_) => -1,
                }
            },
        )
        .map_err(|e| {
            ParcError::PluginError(format!("failed to link parc_host_fragment_search: {}", e))
        })?;

    // parc_host_fragment_list(params_ptr: i32, params_len: i32) -> i32
    linker
        .func_wrap(
            "parc_host",
            "parc_host_fragment_list",
            |mut caller: Caller<'_, PluginState>, _params_ptr: i32, _params_len: i32| -> i32 {
                if !caller.data().manifest.capabilities.read_fragments {
                    return -1;
                }

                let vault_path = caller.data().vault_path.clone();
                match crate::fragment::list_fragment_ids(&vault_path) {
                    Ok(ids) => match serde_json::to_string(&ids) {
                        Ok(json) => {
                            let bytes = json.as_bytes();
                            let len = bytes.len() as i32;
                            caller.data_mut().output_buffer.clear();
                            caller.data_mut().output_buffer.extend_from_slice(bytes);
                            len
                        }
                        Err(_) => -1,
                    },
                    Err(_) => -1,
                }
            },
        )
        .map_err(|e| {
            ParcError::PluginError(format!("failed to link parc_host_fragment_list: {}", e))
        })?;

    // parc_host_fragment_create(json_ptr: i32, json_len: i32) -> i32
    linker
        .func_wrap(
            "parc_host",
            "parc_host_fragment_create",
            |mut caller: Caller<'_, PluginState>, json_ptr: i32, json_len: i32| -> i32 {
                if !caller.data().manifest.capabilities.write_fragments {
                    return -1;
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return -1,
                };
                let data = memory.data(&caller);
                let range = match guest_range(json_ptr, json_len, data.len()) {
                    Some(r) => r,
                    None => return -1,
                };
                let json_str = String::from_utf8_lossy(&data[range]).to_string();
                let vault_path = caller.data().vault_path.clone();

                match serde_json::from_str::<crate::fragment::Fragment>(&json_str) {
                    Ok(frag) => match crate::fragment::create_fragment(&vault_path, &frag) {
                        Ok(id) => {
                            let id_bytes = id.as_bytes();
                            let len = id_bytes.len() as i32;
                            caller.data_mut().output_buffer.clear();
                            caller.data_mut().output_buffer.extend_from_slice(id_bytes);
                            len
                        }
                        Err(_) => -1,
                    },
                    Err(_) => -1,
                }
            },
        )
        .map_err(|e| {
            ParcError::PluginError(format!("failed to link parc_host_fragment_create: {}", e))
        })?;

    // parc_host_fragment_update(json_ptr: i32, json_len: i32) -> i32
    linker
        .func_wrap(
            "parc_host",
            "parc_host_fragment_update",
            |mut caller: Caller<'_, PluginState>, json_ptr: i32, json_len: i32| -> i32 {
                if !caller.data().manifest.capabilities.write_fragments {
                    return -1;
                }

                let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                    Some(m) => m,
                    None => return -1,
                };
                let data = memory.data(&caller);
                let range = match guest_range(json_ptr, json_len, data.len()) {
                    Some(r) => r,
                    None => return -1,
                };
                let json_str = String::from_utf8_lossy(&data[range]).to_string();
                let vault_path = caller.data().vault_path.clone();

                match serde_json::from_str::<crate::fragment::Fragment>(&json_str) {
                    Ok(frag) => match crate::fragment::write_fragment(&vault_path, &frag) {
                        Ok(()) => {
                            let id_bytes = frag.id.as_bytes();
                            let len = id_bytes.len() as i32;
                            caller.data_mut().output_buffer.clear();
                            caller.data_mut().output_buffer.extend_from_slice(id_bytes);
                            len
                        }
                        Err(_) => -1,
                    },
                    Err(_) => -1,
                }
            },
        )
        .map_err(|e| {
            ParcError::PluginError(format!("failed to link parc_host_fragment_update: {}", e))
        })?;

    Ok(())
}
