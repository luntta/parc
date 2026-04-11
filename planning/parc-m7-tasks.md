# M7 — JSON-RPC Server

## Features

1. **`parc-server` crate scaffold** — New workspace member with clap, tokio, serde_json
2. **JSON-RPC 2.0 protocol layer** — Hand-rolled request/response/error types per spec
3. **stdio transport** — Newline-delimited JSON over stdin/stdout (LSP-style)
4. **Unix socket transport** — Persistent server on `<vault>/server.sock`
5. **Method router** — Dispatch `method` string to core API calls
6. **Fragment methods** — `fragment.create`, `.get`, `.update`, `.delete`, `.list`, `.search`
7. **Link methods** — `fragment.link`, `.unlink`, `.backlinks`
8. **Attachment methods** — `fragment.attach`, `.detach`, `.attachments`
9. **Vault & schema methods** — `vault.info`, `.reindex`, `.doctor`, `schema.list`, `.get`, `tags.list`
10. **History methods** — `history.list`, `.get`, `.restore`
11. **Server config integration** — Read `server` section from `config.yml`
12. **Graceful shutdown** — Signal handling, socket cleanup, panic recovery
13. **`parc server` CLI subcommand** — Run server from `parc-cli` without separate binary
14. **Integration tests** — Full lifecycle tests over stdio transport
15. **Documentation** — Method reference with examples

**PRD refs:** §3.3, §8.11, §9 (server config), M7 milestone definition.

---

## Feature 1: `parc-server` Crate Scaffold

### Files
- `Cargo.toml` (workspace root) — Add `parc-server` to `members`
- `parc-server/Cargo.toml` — New crate manifest
- `parc-server/src/main.rs` — Entry point with clap args

### Design

Thin binary crate depending on `parc-core`. Uses `tokio` for async I/O on both transports. Clap for argument parsing with the same `--vault` semantics as `parc-cli`.

### Tasks
- [ ] Add `"parc-server"` to workspace members in root `Cargo.toml`
- [ ] Create `parc-server/Cargo.toml`:
  - `parc-core = { path = "../parc-core" }`
  - `tokio = { version = "1", features = ["rt", "io-util", "io-std", "net", "signal", "macros"] }`
  - `serde`, `serde_json`, `clap` (derive), `anyhow`, `thiserror`
- [ ] Create `parc-server/src/main.rs` with clap args:
  - `--vault <path>` — optional, same resolution as CLI
  - `--socket` — flag to use Unix socket instead of stdio
  - `--socket-path <path>` — override default socket location
- [ ] Verify `cargo build -p parc-server` succeeds
- [ ] Verify `cargo test --workspace` still passes

---

## Feature 2: JSON-RPC 2.0 Protocol Layer

### Files
- `parc-server/src/jsonrpc.rs` — Request, Response, Error types and parsing

### Design

Hand-rolled implementation (no `jsonrpc-core` crate needed). The spec is small: parse request objects, produce response objects, handle standard error codes. Support batch requests (JSON arrays) per spec.

### Tasks
- [ ] Create `jsonrpc.rs` with types:
  - `Request { jsonrpc: String, id: Option<Value>, method: String, params: Option<Value> }`
  - `Response { jsonrpc: String, id: Value, result: Option<Value>, error: Option<RpcError> }`
  - `RpcError { code: i64, message: String, data: Option<Value> }`
- [ ] Standard error constructors:
  - `parse_error()` — code `-32700`
  - `invalid_request()` — code `-32600`
  - `method_not_found(method)` — code `-32601`
  - `invalid_params(msg)` — code `-32602`
  - `internal_error(msg)` — code `-32603`
- [ ] `parse_request(line: &str) -> Result<Vec<Request>, Response>`:
  - Parse JSON; on failure return parse_error response
  - If array, return batch; if object, return single-element vec
  - Validate `jsonrpc: "2.0"` field
- [ ] `Response::success(id, result)` and `Response::error(id, rpc_error)` constructors
- [ ] `Response::serialize(&self) -> String` — compact JSON, no trailing newline
- [ ] Unit tests: parse valid request, parse batch, parse malformed JSON, parse missing fields

---

## Feature 3: stdio Transport

### Files
- `parc-server/src/transport.rs` — `Transport` trait and `StdioTransport`

### Design

Newline-delimited JSON: one JSON-RPC message per line. Read from stdin line-by-line, write response + `\n` to stdout, flush after each. Exit cleanly on EOF or broken pipe.

