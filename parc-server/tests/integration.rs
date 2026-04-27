use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde_json::Value;

/// Test harness: init vault, spawn parc-server, provide send_rpc helper.
struct ServerHarness {
    _tmp: tempfile::TempDir,
    vault_path: PathBuf,
    child: Child,
    stdin: std::process::ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
}

impl ServerHarness {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("create temp dir");
        let vault_path = tmp.path().join(".parc");

        // Init vault using parc-core directly
        parc_core::vault::init_vault(&vault_path).expect("init vault");

        let bin = env!("CARGO_BIN_EXE_parc-server");
        let mut child = Command::new(bin)
            .arg("--vault")
            .arg(&vault_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn parc-server");

        let stdin = child.stdin.take().expect("open stdin");
        let stdout = child.stdout.take().expect("open stdout");
        let reader = BufReader::new(stdout);

        ServerHarness {
            _tmp: tmp,
            vault_path,
            child,
            stdin,
            reader,
        }
    }

    fn send_rpc(&mut self, request: Value) -> Value {
        let line = serde_json::to_string(&request).expect("serialize request");
        writeln!(self.stdin, "{}", line).expect("write to stdin");
        self.stdin.flush().expect("flush stdin");

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .expect("read from stdout");
        serde_json::from_str(&response_line).expect("parse response")
    }

    /// Send raw string (for malformed JSON tests)
    fn send_raw(&mut self, raw: &str) -> Value {
        writeln!(self.stdin, "{}", raw).expect("write to stdin");
        self.stdin.flush().expect("flush stdin");

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .expect("read from stdout");
        serde_json::from_str(&response_line).expect("parse response")
    }

    /// Send raw and get raw response (for batch)
    fn send_raw_get_raw(&mut self, raw: &str) -> String {
        writeln!(self.stdin, "{}", raw).expect("write to stdin");
        self.stdin.flush().expect("flush stdin");

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .expect("read from stdout");
        response_line
    }
}

impl Drop for ServerHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn rpc(id: u64, method: &str, params: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

fn result_of(resp: &Value) -> &Value {
    resp.get("result").expect("response should have result")
}

fn error_of(resp: &Value) -> &Value {
    resp.get("error").expect("response should have error")
}

// ── Fragment lifecycle ──────────────────────────────────────────────

#[test]
fn test_fragment_lifecycle() {
    let mut h = ServerHarness::new();

    // Create
    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({
            "type": "note",
            "title": "Test Note",
            "tags": ["test", "rpc"],
            "body": "Hello from integration test."
        }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["type"], "note");
    assert_eq!(result["title"], "Test Note");
    let id = result["id"].as_str().unwrap().to_string();

    // Get
    let resp = h.send_rpc(rpc(
        2,
        "fragment.get",
        serde_json::json!({ "id": &id[..8] }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["title"], "Test Note");
    assert!(result["body"]
        .as_str()
        .unwrap()
        .contains("Hello from integration test."));

    // Update
    let resp = h.send_rpc(rpc(
        3,
        "fragment.update",
        serde_json::json!({
            "id": &id,
            "title": "Updated Note",
            "body": "Updated body."
        }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["title"], "Updated Note");

    // List
    let resp = h.send_rpc(rpc(4, "fragment.list", serde_json::json!({})));
    let results = result_of(&resp).as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "Updated Note");

    // Search
    let resp = h.send_rpc(rpc(
        5,
        "fragment.search",
        serde_json::json!({ "query": "type:note" }),
    ));
    let results = result_of(&resp).as_array().unwrap();
    assert_eq!(results.len(), 1);

    // Delete
    let resp = h.send_rpc(rpc(6, "fragment.delete", serde_json::json!({ "id": &id })));
    let result = result_of(&resp);
    assert!(result["deleted"].as_bool().unwrap());

    // Verify deleted (list should be empty)
    let resp = h.send_rpc(rpc(7, "fragment.list", serde_json::json!({})));
    let results = result_of(&resp).as_array().unwrap();
    assert_eq!(results.len(), 0);
}

// ── Link lifecycle ──────────────────────────────────────────────────

#[test]
fn test_link_lifecycle() {
    let mut h = ServerHarness::new();

    // Create two fragments
    let resp1 = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "A" }),
    ));
    let id_a = result_of(&resp1)["id"].as_str().unwrap().to_string();

    let resp2 = h.send_rpc(rpc(
        2,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "B" }),
    ));
    let id_b = result_of(&resp2)["id"].as_str().unwrap().to_string();

    // Link
    let resp = h.send_rpc(rpc(
        3,
        "fragment.link",
        serde_json::json!({ "id_a": &id_a, "id_b": &id_b }),
    ));
    let result = result_of(&resp);
    let linked = result["linked"].as_array().unwrap();
    assert_eq!(linked.len(), 2);

    // Verify backlinks
    let resp = h.send_rpc(rpc(
        4,
        "fragment.backlinks",
        serde_json::json!({ "id": &id_b }),
    ));
    let backlinks = result_of(&resp).as_array().unwrap();
    assert_eq!(backlinks.len(), 1);
    assert_eq!(backlinks[0]["id"], id_a);

    // Unlink
    let resp = h.send_rpc(rpc(
        5,
        "fragment.unlink",
        serde_json::json!({ "id_a": &id_a, "id_b": &id_b }),
    ));
    let result = result_of(&resp);
    assert!(result["unlinked"].as_array().unwrap().len() == 2);

    // Verify no more backlinks
    let resp = h.send_rpc(rpc(
        6,
        "fragment.backlinks",
        serde_json::json!({ "id": &id_b }),
    ));
    let backlinks = result_of(&resp).as_array().unwrap();
    assert_eq!(backlinks.len(), 0);
}

