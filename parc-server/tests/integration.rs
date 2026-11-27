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
            .env("HOME", tmp.path())
            // Whether the meaning model is installed here must not change results.
            .env("PARC_MODELS_DIR", "/nonexistent/parc-test-models")
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

    // Search, only the rows asked for by ID
    let resp = h.send_rpc(rpc(
        51,
        "fragment.search",
        serde_json::json!({ "query": "is:all", "ids": [&id] }),
    ));
    assert_eq!(result_of(&resp).as_array().unwrap().len(), 1);
    let resp = h.send_rpc(rpc(
        52,
        "fragment.search",
        serde_json::json!({ "query": "is:all", "ids": ["01JQ7V3XKP5GQZ2N8R6T1WBMVH"] }),
    ));
    assert_eq!(result_of(&resp).as_array().unwrap().len(), 0);

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

    // vault.background: nothing to extract, and no meaning model here (PARC_MODELS_DIR is missing)
    let resp = h.send_rpc(rpc(20, "vault.background", serde_json::json!({})));
    let result = result_of(&resp);
    assert_eq!(result["extract"], 0);
    assert_eq!(result["meaning"], 0);

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
        // Whether the meaning model is installed here must not change results.
        .env("PARC_MODELS_DIR", "/nonexistent/parc-test-models")
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

// ── Writes run vault hooks like the CLI does ─────────────────────────

#[cfg(unix)]
#[test]
fn test_update_runs_hooks_and_reports_warnings() {
    use std::os::unix::fs::PermissionsExt;

    let mut h = ServerHarness::new();
    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Hooked" }),
    ));
    let id = result_of(&resp)["id"].as_str().unwrap().to_string();

    std::fs::write(h.vault_path.join("config.yml"), "hooks:\n  enabled: true\n").unwrap();
    let hooks_dir = h.vault_path.join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let marker = h.vault_path.join("pre-update-fired");
    let pre = hooks_dir.join("pre-update");
    std::fs::write(
        &pre,
        "#!/bin/sh\ntouch \"$(dirname \"$0\")/../pre-update-fired\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&pre, std::fs::Permissions::from_mode(0o700)).unwrap();
    let post = hooks_dir.join("post-update");
    std::fs::write(&post, "#!/bin/sh\necho 'post broke' >&2\nexit 2\n").unwrap();
    std::fs::set_permissions(&post, std::fs::Permissions::from_mode(0o700)).unwrap();
    parc_core::trust::grant_for_home(&h.vault_path, h._tmp.path()).unwrap();

    let resp = h.send_rpc(rpc(
        2,
        "fragment.update",
        serde_json::json!({ "id": &id, "title": "Hooked again" }),
    ));
    let result = result_of(&resp);

    assert_eq!(result["title"], "Hooked again");
    assert!(marker.exists(), "pre-update hook did not run");
    let warnings = result["warnings"].as_array().expect("warnings array");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].as_str().unwrap().contains("post broke"));
}

#[cfg(unix)]
#[test]
fn test_pre_hook_veto_is_invalid_params() {
    use std::os::unix::fs::PermissionsExt;

    let mut h = ServerHarness::new();
    std::fs::write(h.vault_path.join("config.yml"), "hooks:\n  enabled: true\n").unwrap();
    let hooks_dir = h.vault_path.join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();
    let pre = hooks_dir.join("pre-create");
    std::fs::write(&pre, "#!/bin/sh\necho 'no new notes' >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(&pre, std::fs::Permissions::from_mode(0o700)).unwrap();
    parc_core::trust::grant_for_home(&h.vault_path, h._tmp.path()).unwrap();

    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Blocked" }),
    ));
    let err = error_of(&resp);
    assert_eq!(err["code"], -32602);
    assert!(err["message"].as_str().unwrap().contains("no new notes"));
}

// ── Schema fields, stale writes and the vault link list ─────────────