### Tasks
- [ ] Define `Transport` trait:
  ```rust
  #[async_trait]
  trait Transport {
      async fn run(&self, handler: Arc<Router>) -> Result<()>;
  }
  ```
- [ ] Implement `StdioTransport`:
  - `tokio::io::stdin()` with `BufReader` for line reading
  - For each line: parse request(s), dispatch via handler, write response(s), flush
  - Handle batch: collect all responses, write as JSON array on one line
  - On EOF: exit cleanly
  - On broken pipe (stdout write error): exit cleanly
- [ ] Wire into `main.rs`: resolve vault, create `Router`, run `StdioTransport`
- [ ] Manual test: pipe JSON into `parc-server` and verify response

---

## Feature 4: Unix Socket Transport

### Files
- `parc-server/src/transport.rs` — `UnixSocketTransport` added to existing file

### Design

Bind to `<vault>/server.sock` (or `--socket-path`). Remove stale socket file on startup. Each connection gets its own read/write loop (same newline-delimited protocol). Clean up socket file on shutdown.

### Tasks
- [ ] Implement `UnixSocketTransport`:
  - Default path: `<vault>/server.sock`
  - On startup: remove existing socket file if present
  - `tokio::net::UnixListener::bind(path)`
  - For each accepted connection: spawn task with `BufReader`/`BufWriter`, same line protocol as stdio
  - Multiple concurrent connections supported
- [ ] Signal handling: on SIGINT/SIGTERM, remove socket file and exit
- [ ] Wire `--socket` / `--socket-path` flags in `main.rs` to select `UnixSocketTransport`
- [ ] Log to stderr on startup: `"parc-server listening on <path>"`

---

## Feature 5: Method Router

### Files
- `parc-server/src/router.rs` — `Router` struct with `dispatch()` method

### Design

`Router` holds the vault path. On each request it resolves the vault, opens the index, and dispatches by method name. Method handlers are plain functions that take `(vault, params) -> Result<Value, RpcError>`. ParcError maps to internal_error with the error message in `data`.

### Tasks
- [ ] Create `Router` struct with `vault_path: PathBuf`
- [ ] `dispatch(&self, method: &str, params: Value) -> Result<Value, RpcError>`:
  - Match on method string prefix (`fragment.`, `vault.`, `schema.`, `tags.`, `history.`)
  - Unknown method → `method_not_found(method)`
  - Deserialize params into method-specific struct; on failure → `invalid_params(msg)`
  - Call core API; on `ParcError` → `internal_error` with error in `data`
  - Return serialized result as `Value`
- [ ] Helper: `fn extract_params<T: DeserializeOwned>(params: Value) -> Result<T, RpcError>`
- [ ] Helper: `fn map_parc_error(e: ParcError) -> RpcError`
- [ ] Unit test: dispatch unknown method returns -32601

---

## Feature 6: Fragment Methods

### Files
- `parc-server/src/methods/fragment.rs` — Fragment CRUD handlers
- `parc-server/src/methods/mod.rs` — Method module registry

### Design

Each method is a function `fn(vault, params) -> Result<Value, RpcError>`. Params are deserialized into typed structs. Fragment JSON includes all frontmatter fields plus `body` with Markdown content. ID params support prefix matching (same as CLI).

### Tasks
- [ ] Create `methods/` module directory with `mod.rs` and `fragment.rs`
- [ ] `fragment.create` — params: `{type, title, tags?, body?, links?, due?, priority?, status?, assignee?}`
  - Build Fragment, write to vault, index, return created fragment JSON
- [ ] `fragment.get` — params: `{id}`
  - Resolve prefix, load fragment, return full JSON (frontmatter + body)
- [ ] `fragment.update` — params: `{id, title?, tags?, body?, links?, due?, priority?, status?, assignee?}`
  - Load existing, merge fields, write, reindex, return updated JSON
- [ ] `fragment.delete` — params: `{id}`
  - Move to trash, remove from index, return `{id, deleted: true}`
- [ ] `fragment.list` — params: `{type?, status?, tag?, limit?, sort?}`
  - Query index, return array of fragment summaries
- [ ] `fragment.search` — params: `{query, limit?, sort?}`
  - Parse DSL query, execute search, return array of summaries
- [ ] Define `FragmentJson` serialization struct that includes body
- [ ] Unit tests: create and get roundtrip, update fields, delete

---

## Feature 7: Link Methods

### Files
- `parc-server/src/methods/link.rs` — Link/unlink/backlinks handlers

