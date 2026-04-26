use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn parc() -> Command {
    #[allow(deprecated)]
    Command::cargo_bin("parc").unwrap()
}

fn init_vault(dir: &TempDir) -> String {
    let vault_path = dir.path().join(".parc");
    parc()
        .args(["init"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized local vault"));
    vault_path.to_str().unwrap().to_string()
}

/// Create a fragment non-interactively by writing directly, then reindex.
/// (Integration tests can't open $EDITOR, so we use a helper approach.)
fn create_fragment_directly(dir: &TempDir, type_name: &str, title: &str, body: &str) -> String {
    let vault_path = dir.path().join(".parc");

    // Use parc-core directly to create a fragment
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve(type_name).unwrap();

    let mut fragment = parc_core::fragment::new_fragment(type_name, title, schema, &config);
    fragment.body = body.to_string();

    let id = parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();

    // Index it
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    id
}

#[test]
fn test_init_local() {
    let tmp = TempDir::new().unwrap();
    parc()
        .args(["init"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized local vault"));

    assert!(tmp.path().join(".parc/config.yml").exists());
    assert!(tmp.path().join(".parc/schemas/todo.yml").exists());
    assert!(tmp.path().join(".parc/fragments").is_dir());
}

#[test]
fn test_init_global() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().to_str().unwrap();

    parc()
        .args(["init", "--global"])
        .env("HOME", home)
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized global vault"));

    assert!(tmp.path().join(".parc/config.yml").exists());
}

#[test]
fn test_init_already_exists() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["init"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("vault already exists"));
}

#[test]
fn test_types() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["types"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("note"))
        .stdout(predicate::str::contains("todo"))
        .stdout(predicate::str::contains("decision"))
        .stdout(predicate::str::contains("risk"))
        .stdout(predicate::str::contains("idea"));
}

#[test]
fn test_list_empty() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No fragments found"));
}

