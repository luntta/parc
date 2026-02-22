use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn parc() -> Command {
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
    parc_core::index::index_fragment_auto(&conn, &fragment).unwrap();

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
    parc_core::index::index_fragment_auto(&conn, &fragment).unwrap();

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

    // Search with type filter
    parc()
        .args(["search", "caching", "--type", "note"])
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
    parc_core::index::index_fragment_auto(&conn, &fragment).unwrap();

    // Search by inline tag
    parc()
        .args(["search", "--tag", "inline-tag"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Hashtag test"));

    // Search by explicit tag
    parc()
        .args(["search", "--tag", "explicit"])
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
    parc_core::index::index_fragment_auto(&conn, &frag1).unwrap();

    let mut frag2 = parc_core::fragment::new_fragment("note", "Tagged B", schema, &config);
    frag2.tags = vec!["alpha".to_string()];
    parc_core::fragment::create_fragment(&vault_path, &frag2).unwrap();
    parc_core::index::index_fragment_auto(&conn, &frag2).unwrap();

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
