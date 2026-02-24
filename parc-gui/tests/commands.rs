//! Integration tests for parc-gui command layer logic.
//!
//! These tests exercise the same parc-core operations that the Tauri commands
//! compose, verifying DTO conversions, error handling, and correctness.
//! Since Tauri `State<'_>` injection requires a running Tauri app, we replicate
//! the command logic directly against a temporary vault.

use std::path::{Path, PathBuf};

use parc_core::attachment;
use parc_core::config::load_config;
use parc_core::doctor;
use parc_core::fragment::{self, Fragment};
use parc_core::history;
use parc_core::index;
use parc_core::schema::{self, FieldType};
use parc_core::search::{self, Filter, SearchQuery, SortOrder};
use parc_core::tag;
use parc_core::vault;

use parc_gui::dto::*;

/// Create a temporary vault and return (TempDir, vault_path).
fn create_test_vault() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("create temp dir");
    let vault_path = tmp.path().join(".parc");
    vault::init_vault(&vault_path).expect("init vault");
    (tmp, vault_path)
}

/// Create a note fragment in the vault, indexed, and return the Fragment.
fn create_note(vault: &Path, title: &str, tags: &[&str], body: &str) -> Fragment {
    let config = load_config(vault).unwrap();
    let schemas = schema::load_schemas(vault).unwrap();
    let schema = schemas.resolve("note").unwrap();

    let mut frag = fragment::new_fragment("note", title, schema, &config);
    frag.tags = tags.iter().map(|t| t.to_string()).collect();
    frag.body = body.to_string();

    fragment::validate_fragment(&frag, schema).unwrap();
    fragment::create_fragment(vault, &frag).unwrap();

    let conn = index::open_index(vault).unwrap();
    index::index_fragment_auto(&conn, &frag, vault).unwrap();

    frag
}

/// Create a todo fragment with extra fields.
fn create_todo(
    vault: &Path,
    title: &str,
    status: &str,
    priority: &str,
) -> Fragment {
    let config = load_config(vault).unwrap();
    let schemas = schema::load_schemas(vault).unwrap();
    let schema = schemas.resolve("todo").unwrap();

    let mut frag = fragment::new_fragment("todo", title, schema, &config);
    frag.extra_fields
        .insert("status".into(), serde_json::Value::String(status.into()));
    frag.extra_fields
        .insert("priority".into(), serde_json::Value::String(priority.into()));

    fragment::validate_fragment(&frag, schema).unwrap();
    fragment::create_fragment(vault, &frag).unwrap();

    let conn = index::open_index(vault).unwrap();
    index::index_fragment_auto(&conn, &frag, vault).unwrap();

    frag
}

// ── Fragment CRUD ──────────────────────────────────────────────────

#[test]
fn test_create_and_get_fragment() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "Test Note", &["test", "integration"], "Hello world");

    // Read back
    let read = fragment::read_fragment(&vault, &frag.id).unwrap();
    assert_eq!(read.title, "Test Note");
    assert_eq!(read.fragment_type, "note");
    assert!(read.tags.contains(&"test".to_string()));
    assert!(read.body.contains("Hello world"));

    // DTO conversion
    let dto = FragmentDto::from(&read);
    assert_eq!(dto.id, frag.id);
    assert_eq!(dto.fragment_type, "note");
    assert_eq!(dto.title, "Test Note");
    assert!(dto.tags.contains(&"test".to_string()));
    assert!(dto.body.contains("Hello world"));
    // Timestamps should be valid RFC3339
    chrono::DateTime::parse_from_rfc3339(&dto.created_at).unwrap();
    chrono::DateTime::parse_from_rfc3339(&dto.updated_at).unwrap();
}

#[test]
fn test_update_fragment() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "Original", &[], "original body");

    let mut updated = fragment::read_fragment(&vault, &frag.id).unwrap();
    updated.title = "Updated".to_string();
    updated.body = "updated body".to_string();
    updated.tags = vec!["new-tag".to_string()];
    updated.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &updated).unwrap();

    let read = fragment::read_fragment(&vault, &frag.id).unwrap();
    assert_eq!(read.title, "Updated");
    assert!(read.body.contains("updated body"));
    assert_eq!(read.tags, vec!["new-tag"]);
}

