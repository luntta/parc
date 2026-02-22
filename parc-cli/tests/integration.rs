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
    parc_core::index::index_fragment_auto(&conn, &fragment, &vault_path).unwrap();

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

    // Link A <-> B
    parc()
        .args(["link", &id_a[..8], &id_b[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Linked"));

    // Show A should have B in links metadata
    parc()
        .args(["show", &id_a[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(&id_b));

    // Show B should have A in backlinks (frontmatter link is bidirectional)
    // After linking, B's frontmatter also has A, so show B should mention A
    parc()
        .args(["show", &id_b[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(&id_a));

    // Backlinks of B should include A
    parc()
        .args(["backlinks", &id_b[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Note A"));

    // Backlinks --json
    parc()
        .args(["backlinks", &id_b[..8], "--json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"Note A\""));

    // Already linked — idempotent
    parc()
        .args(["link", &id_a[..8], &id_b[..8]])
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

    // Link then unlink
    parc()
        .args(["link", &id_a[..8], &id_b[..8]])
        .current_dir(tmp.path())
        .assert()
        .success();

    parc()
        .args(["unlink", &id_a[..8], &id_b[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Unlinked"));

    // Backlinks should be empty
    parc()
        .args(["backlinks", &id_b[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No backlinks found"));

    // Unlink again — not linked
    parc()
        .args(["unlink", &id_a[..8], &id_b[..8]])
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

    // Create link B -> A
    parc()
        .args(["link", &id_b[..8], &id_a[..8]])
        .current_dir(tmp.path())
        .assert()
        .success();

    // Show A should have backlinks section with B
    parc()
        .args(["show", &id_a[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Backlinks"))
        .stdout(predicate::str::contains("Linking note"));

    // Show A --json should include backlinks
    parc()
        .args(["show", &id_a[..8], "--json"])
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
        .args(["backlinks", &id_a[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Linker via body"));

    // Show A should have backlinks section
    parc()
        .args(["show", &id_a[..8]])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Backlinks"))
        .stdout(predicate::str::contains("Linker via body"));

    let _ = id_b; // suppress unused warning
}

#[test]
fn test_doctor_healthy() {
    let tmp = TempDir::new().unwrap();
    init_vault(&tmp);

    let id_a = create_fragment_directly(&tmp, "note", "Note A", "Body A");
    let id_b = create_fragment_directly(&tmp, "note", "Note B", "Body B");

    // Link them so neither is orphan
    parc()
        .args(["link", &id_a[..8], &id_b[..8]])
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
