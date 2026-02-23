use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "parc-server", about = "parc JSON-RPC server")]
struct Args {
    /// Path to vault directory
    #[arg(long)]
    vault: Option<PathBuf>,

    /// Use Unix domain socket instead of stdio
    #[arg(long)]
    socket: bool,

    /// Custom socket path (implies --socket)
    #[arg(long)]
    socket_path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let vault_path = parc_core::vault::resolve_vault(args.vault.as_deref())?;

    let use_socket = args.socket || args.socket_path.is_some();
    let transport = if use_socket {
        let path = args
            .socket_path
            .unwrap_or_else(|| vault_path.join("server.sock"));
        eprintln!(
            "parc-server: vault={} transport=socket path={}",
            vault_path.display(),
            path.display()
        );
        parc_server::TransportMode::Socket { path }
    } else {
        eprintln!(
            "parc-server: vault={} transport=stdio",
            vault_path.display()
        );
        parc_server::TransportMode::Stdio
    };

    parc_server::run(vault_path, transport).await
}