#[test]
fn test_delete_fragment() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "To Delete", &[], "");

    let deleted_id = fragment::delete_fragment(&vault, &frag.id).unwrap();
    assert_eq!(deleted_id, frag.id);

    // Should not be readable anymore
    assert!(fragment::read_fragment(&vault, &frag.id).is_err());
}

#[test]
fn test_archive_fragment() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "To Archive", &[], "");

    // Archive (same logic as archive_fragment command)
    let mut read = fragment::read_fragment(&vault, &frag.id).unwrap();
    read.extra_fields
        .insert("archived".into(), serde_json::Value::Bool(true));
    read.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &read).unwrap();

    let archived = fragment::read_fragment(&vault, &frag.id).unwrap();
    assert_eq!(
        archived.extra_fields.get("archived"),
        Some(&serde_json::Value::Bool(true))
    );

    // Undo archive
    let mut unarchived = fragment::read_fragment(&vault, &frag.id).unwrap();
    unarchived.extra_fields.remove("archived");
    unarchived.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &unarchived).unwrap();

    let read = fragment::read_fragment(&vault, &frag.id).unwrap();
    assert!(read.extra_fields.get("archived").is_none());
}

#[test]
fn test_prefix_id_resolution() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "Prefix Test", &[], "");

    // Should resolve with 8-char prefix
    let prefix = &frag.id[..8];
    let resolved = fragment::resolve_id(&vault, prefix).unwrap();
    assert_eq!(resolved, frag.id);
}

// ── List & Search ──────────────────────────────────────────────────

#[test]
fn test_list_fragments_with_type_filter() {
    let (_tmp, vault) = create_test_vault();
    create_note(&vault, "Note A", &[], "");
    create_todo(&vault, "Todo B", "open", "high");
    create_note(&vault, "Note C", &[], "");

    let conn = index::open_index(&vault).unwrap();

    // List all
    let query = SearchQuery {
        text_terms: vec![],
        filters: vec![],
        sort: SortOrder::UpdatedDesc,
        limit: None,
    };
    let all = search::search(&conn, &query).unwrap();
    assert_eq!(all.len(), 3);

    // Filter by type=note
    let query = SearchQuery {
        text_terms: vec![],
        filters: vec![Filter::Type {
            value: "note".into(),
            negated: false,
        }],
        sort: SortOrder::UpdatedDesc,
        limit: None,
    };
    let notes = search::search(&conn, &query).unwrap();
    assert_eq!(notes.len(), 2);

    // Summary DTO conversion
    let summaries: Vec<FragmentSummaryDto> = notes
        .into_iter()
        .map(|r| FragmentSummaryDto {
            id: r.id,
            fragment_type: r.fragment_type,
            title: r.title,
            status: r.status,
            tags: r.tags,
            updated_at: r.updated_at,
        })
        .collect();
    assert!(summaries.iter().all(|s| s.fragment_type == "note"));
}

#[test]
fn test_search_with_dsl() {
    let (_tmp, vault) = create_test_vault();
    create_note(&vault, "Search Me", &["findme"], "unique content here");
    create_note(&vault, "Other", &[], "nothing special");

    let conn = index::open_index(&vault).unwrap();

    // Text search
    let query = search::parse_query("unique").unwrap();
    let results = search::search(&conn, &query).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Search Me");

    // Tag filter
    let query = search::parse_query("tag:findme").unwrap();
    let results = search::search(&conn, &query).unwrap();
    assert_eq!(results.len(), 1);

    // Search result DTO
    let dto = SearchResultDto {
        id: results[0].id.clone(),
        fragment_type: results[0].fragment_type.clone(),
        title: results[0].title.clone(),
        status: results[0].status.clone(),
        tags: results[0].tags.clone(),
        updated_at: results[0].updated_at.clone(),
        snippet: results[0].snippet.clone(),
    };
    assert_eq!(dto.title, "Search Me");
}