#[test]
fn test_fields_map_sets_and_clears_schema_fields() {
    let mut h = ServerHarness::new();

    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({
            "type": "risk",
            "title": "Disk fills",
            "fields": { "impact": "high", "likelihood": "low" },
        }),
    ));
    let result = result_of(&resp);
    assert_eq!(result["impact"], "high");
    assert_eq!(result["status"], "identified");
    let id = result["id"].as_str().unwrap().to_string();

    // null clears an optional field; others are set alongside.
    let resp = h.send_rpc(rpc(
        2,
        "fragment.update",
        serde_json::json!({ "id": &id, "fields": { "likelihood": null, "status": "mitigating" } }),
    ));
    let result = result_of(&resp);
    assert!(result.get("likelihood").is_none());
    assert_eq!(result["status"], "mitigating");
    assert_eq!(result["impact"], "high");

    // Required fields cannot be cleared, unknown fields are named, and
    // enum values are still checked.
    for (n, fields, needle) in [
        (3, serde_json::json!({ "status": null }), "required"),
        (
            4,
            serde_json::json!({ "due": "today" }),
            "unknown field 'due'",
        ),
        (5, serde_json::json!({ "impact": "huge" }), "invalid value"),
    ] {
        let resp = h.send_rpc(rpc(
            n,
            "fragment.update",
            serde_json::json!({ "id": &id, "fields": fields }),
        ));
        let err = error_of(&resp);
        assert_eq!(err["code"], -32602);
        assert!(
            err["message"].as_str().unwrap().contains(needle),
            "{} not in {}",
            needle,
            err["message"]
        );
    }
}

#[test]
fn test_update_refuses_stale_expected_updated_at() {
    let mut h = ServerHarness::new();

    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Shared", "body": "v1" }),
    ));
    let id = result_of(&resp)["id"].as_str().unwrap().to_string();
    let resp = h.send_rpc(rpc(2, "fragment.get", serde_json::json!({ "id": &id })));
    let read_at = result_of(&resp)["updated_at"].as_str().unwrap().to_string();

    // The copy that was read can be saved...
    let resp = h.send_rpc(rpc(
        3,
        "fragment.update",
        serde_json::json!({ "id": &id, "body": "v2", "expected_updated_at": &read_at }),
    ));
    let saved_at = result_of(&resp)["updated_at"].as_str().unwrap().to_string();

    // ...and saved again from the timestamp the save returned...
    let resp = h.send_rpc(rpc(
        4,
        "fragment.update",
        serde_json::json!({ "id": &id, "body": "v3", "expected_updated_at": &saved_at }),
    ));
    assert_eq!(result_of(&resp)["body"], "v3");

    // ...but a copy older than the file is refused and changes nothing.
    let resp = h.send_rpc(rpc(
        5,
        "fragment.update",
        serde_json::json!({ "id": &id, "body": "stale", "expected_updated_at": "2020-01-01T00:00:00Z" }),
    ));
    let err = error_of(&resp);
    assert_eq!(err["code"], -32010);
    assert!(err["data"]["updated_at"].is_string());
    let resp = h.send_rpc(rpc(6, "fragment.get", serde_json::json!({ "id": &id })));
    assert_eq!(result_of(&resp)["body"].as_str().unwrap().trim(), "v3");
}

#[test]
fn test_schema_list_describes_enum_values_and_toggle() {
    let mut h = ServerHarness::new();
    let resp = h.send_rpc(rpc(1, "schema.list", serde_json::json!({})));
    let schemas = result_of(&resp).as_array().unwrap();
    let todo = schemas.iter().find(|s| s["name"] == "todo").unwrap();
    let status = todo["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "status")
        .unwrap();
    assert_eq!(status["kind"], "enum");
    assert_eq!(status["values"][0], "open");
    assert_eq!(status["toggle"], serde_json::json!(["open", "done"]));
    assert_eq!(
        todo["toggle"],
        serde_json::json!({ "field": "status", "off": "open", "on": "done" })
    );
    let note = schemas.iter().find(|s| s["name"] == "note").unwrap();
    assert!(note["toggle"].is_null());
}

#[test]
fn test_vault_links_lists_resolved_edges() {
    let mut h = ServerHarness::new();
    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Target" }),
    ));
    let target = result_of(&resp)["id"].as_str().unwrap().to_string();
    let resp = h.send_rpc(rpc(
        2,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Source", "body": format!("See [[{}]].", &target[..10]) }),
    ));
    let source = result_of(&resp)["id"].as_str().unwrap().to_string();

    let resp = h.send_rpc(rpc(3, "vault.links", serde_json::json!({})));
    assert_eq!(
        result_of(&resp),
        &serde_json::json!([{ "source": source, "target": target }])
    );
}