// ── Attachment lifecycle ────────────────────────────────────────────

#[test]
fn test_attachment_lifecycle() {
    let mut h = ServerHarness::new();

    // Create fragment
    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "With Attachment" }),
    ));
    let id = result_of(&resp)["id"].as_str().unwrap().to_string();

    // Create a temp file to attach
    let tmp_file = h.vault_path.parent().unwrap().join("test.txt");
    std::fs::write(&tmp_file, "attachment content").unwrap();

    // Attach
    let resp = h.send_rpc(rpc(
        2,
        "fragment.attach",
        serde_json::json!({
            "id": &id,
            "path": tmp_file.to_string_lossy(),
        }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["filename"], "test.txt");

    // List attachments
    let resp = h.send_rpc(rpc(
        3,
        "fragment.attachments",
        serde_json::json!({ "id": &id }),
    ));
    let attachments = result_of(&resp).as_array().unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0]["filename"], "test.txt");

    // Detach
    let resp = h.send_rpc(rpc(
        4,
        "fragment.detach",
        serde_json::json!({ "id": &id, "filename": "test.txt" }),
    ));
    let result = result_of(&resp);
    assert!(result["detached"].as_bool().unwrap());

    // Verify empty
    let resp = h.send_rpc(rpc(
        5,
        "fragment.attachments",
        serde_json::json!({ "id": &id }),
    ));
    let attachments = result_of(&resp).as_array().unwrap();
    assert_eq!(attachments.len(), 0);
}

// ── History lifecycle ───────────────────────────────────────────────

