use std::path::Path;

use anyhow::Result;
use parc_core::config::load_config;

pub fn run(vault: &Path, socket: bool, socket_path: Option<String>) -> Result<()> {
    let config = load_config(vault)?;

    let use_socket = socket
        || socket_path.is_some()
        || config.server.transport == "socket";

    let transport = if use_socket {
        let path = socket_path
            .map(std::path::PathBuf::from)
            .or_else(|| config.server.socket_path.map(std::path::PathBuf::from))
            .unwrap_or_else(|| vault.join("server.sock"));
        parc_server::TransportMode::Socket { path }
    } else {
        parc_server::TransportMode::Stdio
    };

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(parc_server::run(vault.to_path_buf(), transport))
}