#[test]
fn test_revision_catches_a_save_in_the_same_second() {
    let mut h = ServerHarness::new();
    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Fast", "body": "v1" }),
    ));
    let created = result_of(&resp).clone();
    let id = created["id"].as_str().unwrap().to_string();
    let resp = h.send_rpc(rpc(2, "fragment.get", serde_json::json!({ "id": &id })));
    assert_eq!(result_of(&resp)["revision"], created["revision"]);

    // Another client saves within the same second: the timestamp can't tell, the revision can.
    let resp = h.send_rpc(rpc(
        3,
        "fragment.update",
        serde_json::json!({ "id": &id, "body": "theirs" }),
    ));
    let theirs = result_of(&resp).clone();
    assert_ne!(theirs["revision"], created["revision"]);
    let resp = h.send_rpc(rpc(
        4,
        "fragment.update",
        serde_json::json!({ "id": &id, "body": "mine", "expected_revision": created["revision"] }),
    ));
    assert_eq!(error_of(&resp)["code"], -32010);
    let resp = h.send_rpc(rpc(
        5,
        "fragment.update",
        serde_json::json!({ "id": &id, "body": "mine", "expected_revision": theirs["revision"] }),
    ));
    assert_eq!(result_of(&resp)["body"], "mine");
}

#[test]
fn test_archive_and_trash_restore() {
    let mut h = ServerHarness::new();
    let resp = h.send_rpc(rpc(
        1,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Keep me" }),
    ));
    let id = result_of(&resp)["id"].as_str().unwrap().to_string();

    let resp = h.send_rpc(rpc(
        2,
        "fragment.archive",
        serde_json::json!({ "id": &id, "archived": true }),
    ));
    assert_eq!(result_of(&resp)["archived"], true);
    let resp = h.send_rpc(rpc(
        3,
        "fragment.search",
        serde_json::json!({ "query": "is:all" }),
    ));
    assert_eq!(result_of(&resp)[0]["archived"], true);
    let resp = h.send_rpc(rpc(
        4,
        "fragment.search",
        serde_json::json!({ "query": "Keep" }),
    ));
    assert!(result_of(&resp).as_array().unwrap().is_empty());
    let resp = h.send_rpc(rpc(
        5,
        "fragment.archive",
        serde_json::json!({ "id": &id, "archived": false }),
    ));
    assert!(result_of(&resp).get("archived").is_none());

    h.send_rpc(rpc(6, "fragment.delete", serde_json::json!({ "id": &id })));
    let resp = h.send_rpc(rpc(7, "fragment.get", serde_json::json!({ "id": &id })));
    assert!(resp.get("error").is_some());
    let resp = h.send_rpc(rpc(8, "trash.restore", serde_json::json!({ "id": &id })));
    assert_eq!(result_of(&resp)["title"], "Keep me");
    let resp = h.send_rpc(rpc(
        9,
        "fragment.search",
        serde_json::json!({ "query": "Keep" }),
    ));
    assert_eq!(result_of(&resp)[0]["id"], id.as_str());
    let resp = h.send_rpc(rpc(10, "trash.restore", serde_json::json!({ "id": &id })));
    assert!(resp.get("error").is_some());
}