#[test]
fn test_history_lifecycle() {
    let mut h = ServerHarness::new();

    // Create
    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({
            "type": "note",
            "title": "V1",
            "body": "version one"
        }),
    ));
    let id = result_of(&resp)["id"].as_str().unwrap().to_string();

    // Update (creates history snapshot)
    let _resp = h.send_rpc(rpc(
        2,
        "fragment.update",
        serde_json::json!({
            "id": &id,
            "title": "V2",
            "body": "version two"
        }),
    ));

    // List history
    let resp = h.send_rpc(rpc(3, "history.list", serde_json::json!({ "id": &id })));
    let versions = result_of(&resp).as_array().unwrap();
    assert!(!versions.is_empty(), "should have at least one version");
    let ts = versions[0]["timestamp"].as_str().unwrap().to_string();

    // Get version
    let resp = h.send_rpc(rpc(
        4,
        "history.get",
        serde_json::json!({ "id": &id, "timestamp": &ts }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["title"], "V1");

    // Restore
    let resp = h.send_rpc(rpc(
        5,
        "history.restore",
        serde_json::json!({ "id": &id, "timestamp": &ts }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["restored_from"], ts);

    // Verify restored
    let resp = h.send_rpc(rpc(6, "fragment.get", serde_json::json!({ "id": &id })));
    let result = result_of(&resp);
    assert_eq!(result["title"], "V1");
}

// ── Vault operations ────────────────────────────────────────────────

#[test]
fn test_vault_operations() {
    let mut h = ServerHarness::new();

    // vault.info
    let resp = h.send_rpc(rpc(1, "vault.info", serde_json::json!({})));
    let result = result_of(&resp);
    assert_eq!(result["fragment_count"], 0);

    // vault.reindex
    let resp = h.send_rpc(rpc(2, "vault.reindex", serde_json::json!({})));
    let result = result_of(&resp);
    assert!(result["indexed"].as_u64().is_some());

    // vault.doctor
    let resp = h.send_rpc(rpc(3, "vault.doctor", serde_json::json!({})));
    let result = result_of(&resp);
    assert!(result["healthy"].as_bool().unwrap());
}

// ── Schema operations ───────────────────────────────────────────────

#[test]
fn test_schema_operations() {
    let mut h = ServerHarness::new();

    // schema.list
    let resp = h.send_rpc(rpc(1, "schema.list", serde_json::json!({})));
    let schemas = result_of(&resp).as_array().unwrap();
    assert!(schemas.len() >= 5); // 5 built-in types
    let names: Vec<&str> = schemas
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"note"));
    assert!(names.contains(&"todo"));

    // schema.get
    let resp = h.send_rpc(rpc(2, "schema.get", serde_json::json!({ "type": "todo" })));
    let result = result_of(&resp);
    assert_eq!(result["name"], "todo");
    let fields = result["fields"].as_array().unwrap();
    let field_names: Vec<&str> = fields.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(field_names.contains(&"status"));
    assert!(field_names.contains(&"priority"));
}

// ── Tags ────────────────────────────────────────────────────────────

#[test]
fn test_tags_list() {
    let mut h = ServerHarness::new();

    // Create tagged fragments
    h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "A", "tags": ["rust", "backend"] }),
    ));
    h.send_rpc(rpc(
        2,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "B", "tags": ["rust", "frontend"] }),
    ));

    let resp = h.send_rpc(rpc(3, "tags.list", serde_json::json!({})));
    let tags = result_of(&resp).as_array().unwrap();
    assert!(tags.len() >= 2);

    // Find the "rust" tag and verify count
    let rust_tag = tags.iter().find(|t| t["tag"] == "rust");
    assert!(rust_tag.is_some());
    assert_eq!(rust_tag.unwrap()["count"], 2);
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn test_error_unknown_method() {
    let mut h = ServerHarness::new();

    let resp = h.send_rpc(rpc(1, "nonexistent.method", serde_json::json!({})));
    let err = error_of(&resp);
    assert_eq!(err["code"], -32601);
}

#[test]
fn test_error_invalid_params() {
    let mut h = ServerHarness::new();

    let resp = h.send_rpc(rpc(
        1,
        "fragment.get",
        serde_json::json!({ "wrong_field": 123 }),
    ));
    let err = error_of(&resp);
    assert_eq!(err["code"], -32602);
}

#[test]
fn test_error_nonexistent_id() {
    let mut h = ServerHarness::new();

    let resp = h.send_rpc(rpc(
        1,
        "fragment.get",
        serde_json::json!({ "id": "NONEXISTENT" }),
    ));
    let err = error_of(&resp);
    assert_eq!(err["code"], -32603);
}

#[test]
fn test_error_malformed_json() {
    let mut h = ServerHarness::new();

    let resp = h.send_raw("this is not json at all");
    let err = error_of(&resp);
    assert_eq!(err["code"], -32700);

    // Connection should still work after malformed input
    let resp = h.send_rpc(rpc(1, "vault.info", serde_json::json!({})));
    assert!(resp.get("result").is_some());
}

// ── Batch request ───────────────────────────────────────────────────

