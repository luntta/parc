use std::path::Path;

use parc_core::hook::{HookStderr, ScriptRunner};
use parc_core::ops::Writer;
use serde_json::Value;

/// A fragment writer for one request. Hook stderr is captured: stdout is the
/// JSON-RPC channel in stdio mode, and errors should reach the client.
pub fn writer(vault: &Path) -> Writer {
    Writer::new(vault, ScriptRunner::new(HookStderr::Capture))
}

/// Attach any hook warnings to an object result as `"warnings": [...]`.
pub fn with_warnings(mut value: Value, writer: &mut Writer) -> Value {
    let warnings = writer.take_warnings();
    if !warnings.is_empty() {
        if let Value::Object(map) = &mut value {
            map.insert("warnings".to_string(), Value::from(warnings));
        }
    }
    value
}
