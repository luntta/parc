use std::path::Path;

use parc_core::hook::{HookStderr, ScriptRunner};
use parc_core::ops::Writer;

use crate::render::sanitize_terminal_text;

/// A fragment writer for one-shot CLI commands: hook scripts write straight
/// to the terminal, and WASM plugins are loaded when the feature is enabled.
pub fn cli_writer(vault: &Path) -> Writer {
    writer(vault, HookStderr::Inherit)
}

/// Build a writer with the given hook stderr handling (the TUI captures it).
pub fn writer(vault: &Path, stderr: HookStderr) -> Writer {
    let writer = Writer::new(vault, ScriptRunner::new(stderr));

    #[cfg(feature = "wasm-plugins")]
    let writer = {
        use parc_core::plugin::manager::PluginManager;
        let plugins = parc_core::config::load_config(vault)
            .and_then(|config| PluginManager::load_all(vault, &config))
            .or_else(|e| {
                eprintln!("Warning: failed to load plugins: {}", e);
                PluginManager::empty()
            });
        match plugins {
            Ok(plugins) => writer.with_plugins(plugins),
            Err(_) => writer,
        }
    };

    writer
}

/// Print any warnings the writer collected (e.g. failed post-hooks).
pub fn report_warnings(writer: &mut Writer) {
    for warning in writer.take_warnings() {
        eprintln!("warning: {}", sanitize_terminal_text(&warning));
    }
}