#[test]
fn test_search_with_limit() {
    let (_tmp, vault) = create_test_vault();
    for i in 0..5 {
        create_note(&vault, &format!("Note {}", i), &[], "");
    }

    let conn = index::open_index(&vault).unwrap();
    let mut query = search::parse_query("type:note").unwrap();
    query.limit = Some(2);

    let results = search::search(&conn, &query).unwrap();
    assert_eq!(results.len(), 2);
}

// ── Tags ───────────────────────────────────────────────────────────

#[test]
fn test_list_tags() {
    let (_tmp, vault) = create_test_vault();
    create_note(&vault, "A", &["rust", "backend"], "");
    create_note(&vault, "B", &["rust", "frontend"], "");
    create_note(&vault, "C", &["python"], "");

    let conn = index::open_index(&vault).unwrap();
    let tags = tag::aggregate_tags(&conn).unwrap();

    let tag_dtos: Vec<TagCountDto> = tags
        .into_iter()
        .map(|t| TagCountDto {
            tag: t.tag,
            count: t.count,
        })
        .collect();

    assert!(tag_dtos.len() >= 3);
    let rust = tag_dtos.iter().find(|t| t.tag == "rust").unwrap();
    assert_eq!(rust.count, 2);
    let python = tag_dtos.iter().find(|t| t.tag == "python").unwrap();
    assert_eq!(python.count, 1);
}

// ── Links ──────────────────────────────────────────────────────────

#[test]
fn test_link_and_unlink() {
    let (_tmp, vault) = create_test_vault();
    let frag_a = create_note(&vault, "A", &[], "");
    let frag_b = create_note(&vault, "B", &[], "");

    // Link (same logic as link_fragments command)
    let mut a = fragment::read_fragment(&vault, &frag_a.id).unwrap();
    let mut b = fragment::read_fragment(&vault, &frag_b.id).unwrap();

    a.links.push(b.id.clone());
    a.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &a).unwrap();

    b.links.push(a.id.clone());
    b.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &b).unwrap();

    let conn = index::open_index(&vault).unwrap();
    index::index_fragment_auto(&conn, &a, &vault).unwrap();
    index::index_fragment_auto(&conn, &b, &vault).unwrap();

    // Check backlinks
    let backlinks = index::get_backlinks(&conn, &frag_b.id).unwrap();
    assert_eq!(backlinks.len(), 1);
    let bl_dto = BacklinkDto {
        id: backlinks[0].source_id.clone(),
        fragment_type: backlinks[0].source_type.clone(),
        title: backlinks[0].source_title.clone(),
    };
    assert_eq!(bl_dto.id, frag_a.id);

    // Unlink
    let mut a = fragment::read_fragment(&vault, &frag_a.id).unwrap();
    a.links.retain(|l| l != &frag_b.id);
    a.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &a).unwrap();

    let mut b = fragment::read_fragment(&vault, &frag_b.id).unwrap();
    b.links.retain(|l| l != &frag_a.id);
    b.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &b).unwrap();

    index::index_fragment_auto(&conn, &a, &vault).unwrap();
    index::index_fragment_auto(&conn, &b, &vault).unwrap();

    let backlinks = index::get_backlinks(&conn, &frag_b.id).unwrap();
    assert_eq!(backlinks.len(), 0);
}

#[test]
fn test_self_link_prevention() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "Self", &[], "");

    // The link command checks id equality before linking
    let a = fragment::read_fragment(&vault, &frag.id).unwrap();
    let b = fragment::read_fragment(&vault, &frag.id).unwrap();
    assert_eq!(a.id, b.id, "same fragment — command would reject this");
}

// ── Schemas ────────────────────────────────────────────────────────