#[test]
fn test_full_lifecycle() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    // Create a fragment directly (can't use $EDITOR in tests)
    let id = create_fragment_directly(&tmp, "note", "Test note", "Hello world");

    // List should show it
    parc()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Test note"));

    // Show should render it
    parc()
        .args(["show", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Test note"))
        .stdout(predicate::str::contains("Hello world"));

    // Show --json should output JSON
    parc()
        .args(["show", &id[..8], "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"Test note\""));

    // Set title
    parc()
        .args(["set", &id[..8], "title", "Updated note"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated"));

    // Show should reflect the change
    parc()
        .args(["show", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated note"));

    // Search should find it
    parc()
        .args(["search", "Updated"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated note"));

    // Delete
    parc()
        .args(["delete", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted"));

    // Should be gone from list
    parc()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No fragments found"));

    // File should be in trash
    assert!(tmp
        .path()
        .join(".parc/trash")
        .join(format!("{}.md", id))
        .exists());
}

#[test]
fn test_quick_capture_creates_note_without_editor() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let assert = parc()
        .args(["+", "Look into connection pooling", "--tag", "backend"])
        .env("EDITOR", "false")
        .current_dir(tmp.path())
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let id = stdout.trim();
    let fragment = parc_core::fragment::read_fragment(&tmp.path().join(".parc"), id).unwrap();

    assert_eq!(fragment.fragment_type, "note");
    assert_eq!(fragment.title, "Look into connection pooling");
    assert_eq!(fragment.body, "");
    assert_eq!(fragment.tags, vec!["backend".to_string()]);
}

#[test]
fn test_quick_capture_from_stdin_uses_first_line_as_title() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let assert = parc()
        .arg("+")
        .write_stdin("Scratch note\nLine one\nLine two\n")
        .env("EDITOR", "false")
        .current_dir(tmp.path())
        .assert()
        .success();

    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let id = stdout.trim();
    let fragment = parc_core::fragment::read_fragment(&tmp.path().join(".parc"), id).unwrap();

    assert_eq!(fragment.fragment_type, "note");
    assert_eq!(fragment.title, "Scratch note");
    assert_eq!(fragment.body.trim(), "Line one\nLine two");
}

#[test]
fn test_promote_note_to_todo() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Promote me", "Body");

    parc()
        .args([
            "promote",
            &id[..8],
            "todo",
            "--priority",
            "high",
            "--due",
            "2026-03-01",
        ])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Promoted"));

    let fragment = parc_core::fragment::read_fragment(&tmp.path().join(".parc"), &id).unwrap();
    assert_eq!(fragment.fragment_type, "todo");
    assert_eq!(fragment.title, "Promote me");
    assert_eq!(fragment.body.trim(), "Body");
    assert_eq!(
        fragment.extra_fields.get("status"),
        Some(&serde_json::Value::String("open".to_string()))
    );
    assert_eq!(
        fragment.extra_fields.get("priority"),
        Some(&serde_json::Value::String("high".to_string()))
    );
    assert_eq!(
        fragment.extra_fields.get("due"),
        Some(&serde_json::Value::String("2026-03-01".to_string()))
    );

    parc()
        .args(["list", "todo"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Promote me"));
}

#[test]
fn test_todo_with_fields() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("todo").unwrap();

    let mut fragment = parc_core::fragment::new_fragment("todo", "Buy groceries", schema, &config);
    fragment.extra_fields.insert(
        "priority".to_string(),
        serde_json::Value::String("high".to_string()),
    );
    fragment.extra_fields.insert(
        "due".to_string(),
        serde_json::Value::String("2026-03-01".to_string()),
    );

    let id = parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    // List with type filter
    parc()
        .args(["list", "todo"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Buy groceries"));

    // List with status filter
    parc()
        .args(["list", "todo", "--status", "open"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Buy groceries"));

    // Set status
    parc()
        .args(["set", &id[..8], "status", "done"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Filter by done
    parc()
        .args(["list", "todo", "--status", "done"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Buy groceries"));

    // Invalid enum value should error
    parc()
        .args(["set", &id[..8], "status", "invalid"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn test_search_fts() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "SQLite indexing", "Using FTS5 for full-text search");
    create_fragment_directly(&tmp, "note", "Redis caching", "Key-value store for caching");

    // Search for SQLite
    parc()
        .args(["search", "SQLite"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("SQLite indexing"))
        .stdout(predicate::str::contains("Redis").not());

    // Search with type filter (DSL syntax)
    parc()
        .args(["search", "caching", "type:note"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Redis caching"));

    // Search with --json
    parc()
        .args(["search", "SQLite", "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"SQLite indexing\""));
}

#[test]
fn test_hashtag_extraction() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("note").unwrap();

    let mut fragment =
        parc_core::fragment::new_fragment("note", "Hashtag test", schema, &config);
    fragment.tags = vec!["explicit".to_string()];
    fragment.body = "This has #inline-tag and #another tag.\n\nCode: `#not-a-tag`\n".to_string();

    let id = parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    // Search by inline tag (DSL syntax)
    parc()
        .args(["search", "#inline-tag"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hashtag test"));

    // Search by explicit tag (DSL syntax)
    parc()
        .args(["search", "tag:explicit"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hashtag test"));

    // Show should display merged tags
    parc()
        .args(["show", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("inline-tag"))
        .stdout(predicate::str::contains("explicit"));
}

#[test]
fn test_reindex() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Before reindex", "Body text");

    // Delete the index
    let index_path = tmp.path().join(".parc/index.db");
    std::fs::remove_file(&index_path).unwrap();

    // List should fail or be empty (no index)
    // Reindex should rebuild
    parc()
        .args(["reindex"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Reindexed 1 fragments"));

    // Now list should work
    parc()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Before reindex"));
}

#[test]
fn test_list_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "JSON test", "Body");

    parc()
        .args(["list", "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"JSON test\""));
}

#[test]
fn test_delete_nonexistent() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["delete", "NONEXISTENT"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("fragment not found"));
}

#[test]
fn test_list_with_tag_filter() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("note").unwrap();

    let mut frag1 = parc_core::fragment::new_fragment("note", "Tagged A", schema, &config);
    frag1.tags = vec!["alpha".to_string(), "beta".to_string()];
    parc_core::fragment::create_fragment(&vault_path, &frag1).unwrap();
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &frag1, &vault_path).unwrap();

    let mut frag2 = parc_core::fragment::new_fragment("note", "Tagged B", schema, &config);
    frag2.tags = vec!["alpha".to_string()];
    parc_core::fragment::create_fragment(&vault_path, &frag2).unwrap();
    parc_core::index::index_fragment_auto(&conn, &frag2, &vault_path).unwrap();

    // Filter by alpha — both should appear
    parc()
        .args(["list", "--tag", "alpha"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Tagged A"))
        .stdout(predicate::str::contains("Tagged B"));

    // Filter by both alpha AND beta — only first
    parc()
        .args(["list", "--tag", "alpha", "--tag", "beta"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Tagged A"))
        .stdout(predicate::str::contains("Tagged B").not());
}

#[test]
fn test_search_no_results() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["search", "nonexistent"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No fragments found"));
}

// ===== M1: Links & Navigation =====

#[test]
fn test_link_and_backlinks() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_a = create_fragment_directly(&tmp, "note", "Note A", "First note");
    let id_b = create_fragment_directly(&tmp, "note", "Note B", "Second note");

    // Link A <-> B (use full IDs to avoid ULID prefix collisions)
    parc()
        .args(["link", &id_a, &id_b])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Linked"));

    // Show A should have B in links metadata
    parc()
        .args(["show", &id_a])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(&id_b));

    // Show B should have A in backlinks (frontmatter link is bidirectional)
    // After linking, B's frontmatter also has A, so show B should mention A
    parc()
        .args(["show", &id_b])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(&id_a));

    // Backlinks of B should include A
    parc()
        .args(["backlinks", &id_b])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Note A"));

    // Backlinks --json
    parc()
        .args(["backlinks", &id_b, "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"Note A\""));

    // Already linked — idempotent
    parc()
        .args(["link", &id_a, &id_b])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Already linked"));
}

#[test]
fn test_unlink() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_a = create_fragment_directly(&tmp, "note", "Note A", "First");
    let id_b = create_fragment_directly(&tmp, "note", "Note B", "Second");

    // Link then unlink (use full IDs to avoid ULID prefix collisions)
    parc()
        .args(["link", &id_a, &id_b])
        .current_dir(tmp.path())
        .assert()
        .success();

    parc()
        .args(["unlink", &id_a, &id_b])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Unlinked"));

    // Backlinks should be empty
    parc()
        .args(["backlinks", &id_b])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No backlinks found"));

    // Unlink again — not linked
    parc()
        .args(["unlink", &id_a, &id_b])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Not linked"));
}

#[test]
fn test_link_self_error() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Self", "Body");

    parc()
        .args(["link", &id[..8], &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Cannot link a fragment to itself"));
}

#[test]
fn test_show_backlinks_section() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_a = create_fragment_directly(&tmp, "note", "Target note", "I am the target");
    let id_b = create_fragment_directly(&tmp, "note", "Linking note", "I link to target");

    // Create link B -> A (use full IDs to avoid ULID prefix collisions)
    parc()
        .args(["link", &id_b, &id_a])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Show A should have backlinks section with B
    parc()
        .args(["show", &id_a])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Backlinks"))
        .stdout(predicate::str::contains("Linking note"));

    // Show A --json should include backlinks
    parc()
        .args(["show", &id_a, "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"backlinks\""))
        .stdout(predicate::str::contains("\"title\": \"Linking note\""));
}

#[test]
fn test_show_no_backlinks_section_when_empty() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Lonely note", "No links");

    // Show should NOT have backlinks section
    parc()
        .args(["show", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Backlinks").not());
}

#[test]
fn test_wiki_link_in_body_creates_backlink() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_a = create_fragment_directly(&tmp, "note", "Target", "I am the target");

    // Create a note with a wiki-link to A in the body
    let vault_path = tmp.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("note").unwrap();

    let mut fragment = parc_core::fragment::new_fragment("note", "Linker via body", schema, &config);
    fragment.body = format!("Check out [[{}]] for details.", id_a);

    let id_b = parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    // Backlinks of A should include B (via body wiki-link)
    parc()
        .args(["backlinks", &id_a])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Linker via body"));

    // Show A should have backlinks section
    parc()
        .args(["show", &id_a])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Backlinks"))
        .stdout(predicate::str::contains("Linker via body"));

    let _ = id_b; // suppress unused warning
}

#[test]
fn test_title_wiki_link_in_body_creates_backlink() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_a = create_fragment_directly(&tmp, "note", "Auth refactor", "I am the target");

    let vault_path = tmp.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("note").unwrap();

    let mut fragment = parc_core::fragment::new_fragment("note", "Linker via title", schema, &config);
    fragment.body = "Check out [[Auth refactor]] for details.".to_string();

    parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    parc()
        .args(["backlinks", &id_a])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Linker via title"));
}

#[test]
fn test_doctor_healthy() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_a = create_fragment_directly(&tmp, "note", "Note A", "Body A");
    let id_b = create_fragment_directly(&tmp, "note", "Note B", "Body B");

    // Link them so neither is orphan (use full IDs to avoid ULID prefix collisions)
    parc()
        .args(["link", &id_a, &id_b])
        .current_dir(tmp.path())
        .assert()
        .success();

    parc()
        .args(["doctor"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("no issues found"));
}

#[test]
fn test_doctor_broken_link() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    // Create a fragment with a broken frontmatter link
    let vault_path = tmp.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("note").unwrap();

    let mut fragment = parc_core::fragment::new_fragment("note", "Broken linker", schema, &config);
    fragment.links = vec!["NONEXISTENT_ID_12345".to_string()];
    parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    // Doctor should find the broken link and exit nonzero
    parc()
        .args(["doctor"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("Broken link"));
}

#[test]
fn test_doctor_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Solo note", "Body");

    parc()
        .args(["doctor", "--json"])
        .current_dir(tmp.path())
        .assert()
        // Orphan note will cause findings, but JSON output should work either way
        .stdout(predicate::str::contains("\"fragments_checked\""))
        .stdout(predicate::str::contains("\"findings\""));
}

#[test]
fn test_doctor_orphan() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Orphan note", "No links at all");

    // Doctor should report the orphan (exit nonzero since orphans are findings)
    parc()
        .args(["doctor"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("Orphan"))
        .stdout(predicate::str::contains("Orphan note"));
}

// ===== M2: Multi-Vault =====

#[test]
fn test_vault_flag_overrides_discovery() {
    let tmp = TempDir::new().unwrap();
    let vault_path = tmp.path().join(".parc");

    // Init via --vault flag
    parc()
        .args(["--vault", tmp.path().to_str().unwrap(), "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized new vault"));

    assert!(vault_path.join("config.yml").exists());

    // Create fragment via --vault
    let vault_str = vault_path.to_str().unwrap();
    create_fragment_directly(&tmp, "note", "Vault flag test", "Body");

    // List via --vault flag (from a different CWD)
    let other_dir = TempDir::new().unwrap();
    parc()
        .args(["--vault", vault_str, "list"])
        .current_dir(other_dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Vault flag test"));
}

#[test]
fn test_vault_flag_without_parc_suffix() {
    let tmp = TempDir::new().unwrap();

    // --vault /path (without .parc) should append .parc for init
    parc()
        .args(["--vault", tmp.path().to_str().unwrap(), "init"])
        .assert()
        .success();

    assert!(tmp.path().join(".parc/config.yml").exists());

    // --vault /path (without .parc) should find the vault for other commands
    parc()
        .args(["--vault", tmp.path().to_str().unwrap(), "types"])
        .assert()
        .success()
        .stdout(predicate::str::contains("note"));
}

#[test]
fn test_vault_flag_with_parc_suffix() {
    let tmp = TempDir::new().unwrap();
    let vault_path = tmp.path().join(".parc");

    // Init normally
    parc()
        .args(["init"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // --vault with .parc suffix should work
    parc()
        .args(["--vault", vault_path.to_str().unwrap(), "types"])
        .assert()
        .success()
        .stdout(predicate::str::contains("note"));
}

#[test]
fn test_parc_vault_env() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Env var test", "Body");

    let vault_path = tmp.path().join(".parc");
    let other_dir = TempDir::new().unwrap();

    // PARC_VAULT env var should override discovery
    parc()
        .args(["list"])
        .env("PARC_VAULT", vault_path.to_str().unwrap())
        .current_dir(other_dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Env var test"));
}

#[test]
fn test_vault_flag_overrides_env() {
    let tmp1 = TempDir::new().unwrap();
    init_vault(&tmp1);
    create_fragment_directly(&tmp1, "note", "From vault1", "Body");

    let tmp2 = TempDir::new().unwrap();
    init_vault(&tmp2);
    create_fragment_directly(&tmp2, "note", "From vault2", "Body");

    let vault1_path = tmp1.path().join(".parc");
    let vault2_path = tmp2.path().join(".parc");

    // --vault flag should override PARC_VAULT env
    parc()
        .args(["--vault", vault1_path.to_str().unwrap(), "list"])
        .env("PARC_VAULT", vault2_path.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains("From vault1"))
        .stdout(predicate::str::contains("From vault2").not());
}

#[test]
fn test_init_vault_at_arbitrary_path() {
    let tmp = TempDir::new().unwrap();
    let target = tmp.path().join("custom-location");
    std::fs::create_dir_all(&target).unwrap();

    parc()
        .args(["--vault", target.to_str().unwrap(), "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized new vault"));

    assert!(target.join(".parc/config.yml").exists());
    assert!(target.join(".parc/fragments").is_dir());

    // Use the newly created vault
    let vault_path = target.join(".parc");
    parc()
        .args(["--vault", vault_path.to_str().unwrap(), "types"])
        .assert()
        .success()
        .stdout(predicate::str::contains("note"));
}

#[test]
fn test_init_vault_global_and_vault_conflict() {
    let tmp = TempDir::new().unwrap();

    parc()
        .args(["--vault", tmp.path().to_str().unwrap(), "init", "--global"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("mutually exclusive"));
}

#[test]
fn test_vault_command_shows_info() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Info test", "Body");

    parc()
        .args(["vault"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Active vault:"))
        .stdout(predicate::str::contains("Scope:"))
        .stdout(predicate::str::contains("Fragments:"));
}

#[test]
fn test_vault_command_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["vault", "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"path\""))
        .stdout(predicate::str::contains("\"scope\""))
        .stdout(predicate::str::contains("\"fragment_count\""));
}

#[test]
fn test_vault_command_with_vault_flag() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");

    // parc --vault <path> vault should work from any CWD
    let other_dir = TempDir::new().unwrap();
    parc()
        .args(["--vault", vault_path.to_str().unwrap(), "vault"])
        .current_dir(other_dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Active vault:"));
}

#[test]
fn test_vault_list() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["vault", "list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("SCOPE"))
        .stdout(predicate::str::contains("PATH"))
        .stdout(predicate::str::contains("FRAGMENTS"));
}

#[test]
fn test_vault_list_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["vault", "list", "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"path\""))
        .stdout(predicate::str::contains("\"scope\""));
}

#[test]
fn test_local_vault_discovery_from_subdir() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Deep discovery", "Body");

    // Create a deep subdirectory
    let subdir = tmp.path().join("a/b/c");
    std::fs::create_dir_all(&subdir).unwrap();

    // Commands from subdirectory should find the vault
    parc()
        .args(["list"])
        .current_dir(&subdir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Deep discovery"));
}

#[test]
fn test_all_commands_accept_vault_flag() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);
    let vault_path = tmp.path().join(".parc");
    let vault_str = vault_path.to_str().unwrap();

    let id = create_fragment_directly(&tmp, "note", "Vault flag all cmds", "Body");
    let short_id = &id[..8];

    // list
    parc()
        .args(["--vault", vault_str, "list"])
        .assert()
        .success();

    // show
    parc()
        .args(["--vault", vault_str, "show", short_id])
        .assert()
        .success();

    // search
    parc()
        .args(["--vault", vault_str, "search", "Vault"])
        .assert()
        .success();

    // types
    parc()
        .args(["--vault", vault_str, "types"])
        .assert()
        .success();

    // reindex
    parc()
        .args(["--vault", vault_str, "reindex"])
        .assert()
        .success();

    // doctor
    let _ = parc()
        .args(["--vault", vault_str, "doctor"])
        .assert();

    // backlinks
    parc()
        .args(["--vault", vault_str, "backlinks", short_id])
        .assert()
        .success();

    // set
    parc()
        .args(["--vault", vault_str, "set", short_id, "title", "New title"])
        .assert()
        .success();

    // vault
    parc()
        .args(["--vault", vault_str, "vault"])
        .assert()
        .success();
}

// ===== M4: Templates, Aliases & Hooks =====

#[test]
fn test_schema_add() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    // Create a custom schema file
    let schema_content = r#"
name: snippet
alias: s
fields:
  - name: language
    type: string
    required: true
    default: text
"#;
    let schema_file = tmp.path().join("snippet.yml");
    std::fs::write(&schema_file, schema_content).unwrap();

    // Add the schema
    parc()
        .args(["schema", "add", schema_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Added schema 'snippet'"));

    // Types should now include snippet
    parc()
        .args(["types"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("snippet"));

    // Adding again should fail (duplicate)
    parc()
        .args(["schema", "add", schema_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    // Template should have been created
    assert!(tmp.path().join(".parc/templates/snippet.md").exists());
}

#[test]
fn test_schema_add_invalid() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    // Create an invalid schema file
    let bad_file = tmp.path().join("bad.yml");
    std::fs::write(&bad_file, "not: valid: yaml: [[[").unwrap();

    parc()
        .args(["schema", "add", bad_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .failure();
}

#[test]
fn test_schema_add_nonexistent_file() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    parc()
        .args(["schema", "add", "/nonexistent/path.yml"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("file not found"));
}

#[test]
fn test_due_date_today() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("todo").unwrap();

    let mut fragment = parc_core::fragment::new_fragment("todo", "Due today", schema, &config);
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    let resolved = parc_core::date::resolve_due_date("today").unwrap();
    assert_eq!(resolved, today);

    fragment.extra_fields.insert(
        "due".to_string(),
        serde_json::Value::String(resolved),
    );

    let id = parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();
    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    // Show should display today's date
    parc()
        .args(["show", &id[..8], "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(&today));
}

#[test]
fn test_due_date_tomorrow() {
    let resolved = parc_core::date::resolve_due_date("tomorrow").unwrap();
    let expected = (chrono::Local::now().date_naive() + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(resolved, expected);
}

#[test]
fn test_due_date_in_n_days() {
    let resolved = parc_core::date::resolve_due_date("in-5-days").unwrap();
    let expected = (chrono::Local::now().date_naive() + chrono::Duration::days(5))
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(resolved, expected);
}

#[test]
fn test_due_date_passthrough_iso() {
    let resolved = parc_core::date::resolve_due_date("2026-06-15").unwrap();
    assert_eq!(resolved, "2026-06-15");
}

#[test]
fn test_due_date_invalid() {
    assert!(parc_core::date::resolve_due_date("not-a-date").is_err());
}

#[test]
fn test_set_due_with_relative_date() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "todo", "Due test", "Body");
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();

    parc()
        .args(["set", &id[..8], "due", "today"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated"));

    // Verify the resolved date
    parc()
        .args(["show", &id[..8], "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(&today));
}

#[test]
fn test_hook_post_create() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let hooks_dir = tmp.path().join(".parc/hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();

    // Create a post-create hook that writes to a marker file
    let hook_script = hooks_dir.join("post-create");
    std::fs::write(
        &hook_script,
        "#!/bin/sh\ntouch \"$(dirname \"$0\")/../hook-fired\"\n",
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook_script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let id = create_fragment_directly(&tmp, "note", "Hook test", "Body");

    // The hook should have fired (marker file exists)
    // Note: create_fragment_directly bypasses CLI, so hooks won't fire there.
    // Let's verify hook discovery works
    let hooks = parc_core::hook::discover_hooks(
        &tmp.path().join(".parc"),
        parc_core::hook::HookEvent::PostCreate,
        "note",
    );
    assert_eq!(hooks.len(), 1);
    assert!(hooks[0].type_filter.is_none());

    let _ = id;
}

#[test]
fn test_hook_type_specific_discovery() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let hooks_dir = tmp.path().join(".parc/hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();

    // Create a generic and a type-specific hook
    std::fs::write(hooks_dir.join("pre-create"), "#!/bin/sh\n").unwrap();
    std::fs::write(hooks_dir.join("pre-create.todo"), "#!/bin/sh\n").unwrap();

    let vault_path = tmp.path().join(".parc");

    // For todos: both hooks
    let hooks = parc_core::hook::discover_hooks(
        &vault_path,
        parc_core::hook::HookEvent::PreCreate,
        "todo",
    );
    assert_eq!(hooks.len(), 2);

    // For notes: only generic
    let hooks = parc_core::hook::discover_hooks(
        &vault_path,
        parc_core::hook::HookEvent::PreCreate,
        "note",
    );
    assert_eq!(hooks.len(), 1);
}

#[test]
fn test_hook_no_hooks_dir() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");

    // No hooks dir — should return empty
    let hooks = parc_core::hook::discover_hooks(
        &vault_path,
        parc_core::hook::HookEvent::PostCreate,
        "note",
    );
    assert!(hooks.is_empty());
}

#[test]
fn test_completions_bash() {
    let tmp = TempDir::new().unwrap();

    parc()
        .args(["completions", "bash"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("parc"));
}

#[test]
fn test_completions_zsh() {
    let tmp = TempDir::new().unwrap();

    parc()
        .args(["completions", "zsh"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("parc"));
}

#[test]
fn test_completions_fish() {
    let tmp = TempDir::new().unwrap();

    parc()
        .args(["completions", "fish"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("parc"));
}

#[test]
fn test_completions_invalid_shell() {
    let tmp = TempDir::new().unwrap();

    parc()
        .args(["completions", "tcsh"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unsupported shell"));
}

// ===== M5: History & Attachments =====

#[test]
fn test_history_no_versions() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "No history", "Body");

    parc()
        .args(["history", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No history"));
}

#[test]
fn test_history_after_set() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "History test", "Original body");

    // Edit via set (triggers history snapshot)
    parc()
        .args(["set", &id[..8], "title", "Updated title"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // History should now list 1 version
    parc()
        .args(["history", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("1 versions"))
        .stdout(predicate::str::contains("TIMESTAMP"));
}

#[test]
fn test_history_show() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Show test", "Original body");

    // Set triggers snapshot
    parc()
        .args(["set", &id[..8], "title", "New title"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Get the timestamp from history list
    let output = parc()
        .args(["history", &id[..8]])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Extract timestamp (ISO format line)
    let timestamp = stdout
        .lines()
        .find(|l| l.contains("2026") || l.contains("202"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();

    // Show the old version
    parc()
        .args(["history", &id[..8], "--show", &timestamp])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Show test"));
}

#[test]
fn test_history_diff() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Diff test", "Line one\nLine two\n");

    // Change title
    parc()
        .args(["set", &id[..8], "title", "Changed title"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Diff should show changes
    parc()
        .args(["history", &id[..8], "--diff"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("-title: Diff test"))
        .stdout(predicate::str::contains("+title: Changed title"));
}

#[test]
fn test_history_restore() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Restore me", "Original");

    // Change title
    parc()
        .args(["set", &id[..8], "title", "Changed"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Get the timestamp
    let output = parc()
        .args(["history", &id[..8]])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let timestamp = stdout
        .lines()
        .find(|l| l.contains("202"))
        .and_then(|l| l.split_whitespace().next())
        .unwrap()
        .to_string();

    // Restore
    parc()
        .args(["history", &id[..8], "--restore", &timestamp])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored"));

    // Verify the title is back
    parc()
        .args(["show", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Restore me"));

    // Should now have 2 versions (original + pre-restore)
    parc()
        .args(["history", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("2 versions"));
}

#[test]
fn test_attach_and_list() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Attach test", "Body");

    // Create a test file to attach
    let test_file = tmp.path().join("test-file.txt");
    std::fs::write(&test_file, "attachment content").unwrap();

    // Attach
    parc()
        .args(["attach", &id[..8], test_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Attached 'test-file.txt'"));

    // List attachments
    parc()
        .args(["attachments", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("test-file.txt"));

    // Source file should still exist (copy mode)
    assert!(test_file.exists());
}

#[test]
fn test_attach_move() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Move test", "Body");

    let test_file = tmp.path().join("moveme.txt");
    std::fs::write(&test_file, "move content").unwrap();

    parc()
        .args(["attach", &id[..8], test_file.to_str().unwrap(), "--mv"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Source should be gone
    assert!(!test_file.exists());

    // Attachment should be listed
    parc()
        .args(["attachments", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("moveme.txt"));
}

#[test]
fn test_detach() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Detach test", "Body");

    let test_file = tmp.path().join("removeme.txt");
    std::fs::write(&test_file, "data").unwrap();

    parc()
        .args(["attach", &id[..8], test_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .success();

    parc()
        .args(["detach", &id[..8], "removeme.txt"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Detached"));

    parc()
        .args(["attachments", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No attachments"));
}

#[test]
fn test_attach_duplicate_error() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Dup test", "Body");

    let test_file = tmp.path().join("dup.txt");
    std::fs::write(&test_file, "data").unwrap();

    parc()
        .args(["attach", &id[..8], test_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .success();

    parc()
        .args(["attach", &id[..8], test_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn test_show_with_attachments() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Show attach", "Body");

    let test_file = tmp.path().join("screenshot.png");
    std::fs::write(&test_file, "fake png data").unwrap();

    parc()
        .args(["attach", &id[..8], test_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Show should display attachments section
    parc()
        .args(["show", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Attachments"))
        .stdout(predicate::str::contains("screenshot.png"));

    // Show --json should include attachments
    parc()
        .args(["show", &id[..8], "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"attachments\""))
        .stdout(predicate::str::contains("screenshot.png"));
}

#[test]
fn test_attachments_empty() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "No attach", "Body");

    parc()
        .args(["attachments", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No attachments"));
}

#[test]
fn test_show_no_attachments_section_when_empty() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "No attach show", "Body");

    parc()
        .args(["show", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Attachments").not());
}

#[test]
fn test_doctor_attachment_mismatch() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");
    let id = create_fragment_directly(&tmp, "note", "Mismatch test", "Body");

    // Create an attachment directory with an unreferenced file
    let attach_dir = vault_path.join("attachments").join(&id);
    std::fs::create_dir_all(&attach_dir).unwrap();
    std::fs::write(attach_dir.join("orphan.txt"), "orphan data").unwrap();

    parc()
        .args(["doctor"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains("Attachment"))
        .stdout(predicate::str::contains("not listed in frontmatter"));
}

#[test]
fn test_attachments_roundtrip_in_frontmatter() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let vault_path = tmp.path().join(".parc");
    let id = create_fragment_directly(&tmp, "note", "Frontmatter test", "Body");

    let test_file = tmp.path().join("doc.pdf");
    std::fs::write(&test_file, "pdf data").unwrap();

    parc()
        .args(["attach", &id[..8], test_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Read the raw file and verify attachments in frontmatter
    let fragment_path = vault_path.join("fragments").join(format!("{}.md", id));
    let content = std::fs::read_to_string(&fragment_path).unwrap();
    assert!(content.contains("attachments:"));
    assert!(content.contains("  - doc.pdf"));
}

// ═══════════════════════════════════════════════════════════════════════
// M6: Quality of Life
// ═══════════════════════════════════════════════════════════════════════

// --- Tags ---

fn create_fragment_with_tags(dir: &TempDir, title: &str, tags: &[&str]) -> String {
    let vault_path = dir.path().join(".parc");
    let config = parc_core::config::load_config(&vault_path).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault_path).unwrap();
    let schema = schemas.resolve("note").unwrap();

    let mut fragment = parc_core::fragment::new_fragment("note", title, schema, &config);
    fragment.tags = tags.iter().map(|s| s.to_string()).collect();

    let id = parc_core::fragment::create_fragment(&vault_path, &fragment).unwrap();

    let conn = parc_core::index::open_index(&vault_path).unwrap();
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

    id
}

#[test]
fn test_tags_aggregation() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_with_tags(&tmp, "Note 1", &["alpha", "beta"]);
    create_fragment_with_tags(&tmp, "Note 2", &["alpha", "gamma"]);
    create_fragment_with_tags(&tmp, "Note 3", &["alpha"]);

    parc()
        .args(["tags"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"))
        .stdout(predicate::str::contains("gamma"));
}

#[test]
fn test_tags_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_with_tags(&tmp, "Note 1", &["rust", "cli"]);

    let output = parc()
        .args(["tags", "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert!(arr.len() >= 2);
    assert!(arr.iter().any(|v| v["tag"] == "rust"));
    assert!(arr.iter().any(|v| v["tag"] == "cli"));
}

// --- Archive ---

#[test]
fn test_archive_and_unarchive() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Archive me", "Body");

    // Archive
    parc()
        .args(["archive", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Archived"));

    // Should not appear in default list
    parc()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Archive me").not());

    // Should appear with is:archived search
    parc()
        .args(["search", "is:archived"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Archive me"));

    // Unarchive
    parc()
        .args(["archive", &id[..8], "--undo"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Unarchived"));

    // Should appear again in default list
    parc()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Archive me"));
}

#[test]
fn test_archive_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Archive JSON", "Body");

    let output = parc()
        .args(["archive", &id[..8], "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["archived"], true);
}

// --- Trash ---

#[test]
fn test_trash_list_and_restore() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Trash me", "Body");

    parc()
        .args(["delete", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Should appear in trash
    parc()
        .args(["trash"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Trash me"));

    // Restore
    parc()
        .args(["trash", "--restore", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Restored"));

    // Should appear in list again
    parc()
        .args(["list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Trash me"));
}

#[test]
fn test_trash_purge() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Purge me", "Body");

    parc()
        .args(["delete", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success();

    parc()
        .args(["trash", "--purge"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Purged"));

    // Trash should be empty now
    parc()
        .args(["trash"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Trash is empty"));
}

#[test]
fn test_trash_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Trash JSON", "Body");

    parc()
        .args(["delete", &id[..8]])
        .current_dir(tmp.path())
        .assert()
        .success();

    let output = parc()
        .args(["trash", "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["title"], "Trash JSON");
}

// --- Export / Import ---

#[test]
fn test_export_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Export test 1", "Body one");
    create_fragment_directly(&tmp, "note", "Export test 2", "Body two");

    let output = parc()
        .args(["export", "--format", "json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn test_export_csv() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "CSV test", "Body");

    let output = parc()
        .args(["export", "--format", "csv"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("id,type,title,"));
    assert!(stdout.contains("CSV test"));
}

#[test]
fn test_export_html() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "HTML test", "Body");

    let out_dir = tmp.path().join("html-export");
    parc()
        .args([
            "export",
            "--format",
            "html",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported"));

    assert!(out_dir.join("index.html").exists());
}

#[test]
fn test_export_to_file() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "File export", "Body");

    let out_file = tmp.path().join("export.json");
    parc()
        .args([
            "export",
            "--format",
            "json",
            "--output",
            out_file.to_str().unwrap(),
        ])
        .current_dir(tmp.path())
        .assert()
        .success();

    let content = std::fs::read_to_string(&out_file).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(json.as_array().unwrap().len() >= 1);
}

#[test]
fn test_import_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Pre-existing", "Body");

    // Export first
    let out_file = tmp.path().join("export.json");
    parc()
        .args([
            "export",
            "--format",
            "json",
            "--output",
            out_file.to_str().unwrap(),
        ])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Create a fresh vault and import
    let tmp2 = TempDir::new().unwrap();
    init_vault(&tmp2);

    parc()
        .args(["import", out_file.to_str().unwrap()])
        .current_dir(tmp2.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("1 created"));

    // Verify imported fragment exists
    parc()
        .args(["list"])
        .current_dir(tmp2.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Pre-existing"));
}

#[test]
fn test_import_dry_run() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Dry run test", "Body");

    let out_file = tmp.path().join("export.json");
    parc()
        .args([
            "export",
            "--format",
            "json",
            "--output",
            out_file.to_str().unwrap(),
        ])
        .current_dir(tmp.path())
        .assert()
        .success();

    let tmp2 = TempDir::new().unwrap();
    init_vault(&tmp2);

    parc()
        .args(["import", out_file.to_str().unwrap(), "--dry-run"])
        .current_dir(tmp2.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("[dry-run]"));

    // Verify nothing was actually imported
    parc()
        .args(["list"])
        .current_dir(tmp2.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No fragments found"));
}

// --- Git hooks ---

#[test]
fn test_git_hooks_install() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    // Init a git repo
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    parc()
        .args(["git-hooks", "install"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Installed post-merge hook"));

    let hook_path = tmp.path().join(".git/hooks/post-merge");
    assert!(hook_path.exists());
    let content = std::fs::read_to_string(&hook_path).unwrap();
    assert!(content.contains("parc reindex"));
}

#[test]
fn test_git_hooks_install_idempotent() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    // Install twice
    parc()
        .args(["git-hooks", "install"])
        .current_dir(tmp.path())
        .assert()
        .success();

    parc()
        .args(["git-hooks", "install"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("already contains"));
}

// --- --json on various commands ---

#[test]
fn test_delete_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Delete JSON", "Body");

    let output = parc()
        .args(["delete", &id[..8], "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["deleted"], true);
}

#[test]
fn test_set_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Set JSON", "Body");

    let output = parc()
        .args(["set", &id[..8], "title", "New Title", "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["updated"], true);
    assert_eq!(json["field"], "title");
}

#[test]
fn test_types_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let output = parc()
        .args(["types", "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert!(arr.len() >= 5); // 5 built-in types
    assert!(arr.iter().any(|v| v["name"] == "todo"));
}

#[test]
fn test_reindex_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_directly(&tmp, "note", "Reindex JSON", "Body");

    let output = parc()
        .args(["reindex", "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["fragments_indexed"], 1);
}

#[test]
fn test_history_json() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "History JSON", "Body v1");

    // Make a change
    parc()
        .args(["set", &id[..8], "title", "History JSON v2"])
        .current_dir(tmp.path())
        .assert()
        .success();

    let output = parc()
        .args(["history", &id[..8], "--json"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["versions"].as_array().unwrap().len() >= 1);
}

// --- has:attachments search ---

#[test]
fn test_search_has_attachments() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_with = create_fragment_directly(&tmp, "note", "With attachment", "Body");
    let _id_without = create_fragment_directly(&tmp, "note", "Without attachment", "Body");

    // Attach a file (use full ID to avoid ambiguity)
    let test_file = tmp.path().join("test.txt");
    std::fs::write(&test_file, "test data").unwrap();
    parc()
        .args(["attach", &id_with, test_file.to_str().unwrap()])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Search with has:attachments
    parc()
        .args(["search", "has:attachments"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("With attachment"))
        .stdout(predicate::str::contains("Without attachment").not());
}

// --- is:all search ---

#[test]
fn test_search_is_all() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id = create_fragment_directly(&tmp, "note", "Archived note", "Body");
    create_fragment_directly(&tmp, "note", "Active note", "Body");

    // Archive one (use full ID to avoid ambiguity)
    parc()
        .args(["archive", &id])
        .current_dir(tmp.path())
        .assert()
        .success();

    // is:all should show both
    parc()
        .args(["search", "is:all"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Archived note"))
        .stdout(predicate::str::contains("Active note"));
}

// --- Export with filter ---

#[test]
fn test_export_with_filter() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    create_fragment_with_tags(&tmp, "Tagged note", &["special"]);
    create_fragment_directly(&tmp, "note", "Untagged note", "Body");

    let output = parc()
        .args(["export", "--format", "json", "tag:special"])
        .current_dir(tmp.path())
        .output()
        .unwrap();

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["title"], "Tagged note");
}