### Tasks
- [ ] `fragment.link` — params: `{id_a, id_b}` — return `{linked: [id_a, id_b]}`
  - Resolve prefixes, add bidirectional link, reindex both
- [ ] `fragment.unlink` — params: `{id_a, id_b}` — return `{unlinked: [id_a, id_b]}`
  - Resolve prefixes, remove bidirectional link, reindex both
- [ ] `fragment.backlinks` — params: `{id}` — return array of linking fragment summaries
- [ ] Register in `methods/mod.rs`

---

## Feature 8: Attachment Methods

### Files
- `parc-server/src/methods/attachment.rs` — Attach/detach/list handlers

### Tasks
- [ ] `fragment.attach` — params: `{id, path}` — copy file, return `{id, filename, size}`
- [ ] `fragment.detach` — params: `{id, filename}` — remove file, return `{id, filename, detached: true}`
- [ ] `fragment.attachments` — params: `{id}` — return array of `{filename, size}`
- [ ] Register in `methods/mod.rs`

---

## Feature 9: Vault & Schema Methods

### Files
- `parc-server/src/methods/vault.rs` — Vault info, reindex, doctor handlers
- `parc-server/src/methods/schema.rs` — Schema list/get handlers
- `parc-server/src/methods/tags.rs` — Tags list handler

### Tasks
- [ ] `vault.info` — return `{path, fragment_count, type_counts, ...}` (reuse vault info logic)
- [ ] `vault.reindex` — reindex all fragments, return `{indexed: N}`
- [ ] `vault.doctor` — run diagnostics, return array of issues
- [ ] `schema.list` — return array of schema summaries (name, alias, fields)
- [ ] `schema.get` — params: `{type}` — return full schema definition
- [ ] `tags.list` — return array of `{tag, count}`
- [ ] Register all in `methods/mod.rs`

---

## Feature 10: History Methods

### Files
- `parc-server/src/methods/history.rs` — History list/get/restore handlers

### Tasks
- [ ] `history.list` — params: `{id}` — return array of `{timestamp, size}`
- [ ] `history.get` — params: `{id, timestamp}` — return full fragment content at that version
- [ ] `history.restore` — params: `{id, timestamp}` — restore version, return restored fragment JSON
- [ ] Register in `methods/mod.rs`

---

## Feature 11: Server Config Integration

### Files
- `parc-core/src/config.rs` — Ensure `server` section is parsed
- `parc-server/src/main.rs` — Read config for defaults

### Design

The vault `config.yml` already defines a `server` section per the PRD:
```yaml
server:
  transport: stdio    # "stdio" | "socket"
  socket_path: null   # defaults to <vault>/server.sock
```

CLI flags (`--socket`, `--socket-path`) override config values.

### Tasks
- [ ] Ensure `ServerConfig { transport: String, socket_path: Option<String> }` is parsed in `config.rs`
- [ ] In `main.rs`, load config and merge with CLI flags (CLI wins)
- [ ] Log resolved config to stderr on startup: transport type, socket path (if socket), vault path

---

## Feature 12: Graceful Shutdown

### Files
- `parc-server/src/main.rs` — Signal handling
- `parc-server/src/transport.rs` — Shutdown coordination

### Tasks
- [ ] Use `tokio::signal` to catch SIGINT and SIGTERM
- [ ] On signal: cancel transport accept loop, flush pending responses
- [ ] Socket transport: remove socket file on shutdown
- [ ] Wrap method dispatch in `catch_unwind` or equivalent — on panic, return `-32603` instead of crashing
- [ ] Connection errors on socket transport: log to stderr, continue accepting new connections
- [ ] Invalid JSON on a connection: return parse_error response, keep connection open

---

## Feature 13: `parc server` CLI Subcommand

### Files
- `parc-cli/Cargo.toml` — Add `tokio` dependency (or `parc-server` as lib)
- `parc-cli/src/main.rs` — Add `Server` command variant
- `parc-cli/src/commands/server.rs` — **New.** Delegates to server startup logic

### Design

Extract core server logic (router, transport, protocol) into a library portion of `parc-server` (lib.rs + bin/main.rs) or a shared module so `parc-cli` can reuse it. This lets users run `parc server` without installing a separate binary.

### Tasks
- [ ] Restructure `parc-server` to expose lib: `parc-server/src/lib.rs` re-exports router + transports
- [ ] Add `Server` variant to `Commands` in `parc-cli/src/main.rs`:
  ```
  Server {
      #[arg(long)] socket: bool,
      #[arg(long)] socket_path: Option<String>,
  }
  ```
