use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::signal;

use crate::jsonrpc::{self, Response, RpcError};
use crate::router::Router;

/// Process a single line of JSON-RPC input and return response line(s).
fn handle_line(router: &Router, line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    match jsonrpc::parse_request(line) {
        Ok(requests) => {
            let is_batch = line.trim_start().starts_with('[');
            let mut responses = Vec::new();

            for req in requests {
                let id = req.id.clone().unwrap_or(serde_json::Value::Null);

                if let Err(err) = jsonrpc::validate_request(&req) {
                    responses.push(Response::error(id, err));
                    continue;
                }

                let params = req.params.unwrap_or(serde_json::Value::Null);

                // Catch panics in method dispatch
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    router.dispatch(&req.method, params)
                }));

                let response = match result {
                    Ok(Ok(value)) => Response::success(id, value),
                    Ok(Err(rpc_err)) => Response::error(id, rpc_err),
                    Err(_) => Response::error(id, RpcError::internal_error("method panicked")),
                };

                responses.push(response);
            }

            if responses.is_empty() {
                return None;
            }

            let output = if is_batch {
                serde_json::to_string(&responses).unwrap_or_default()
            } else {
                serde_json::to_string(&responses[0]).unwrap_or_default()
            };

            Some(output)
        }
        Err(err_response) => Some(serde_json::to_string(&err_response).unwrap_or_default()),
    }
}

/// Run the server on stdio (newline-delimited JSON over stdin/stdout).
pub async fn run_stdio(router: Arc<Router>) -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(response) = handle_line(&router, &line) {
            if stdout.write_all(response.as_bytes()).await.is_err() {
                break; // broken pipe
            }
            if stdout.write_all(b"\n").await.is_err() {
                break;
            }
            if stdout.flush().await.is_err() {
                break;
            }
        }
    }

    Ok(())
}

/// Run the server on a Unix domain socket.
pub async fn run_socket(router: Arc<Router>, socket_path: PathBuf) -> anyhow::Result<()> {
    // Remove stale socket file
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }

    let listener = UnixListener::bind(&socket_path)?;
    eprintln!(
        "parc-server listening on {}",
        socket_path.to_string_lossy()
    );

    // Spawn signal handler for cleanup
    let cleanup_path = socket_path.clone();
    tokio::spawn(async move {
        let _ = signal::ctrl_c().await;
        let _ = std::fs::remove_file(&cleanup_path);
        std::process::exit(0);
    });

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let router = Arc::clone(&router);
                tokio::spawn(async move {
                    let (reader, mut writer) = stream.into_split();
                    let reader = BufReader::new(reader);
                    let mut lines = reader.lines();

                    while let Ok(Some(line)) = lines.next_line().await {
                        if let Some(response) = handle_line(&router, &line) {
                            if writer.write_all(response.as_bytes()).await.is_err() {
                                break;
                            }
                            if writer.write_all(b"\n").await.is_err() {
                                break;
                            }
                            if writer.flush().await.is_err() {
                                break;
                            }
                        }
                    }
                });
            }
            Err(e) => {
                eprintln!("parc-server: connection error: {}", e);
            }
        }
    }
}
