pub mod jsonrpc;
pub mod methods;
pub mod router;
pub mod transport;

use std::path::PathBuf;
use std::sync::Arc;

use router::Router;

#[derive(Debug, Clone)]
pub enum TransportMode {
    Stdio,
    Socket { path: PathBuf },
}

/// Run the parc JSON-RPC server with the given vault and transport.
pub async fn run(vault_path: PathBuf, transport: TransportMode) -> anyhow::Result<()> {
    let router = Arc::new(Router::new(vault_path));

    match transport {
        TransportMode::Stdio => transport::run_stdio(router).await,
        TransportMode::Socket { path } => transport::run_socket(router, path).await,
    }
}