#[test]
fn test_mcp_mode_over_stdio() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault_path = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault_path).unwrap();
    let bin = env!("CARGO_BIN_EXE_parc-server");

    let mut child = Command::new(bin)
        // Whether the meaning model is installed here must not change results.
        .env("PARC_MODELS_DIR", "/nonexistent/parc-test-models")
        .args(["--mcp", "--read-only", "--vault"])
        .arg(&vault_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#
    )
    .unwrap();
    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
    )
    .unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
    drop(stdin);

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let lines: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "the notification gets no reply");
    assert_eq!(lines[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(lines[1]["result"]["tools"].as_array().unwrap().len(), 6);

    // --read-only only makes sense for MCP.
    let status = Command::new(bin)
        // Whether the meaning model is installed here must not change results.
        .env("PARC_MODELS_DIR", "/nonexistent/parc-test-models")
        .args(["--read-only", "--vault"])
        .arg(&vault_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
}

// ── Recall ──────────────────────────────────────────────────────────

#[test]
fn test_recall_context_for_drafts_and_saved_fragments() {
    let mut h = ServerHarness::new();
    let create = |h: &mut ServerHarness, title: &str, body: &str| {
        let resp = h.send_rpc(rpc(
            1,
            "fragment.create",
            serde_json::json!({ "type": "note", "title": title, "body": body }),
        ));
        result_of(&resp)["id"].as_str().unwrap().to_string()
    };
    let ferry = create(
        &mut h,
        "What helped at the ferry desk",
        "The queue was watching.\nGiving each visitor a route card to mark in private worked better.",
    );
    let orientation = create(
        &mut h,
        "Volunteer orientation",
        "Hand out a route card to mark in private, as at the ferry desk.",
    );
    create(&mut h, "Kitchen timer", "Broken again.");

    // A draft that isn't saved: found by its words, quoted verbatim.
    let resp = h.send_rpc(rpc(
        2,
        "recall.context",
        serde_json::json!({ "text": "Nobody asked in front of the room. A private route card?" }),
    ));
    let ctx = result_of(&resp);
    assert_eq!(ctx["seed"]["id"], Value::Null);
    assert_eq!(ctx["truncated"], false);
    let sources = ctx["sources"].as_array().unwrap();
    let ids: Vec<&str> = sources.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&ferry.as_str()) && ids.contains(&orientation.as_str()));
    assert_eq!(ids.len(), 2, "the kitchen timer shares nothing");
    let ferry_source = sources.iter().find(|s| s["id"] == ferry.as_str()).unwrap();
    assert_eq!(ferry_source["type"], "note");
    assert_eq!(ferry_source["title"], "What helped at the ferry desk");
    let excerpt = &ferry_source["excerpts"][0];
    assert_eq!(excerpt["line"], 2);
    assert_eq!(
        excerpt["text"],
        "Giving each visitor a route card to mark in private worked better."
    );
    assert_eq!(
        ferry_source["reasons"]["shared_words"],
        serde_json::json!(["card", "private", "route"])
    );
    // The revision is the one fragment.get reports, so a quote can be checked.
    let got = h.send_rpc(rpc(3, "fragment.get", serde_json::json!({ "id": &ferry })));
    assert_eq!(ferry_source["revision"], result_of(&got)["revision"]);

    // A saved fragment by ID prefix: never itself.
    let resp = h.send_rpc(rpc(
        4,
        "recall.context",
        serde_json::json!({ "id": &orientation[..10], "limit": 1 }),
    ));
    let ctx = result_of(&resp);
    assert_eq!(ctx["seed"]["id"], orientation.as_str());
    assert_eq!(ctx["sources"][0]["id"], ferry.as_str());
    assert_eq!(ctx["sources"].as_array().unwrap().len(), 1);

    // Unsaved edits to a saved fragment: the saved copy is left out.
    let resp = h.send_rpc(rpc(
        5,
        "recall.context",
        serde_json::json!({ "id": &ferry, "text": "ferry desk route card, private" }),
    ));
    let ids: Vec<&str> = result_of(&resp)["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![orientation.as_str()]);
}

#[test]
fn test_recall_context_rejects_unclear_requests() {
    let mut h = ServerHarness::new();
    for params in [
        serde_json::json!({}),
        serde_json::json!({ "limit": 3 }),
        serde_json::json!({ "text": "x", "query": "y" }),
        serde_json::json!({ "text": "x", "limit": 0 }),
    ] {
        let resp = h.send_rpc(rpc(1, "recall.context", params.clone()));
        assert_eq!(error_of(&resp)["code"], -32602, "{params}");
    }
    let resp = h.send_rpc(rpc(
        2,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Saved" }),
    ));
    let id = result_of(&resp)["id"].as_str().unwrap().to_string();
    let resp = h.send_rpc(rpc(
        3,
        "recall.context",
        serde_json::json!({ "id": id, "tags": ["x"] }),
    ));
    assert_eq!(error_of(&resp)["code"], -32602, "tags without text");
    let resp = h.send_rpc(rpc(
        4,
        "recall.context",
        serde_json::json!({ "id": "01JQ7V3XKP5GQZ2N8R6T1WBMVH" }),
    ));
    assert!(resp.get("error").is_some(), "an unknown ID is an error");
}

#[test]
fn test_promote_templates_and_trash_management() {
    let mut h = ServerHarness::new();

    // schema.list exposes non-empty body templates (e.g. decision) and null for blank ones (note).
    let resp = h.send_rpc(rpc(1, "schema.list", serde_json::json!({})));
    let schemas = result_of(&resp).as_array().unwrap();
    let decision = schemas.iter().find(|s| s["name"] == "decision").unwrap();
    assert!(decision["template"]
        .as_str()
        .unwrap()
        .contains("## Context"));
    let note_schema = schemas.iter().find(|s| s["name"] == "note").unwrap();
    assert!(note_schema["template"].is_null());

    // Promote a note to a todo; preserves body/tags and fills defaults.
    let resp = h.send_rpc(rpc(
        2,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Ship v2", "body": "Checklist", "tags": ["release"] }),
    ));
    let created = result_of(&resp).clone();
    let id = created["id"].as_str().unwrap().to_string();
    let rev = created["revision"].as_str().unwrap().to_string();

    let resp = h.send_rpc(rpc(
        3,
        "fragment.promote",
        serde_json::json!({ "id": &id, "type": "todo", "expected_revision": &rev }),
    ));
    let promoted = result_of(&resp).clone();
    assert_eq!(promoted["type"], "todo");
    assert_eq!(promoted["status"], "open");
    assert_eq!(promoted["body"].as_str().unwrap().trim(), "Checklist");
    assert_eq!(promoted["tags"], serde_json::json!(["release"]));
    assert_ne!(promoted["revision"], created["revision"]);

    // Stale expected_revision is refused on promote.
    let resp = h.send_rpc(rpc(
        4,
        "fragment.promote",
        serde_json::json!({ "id": &id, "type": "risk", "expected_revision": &rev }),
    ));
    assert_eq!(error_of(&resp)["code"], -32010);

    // Trash list + purge (single and empty).
    let resp = h.send_rpc(rpc(
        5,
        "fragment.create",
        serde_json::json!({ "type": "note", "title": "Drop me" }),
    ));
    let id2 = result_of(&resp)["id"].as_str().unwrap().to_string();
    h.send_rpc(rpc(6, "fragment.delete", serde_json::json!({ "id": &id })));
    h.send_rpc(rpc(7, "fragment.delete", serde_json::json!({ "id": &id2 })));

    let resp = h.send_rpc(rpc(8, "trash.list", serde_json::json!({})));
    let trashed = result_of(&resp).as_array().unwrap();
    assert_eq!(trashed.len(), 2);

    let resp = h.send_rpc(rpc(9, "trash.purge", serde_json::json!({ "id": &id2 })));
    assert_eq!(result_of(&resp)["purged"], id2.as_str());
    let resp = h.send_rpc(rpc(10, "trash.list", serde_json::json!({})));
    assert_eq!(result_of(&resp).as_array().unwrap().len(), 1);

    let resp = h.send_rpc(rpc(11, "trash.purge", serde_json::json!({})));
    assert_eq!(result_of(&resp)["purged_count"], 1);
    let resp = h.send_rpc(rpc(12, "trash.list", serde_json::json!({})));
    assert_eq!(result_of(&resp).as_array().unwrap().len(), 0);
}

#[test]
fn inbox_rpc_stays_responsive_while_a_batch_waits_for_extraction() {
    use std::time::{Duration, SystemTime};

    let mut server = ServerHarness::new();
    let conn = parc_core::index::open_index(&server.vault_path).unwrap();
    // Hold the worker lease instead of sleeping or relying on a slow parser.
    assert!(parc_core::extract::claim_lease(&conn, "test-controlled-delay", 60).unwrap());
    for i in 0..8 {
        let file = std::fs::File::create(server.vault_path.join("inbox").join(format!("{i}.txt")))
            .unwrap();
        (&file).write_all(b"Original contents").unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(5))
            .unwrap();
    }
    let report = server.send_rpc(rpc(1, "inbox.sweep", serde_json::json!({})));
    assert_eq!(report["result"]["pending"], 8);
    let id = report["result"]["created"][0]["id"].as_str().unwrap();
    let read = server.send_rpc(rpc(2, "fragment.get", serde_json::json!({ "id": id })));
    assert_eq!(read["result"]["id"], id);
    let snapshot = server.send_rpc(rpc(3, "inbox.attachments", serde_json::json!({})));
    assert_eq!(snapshot["result"].as_object().unwrap().len(), 8);
    assert_eq!(snapshot["result"][id][0]["status"], "pending");
    let text = server.send_rpc(rpc(
        4,
        "attachment.text",
        serde_json::json!({
            "id": id, "filename": "0.txt", "ready_only": true
        }),
    ));
    assert_eq!(text["result"]["status"], "pending");
}