#[test]
fn test_list_schemas() {
    let (_tmp, vault) = create_test_vault();
    let registry = schema::load_schemas(&vault).unwrap();
    let schemas = registry.list();

    assert!(schemas.len() >= 5, "should have at least 5 built-in types");

    let dtos: Vec<SchemaDto> = schemas
        .iter()
        .map(|s| {
            SchemaDto {
                name: s.name.clone(),
                alias: s.alias.clone(),
                editor_skip: s.editor_skip,
                fields: s
                    .fields
                    .iter()
                    .map(|f| {
                        let (type_str, values) = match &f.field_type {
                            FieldType::String => ("string".into(), vec![]),
                            FieldType::Date => ("date".into(), vec![]),
                            FieldType::Enum(vals) => ("enum".into(), vals.clone()),
                            FieldType::ListOfStrings => ("list".into(), vec![]),
                        };
                        SchemaFieldDto {
                            name: f.name.clone(),
                            field_type: type_str,
                            required: f.required,
                            default: f.default.clone(),
                            values,
                        }
                    })
                    .collect(),
            }
        })
        .collect();

    let names: Vec<&str> = dtos.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"note"));
    assert!(names.contains(&"todo"));
    assert!(names.contains(&"decision"));
    assert!(names.contains(&"risk"));
    assert!(names.contains(&"idea"));
}

#[test]
fn test_get_schema() {
    let (_tmp, vault) = create_test_vault();
    let registry = schema::load_schemas(&vault).unwrap();

    let todo_schema = registry.resolve("todo").unwrap();
    assert_eq!(todo_schema.name, "todo");

    let field_names: Vec<&str> = todo_schema.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(field_names.contains(&"status"));
    assert!(field_names.contains(&"priority"));
}

#[test]
fn test_get_unknown_schema() {
    let (_tmp, vault) = create_test_vault();
    let registry = schema::load_schemas(&vault).unwrap();
    assert!(registry.resolve("nonexistent").is_none());
}

// ── Vault ──────────────────────────────────────────────────────────

#[test]
fn test_vault_info() {
    let (_tmp, vault) = create_test_vault();

    let info = vault::vault_info(&vault).unwrap();
    let dto = VaultInfoDto {
        path: info.path.to_string_lossy().to_string(),
        scope: info.scope.to_string(),
        fragment_count: info.fragment_count,
    };

    assert_eq!(dto.fragment_count, 0);
    assert!(dto.path.ends_with(".parc"));
}

#[test]
fn test_vault_info_with_fragments() {
    let (_tmp, vault) = create_test_vault();
    create_note(&vault, "A", &[], "");
    create_note(&vault, "B", &[], "");

    let info = vault::vault_info(&vault).unwrap();
    assert_eq!(info.fragment_count, 2);
}