- [ ] Create `commands/server.rs` that calls `parc_server::run(vault, transport_config)`
- [ ] Register in `mod.rs` and `main.rs`
- [ ] Test: `parc server` starts and accepts a request on stdio

---

## Feature 14: Integration Tests

### Files
- `parc-server/tests/integration.rs` — **New.** Full test suite

### Design

Spawn `parc-server` as a child process with stdio transport. Helper function sends a JSON-RPC request line and reads the response line. Each test inits a fresh temp vault.

### Tasks
- [ ] Test harness:
  - `setup()` — create temp dir, init vault, spawn `parc-server --vault <path>`
  - `send_rpc(child, request) -> Response` — write JSON line to stdin, read JSON line from stdout
  - `teardown()` — kill child, clean up temp dir
- [ ] Test: fragment lifecycle — create → get → update → list → search → delete
- [ ] Test: link lifecycle — create two fragments, link, verify backlinks, unlink
- [ ] Test: attachment lifecycle — create fragment, attach file, list attachments, detach
- [ ] Test: history — create, update (triggers snapshot), list history, get version, restore
- [ ] Test: vault operations — vault.info, vault.reindex, vault.doctor
- [ ] Test: schema operations — schema.list, schema.get
- [ ] Test: tags.list — create tagged fragments, verify tag counts
- [ ] Test: error cases — unknown method (→ -32601), invalid params (→ -32602), nonexistent ID (→ -32603)
- [ ] Test: batch request — send JSON array of 3 requests, verify array of 3 responses
- [ ] Test: malformed JSON — verify parse_error response, connection stays open for next request

---

## Feature 15: Documentation

### Files
- `docs/json-rpc.md` — **New.** Full method reference

### Tasks
- [ ] Write method reference with request/response examples for every method
- [ ] Transport setup instructions:
  - stdio: spawning as child process, line protocol
  - socket: connecting to Unix domain socket
- [ ] Error code reference table (-32700 through -32603 + application errors)
- [ ] Example integration snippets: Node.js/TypeScript, Python
- [ ] Link from main README

---

## Implementation Order

1. **Feature 1** (scaffold) — get the crate building
2. **Feature 2** (protocol) — JSON-RPC types, independent of transport
3. **Feature 5** (router) — dispatch skeleton returning method_not_found for everything
4. **Feature 3** (stdio) — wire it all together, test manually
5. **Feature 6** (fragment methods) — largest surface area, validates the full pipeline
6. **Feature 7, 8, 9, 10** (remaining methods) — fill in the rest, parallelizable
7. **Feature 11** (config) — config-driven defaults
8. **Feature 4** (socket transport) — second transport
9. **Feature 12** (shutdown) — hardening
10. **Feature 13** (CLI subcommand) — convenience integration
11. **Feature 14** (tests) — full integration suite
12. **Feature 15** (docs) — method reference

---

## Verification

```bash
cargo build -p parc-server
cargo test --workspace

# stdio smoke test
echo '{"jsonrpc":"2.0","id":1,"method":"vault.info","params":{}}' | parc-server --vault /tmp/test-vault

# Fragment lifecycle over stdio
echo '{"jsonrpc":"2.0","id":1,"method":"fragment.create","params":{"type":"note","title":"Hello"}}' | parc-server --vault /tmp/test-vault
echo '{"jsonrpc":"2.0","id":2,"method":"fragment.list","params":{}}' | parc-server --vault /tmp/test-vault
echo '{"jsonrpc":"2.0","id":3,"method":"fragment.search","params":{"query":"type:note"}}' | parc-server --vault /tmp/test-vault

# Socket transport
parc-server --vault /tmp/test-vault --socket &
echo '{"jsonrpc":"2.0","id":1,"method":"tags.list","params":{}}' | socat - UNIX-CONNECT:/tmp/test-vault/server.sock
kill %1

# CLI subcommand
echo '{"jsonrpc":"2.0","id":1,"method":"schema.list","params":{}}' | parc server --vault /tmp/test-vault

# Error handling
echo 'not json' | parc-server --vault /tmp/test-vault           # parse_error
echo '{"jsonrpc":"2.0","id":1,"method":"nope","params":{}}' | parc-server --vault /tmp/test-vault  # method_not_found

# Batch request
echo '[{"jsonrpc":"2.0","id":1,"method":"vault.info","params":{}},{"jsonrpc":"2.0","id":2,"method":"tags.list","params":{}}]' | parc-server --vault /tmp/test-vault
```