#[test]
fn test_batch_request() {
    let mut h = ServerHarness::new();

    let batch = r#"[{"jsonrpc":"2.0","id":1,"method":"vault.info","params":{}},{"jsonrpc":"2.0","id":2,"method":"tags.list","params":{}},{"jsonrpc":"2.0","id":3,"method":"schema.list","params":{}}]"#;
    let raw = h.send_raw_get_raw(batch);
    let responses: Vec<Value> = serde_json::from_str(&raw).expect("parse batch response");

    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["id"], 1);
    assert!(responses[0].get("result").is_some());
    assert_eq!(responses[1]["id"], 2);
    assert!(responses[1].get("result").is_some());
    assert_eq!(responses[2]["id"], 3);
    assert!(responses[2].get("result").is_some());
}

// ── Fragment with type-specific fields ──────────────────────────────

#[test]
fn test_todo_with_fields() {
    let mut h = ServerHarness::new();

    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({
            "type": "todo",
            "title": "Test Task",
            "status": "open",
            "priority": "high",
            "due": "2026-12-31",
        }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["type"], "todo");
    assert_eq!(result["status"], "open");
    assert_eq!(result["priority"], "high");
    assert_eq!(result["due"], "2026-12-31");
    let id = result["id"].as_str().unwrap().to_string();

    // Update status
    let resp = h.send_rpc(rpc(
        2,
        "fragment.update",
        serde_json::json!({ "id": &id, "status": "done" }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["status"], "done");

    // Invalid enum values are rejected and must not be persisted
    let resp = h.send_rpc(rpc(
        3,
        "fragment.update",
        serde_json::json!({ "id": &id, "status": "blocked" }),
    ));
    let err = error_of(&resp);
    assert_eq!(err["code"], -32602);

    let resp = h.send_rpc(rpc(4, "fragment.get", serde_json::json!({ "id": &id })));
    let result = result_of(&resp);
    assert_eq!(result["status"], "done");

    // Search by status
    let resp = h.send_rpc(rpc(
        5,
        "fragment.search",
        serde_json::json!({ "query": "type:todo status:done" }),
    ));
    let results = result_of(&resp).as_array().unwrap();
    assert_eq!(results.len(), 1);
}

// ── List with filters ───────────────────────────────────────────────

#[test]
fn test_list_with_filters() {
    let mut h = ServerHarness::new();

    h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Note A", "tags": ["alpha"] }),
    ));
    h.send_rpc(rpc(
        2,
        "fragment.create",
        serde_json::json!({ "type": "todo", "title": "Todo B", "tags": ["beta"] }),
    ));

    // Filter by type
    let resp = h.send_rpc(rpc(
        3,
        "fragment.list",
        serde_json::json!({ "type": "note" }),
    ));
    let results = result_of(&resp).as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["type"], "note");

    // Filter by tag
    let resp = h.send_rpc(rpc(
        4,
        "fragment.list",
        serde_json::json!({ "tag": "beta" }),
    ));
    let results = result_of(&resp).as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "Todo B");
}

// ── Schema.get with unknown type ────────────────────────────────────

#[test]
fn test_schema_get_unknown() {
    let mut h = ServerHarness::new();

    let resp = h.send_rpc(rpc(
        1,
        "schema.get",
        serde_json::json!({ "type": "nonexistent" }),
    ));
    let err = error_of(&resp);
    assert_eq!(err["code"], -32602);
}

// ── Unix socket is bound 0600 ───────────────────────────────────────

#[cfg(unix)]
#[test]
fn test_socket_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new().expect("tmp");
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).expect("init vault");
    let socket = tmp.path().join("server.sock");

    let bin = env!("CARGO_BIN_EXE_parc-server");
    let mut child = Command::new(bin)
        .arg("--vault")
        .arg(&vault)
        .arg("--socket-path")
        .arg(&socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn parc-server");

    // Wait until the socket appears (server bound), with a timeout.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !socket.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(socket.exists(), "socket never appeared");

    let mode = std::fs::metadata(&socket)
        .expect("stat socket")
        .permissions()
        .mode()
        & 0o777;
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(mode, 0o600, "socket mode is {:o}, expected 0600", mode);
}