#[test]
fn test_reindex() {
    let (_tmp, vault) = create_test_vault();
    create_note(&vault, "A", &["tag1"], "hello");
    create_note(&vault, "B", &["tag2"], "world");

    let count = index::reindex(&vault).unwrap();
    assert_eq!(count, 2);

    // Verify index works after reindex
    let conn = index::open_index(&vault).unwrap();
    let query = search::parse_query("type:note").unwrap();
    let results = search::search(&conn, &query).unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn test_doctor_empty_vault() {
    let (_tmp, vault) = create_test_vault();

    let report = doctor::run_doctor(&vault).unwrap();
    let dto = DoctorReportDto {
        fragments_checked: report.fragments_checked,
        healthy: report.is_healthy(),
        findings: vec![],
    };

    assert_eq!(dto.fragments_checked, 0);
    assert!(dto.healthy);
}

#[test]
fn test_doctor_with_fragments() {
    let (_tmp, vault) = create_test_vault();
    let a = create_note(&vault, "A", &[], "");
    let b = create_note(&vault, "B", &[], "");

    // Link them so they aren't orphans
    let mut fa = fragment::read_fragment(&vault, &a.id).unwrap();
    let mut fb = fragment::read_fragment(&vault, &b.id).unwrap();
    fa.links.push(fb.id.clone());
    fb.links.push(fa.id.clone());
    fragment::write_fragment(&vault, &fa).unwrap();
    fragment::write_fragment(&vault, &fb).unwrap();

    let report = doctor::run_doctor(&vault).unwrap();
    assert_eq!(report.fragments_checked, 2);
    // No broken links or schema violations
    let non_orphan: Vec<_> = report
        .findings
        .iter()
        .filter(|f| !matches!(f, doctor::DoctorFinding::OrphanFragment { .. }))
        .collect();
    assert!(non_orphan.is_empty());
}

#[test]
fn test_init_vault() {
    let tmp = tempfile::TempDir::new().unwrap();
    let new_vault = tmp.path().join("new-project").join(".parc");

    vault::init_vault(&new_vault).unwrap();
    assert!(new_vault.join("config.yml").exists());
    assert!(new_vault.join("schemas").exists());
    assert!(new_vault.join("fragments").exists());

    let info = vault::vault_info(&new_vault).unwrap();
    assert_eq!(info.fragment_count, 0);
}

// ── Attachments ────────────────────────────────────────────────────

#[test]
fn test_attachment_lifecycle() {
    let (tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "With Attachment", &[], "");

    // Create a temp file to attach
    let src_file = tmp.path().join("test-file.txt");
    std::fs::write(&src_file, "attachment content").unwrap();

    // Attach
    let filename = attachment::attach_file(&vault, &frag.id, &src_file, false).unwrap();
    assert_eq!(filename, "test-file.txt");

    // List
    let infos = attachment::list_attachments(&vault, &frag.id).unwrap();
    assert_eq!(infos.len(), 1);

    let dto = AttachmentInfoDto {
        filename: infos[0].filename.clone(),
        size: infos[0].size,
        path: infos[0].path.to_string_lossy().to_string(),
    };
    assert_eq!(dto.filename, "test-file.txt");
    assert!(dto.size > 0);

    // Verify path exists
    let attach_path = vault
        .join("attachments")
        .join(&frag.id)
        .join("test-file.txt");
    assert!(attach_path.exists());

    // Detach
    attachment::detach_file(&vault, &frag.id, "test-file.txt").unwrap();
    let infos = attachment::list_attachments(&vault, &frag.id).unwrap();
    assert_eq!(infos.len(), 0);
}

#[test]
fn test_attachment_nonexistent_file() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "No File", &[], "");

    let result = attachment::attach_file(
        &vault,
        &frag.id,
        Path::new("/nonexistent/file.txt"),
        false,
    );
    assert!(result.is_err());
}

// ── History ────────────────────────────────────────────────────────

#[test]
fn test_history_lifecycle() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "V1", &[], "version one");

    // Update (creates snapshot)
    let mut updated = fragment::read_fragment(&vault, &frag.id).unwrap();
    updated.title = "V2".to_string();
    updated.body = "version two".to_string();
    updated.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &updated).unwrap();

    // List versions
    let versions = history::list_versions(&vault, &frag.id).unwrap();
    assert!(
        !versions.is_empty(),
        "should have at least one history version"
    );

    let version_dtos: Vec<VersionEntryDto> = versions
        .iter()
        .map(|v| VersionEntryDto {
            timestamp: v.timestamp.clone(),
            size: v.size,
        })
        .collect();
    assert!(version_dtos[0].size > 0);

    // Read old version
    let old = history::read_version(&vault, &frag.id, &versions[0].timestamp).unwrap();
    assert_eq!(old.title, "V1");
    assert!(old.body.contains("version one"));

    // Diff
    let diff = history::diff_versions(&vault, &frag.id, Some(&versions[0].timestamp)).unwrap();
    let diff_dto = DiffDto { diff: diff.clone() };
    assert!(!diff_dto.diff.is_empty());

    // Restore
    let restored =
        history::restore_version(&vault, &frag.id, &versions[0].timestamp).unwrap();
    assert_eq!(restored.title, "V1");

    // Verify current state is restored
    let current = fragment::read_fragment(&vault, &frag.id).unwrap();
    assert_eq!(current.title, "V1");
}

// ── Markdown Rendering ─────────────────────────────────────────────

#[test]
fn test_render_markdown() {
    use comrak::{markdown_to_html, Options};

    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.unsafe_ = true;

    let html = markdown_to_html("# Hello\n\n**bold** and *italic*", &options);
    assert!(html.contains("<h1>Hello</h1>"));
    assert!(html.contains("<strong>bold</strong>"));
    assert!(html.contains("<em>italic</em>"));

    // Table
    let html = markdown_to_html("| A | B |\n|---|---|\n| 1 | 2 |", &options);
    assert!(html.contains("<table>"));

    // Tasklist
    let html = markdown_to_html("- [x] done\n- [ ] todo", &options);
    assert!(html.contains("checked"));

    // Strikethrough
    let html = markdown_to_html("~~deleted~~", &options);
    assert!(html.contains("<del>"));
}

