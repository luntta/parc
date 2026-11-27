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

    /// Speak the Model Context Protocol on stdio instead of parc's JSON-RPC,
    /// for agent harnesses (Claude Code, Codex, editors)
    #[arg(long, conflicts_with_all = ["socket", "socket_path"])]
    mcp: bool,

    /// With --mcp: offer only the tools that read the vault
    #[arg(long, requires = "mcp")]
    read_only: bool,
}

fn main() -> anyhow::Result<()> {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == parc_core::extract::CHILD_ARG)
    {
        parc_core::extract::child_main();
    }
    let args = Args::parse();

    let vault_path = parc_core::vault::resolve_vault(args.vault.as_deref())?;

    if args.mcp {
        eprintln!(
            "parc-server: vault={} transport=stdio protocol=mcp{}",
            vault_path.display(),
            if args.read_only { " read-only" } else { "" }
        );
        return parc_server::mcp::run_stdio(vault_path, args.read_only);
    }

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

    tokio::runtime::Runtime::new()?.block_on(parc_server::run(vault_path, transport))
}