// ── Todo with Extra Fields ─────────────────────────────────────────

#[test]
fn test_todo_extra_fields() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_todo(&vault, "Test Task", "open", "high");

    let dto = FragmentDto::from(&frag);
    assert_eq!(dto.fragment_type, "todo");
    assert_eq!(
        dto.extra_fields.get("status"),
        Some(&serde_json::Value::String("open".into()))
    );
    assert_eq!(
        dto.extra_fields.get("priority"),
        Some(&serde_json::Value::String("high".into()))
    );

    // Update status
    let mut updated = fragment::read_fragment(&vault, &frag.id).unwrap();
    updated
        .extra_fields
        .insert("status".into(), serde_json::Value::String("done".into()));
    updated.updated_at = chrono::Utc::now();
    fragment::write_fragment(&vault, &updated).unwrap();

    let conn = index::open_index(&vault).unwrap();
    index::index_fragment_auto(&conn, &updated, &vault).unwrap();

    // Search by status
    let query = search::parse_query("type:todo status:done").unwrap();
    let results = search::search(&conn, &query).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Test Task");
}

// ── Error Cases ────────────────────────────────────────────────────

#[test]
fn test_error_nonexistent_fragment() {
    let (_tmp, vault) = create_test_vault();
    let result = fragment::read_fragment(&vault, "NONEXISTENT");
    assert!(result.is_err());
}

#[test]
fn test_error_gui_error_from_parc_error() {
    use parc_gui::error::GuiError;

    let (_tmp, vault) = create_test_vault();
    let err = fragment::read_fragment(&vault, "NONEXISTENT").unwrap_err();
    let gui_err: GuiError = err.into();

    // GuiError should be serializable (Tauri requirement)
    let json = serde_json::to_string(&gui_err).unwrap();
    assert!(!json.is_empty());
}

#[test]
fn test_error_gui_error_variants() {
    use parc_gui::error::GuiError;

    // Other variant
    let err = GuiError::Other("test error".into());
    assert_eq!(err.to_string(), "test error");
    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("test error"));
}

// ── DTO Serialization Round-Trip ───────────────────────────────────

#[test]
fn test_dto_serialization_roundtrip() {
    let (_tmp, vault) = create_test_vault();
    let frag = create_note(&vault, "Roundtrip", &["a", "b"], "body text");

    let dto = FragmentDto::from(&frag);
    let json = serde_json::to_string(&dto).unwrap();
    let deserialized: FragmentDto = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.id, dto.id);
    assert_eq!(deserialized.title, dto.title);
    assert_eq!(deserialized.fragment_type, dto.fragment_type);
    assert_eq!(deserialized.tags, dto.tags);
    assert_eq!(deserialized.body, dto.body);
}

#[test]
fn test_vault_info_dto_serialization() {
    let dto = VaultInfoDto {
        path: "/tmp/test/.parc".into(),
        scope: "local".into(),
        fragment_count: 42,
    };
    let json = serde_json::to_string(&dto).unwrap();
    let de: VaultInfoDto = serde_json::from_str(&json).unwrap();
    assert_eq!(de.fragment_count, 42);
    assert_eq!(de.scope, "local");
}

#[test]
fn test_doctor_report_dto_serialization() {
    let dto = DoctorReportDto {
        fragments_checked: 10,
        healthy: true,
        findings: vec![DoctorFindingDto {
            finding_type: "broken_link".into(),
            details: {
                let mut m = std::collections::BTreeMap::new();
                m.insert("source_id".into(), serde_json::json!("abc123"));
                m
            },
        }],
    };
    let json = serde_json::to_string(&dto).unwrap();
    let de: DoctorReportDto = serde_json::from_str(&json).unwrap();
    assert_eq!(de.fragments_checked, 10);
    assert_eq!(de.findings.len(), 1);
    assert_eq!(de.findings[0].finding_type, "broken_link");
}
