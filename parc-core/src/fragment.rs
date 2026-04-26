use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::config::Config;
use crate::error::ParcError;
use crate::schema::{FieldType, Schema};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Fragment {
    pub id: String,
    pub fragment_type: String,
    pub title: String,
    pub tags: Vec<String>,
    pub links: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<String>,
    pub extra_fields: BTreeMap<String, Value>,
    pub body: String,
}

/// Generate a new ULID string.
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

/// Create a new fragment with defaults from a schema.
pub fn new_fragment(
    fragment_type: &str,
    title: &str,
    schema: &Schema,
    config: &Config,
) -> Fragment {
    let now = Utc::now();
    let mut extra_fields = BTreeMap::new();

    for field in &schema.fields {
        if let Some(ref default) = field.default {
            extra_fields.insert(field.name.clone(), Value::String(default.clone()));
        }
    }

    let mut tags = config.default_tags.clone();
    tags.dedup();

    Fragment {
        id: new_id(),
        fragment_type: fragment_type.to_string(),
        title: title.to_string(),
        tags,
        links: Vec::new(),
        attachments: Vec::new(),
        created_at: now,
        updated_at: now,
        created_by: config.user.clone(),
        extra_fields,
        body: String::new(),
    }
}

/// Parse a fragment from Markdown with YAML frontmatter.
pub fn parse_fragment(content: &str) -> Result<Fragment, ParcError> {
    let (frontmatter, body) = split_frontmatter(content)?;

    // Parse YAML into a generic map, then convert to JSON values
    let yaml_value: serde_yaml::Value = serde_yaml::from_str(&frontmatter)?;
    let json_value: Value = serde_json::to_value(&yaml_value)?;
    let map = json_value
        .as_object()
        .ok_or_else(|| ParcError::ValidationError("frontmatter must be a YAML mapping".into()))?;

    let id = map
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ParcError::ValidationError("missing 'id' in frontmatter".into()))?
        .to_string();

    let fragment_type = map
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ParcError::ValidationError("missing 'type' in frontmatter".into()))?
        .to_string();

    let title = map
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let tags = map
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let links = map
        .get("links")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let attachments = map
        .get("attachments")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let created_at = map
        .get("created_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let updated_at = map
        .get("updated_at")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let created_by = map.get("created_by").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Remaining fields go into extra_fields
    let envelope_keys = [
        "id",
        "type",
        "title",
        "tags",
        "links",
        "attachments",
        "created_at",
        "updated_at",
        "created_by",
    ];
    let mut extra_fields = BTreeMap::new();
    for (key, value) in map {
        if !envelope_keys.contains(&key.as_str()) {
            extra_fields.insert(key.clone(), value.clone());
        }
    }

    Ok(Fragment {
        id,
        fragment_type,
        title,
        tags,
        links,
        attachments,
        created_at,
        updated_at,
        created_by,
        extra_fields,
        body,
    })
}

/// Serialize a fragment to Markdown with YAML frontmatter.
pub fn serialize_fragment(fragment: &Fragment) -> String {
    let mut lines = Vec::new();
    lines.push("---".to_string());
    lines.push(format!("id: {}", fragment.id));
    lines.push(format!("type: {}", fragment.fragment_type));
    lines.push(format!("title: {}", yaml_escape_string(&fragment.title)));
    // Tags: one per line
    if fragment.tags.is_empty() {
        lines.push("tags: []".to_string());
    } else {
        lines.push("tags:".to_string());
        for tag in &fragment.tags {
            lines.push(format!("  - {}", tag));
        }
    }
    // Links
    if fragment.links.is_empty() {
        lines.push("links: []".to_string());
    } else {
        lines.push("links:".to_string());
        for link in &fragment.links {
            lines.push(format!("  - {}", link));
        }
    }
    // Attachments
    if !fragment.attachments.is_empty() {
        lines.push("attachments:".to_string());
        for attachment in &fragment.attachments {
            lines.push(format!("  - {}", attachment));
        }
    }
    // Extra fields (type-specific)
    for (key, value) in &fragment.extra_fields {
        lines.push(format_yaml_field(key, value));
    }
    // Timestamps
    lines.push(format!(
        "created_at: {}",
        fragment.created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    ));
    lines.push(format!(
        "updated_at: {}",
        fragment.updated_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    ));
    if let Some(ref by) = fragment.created_by {
        lines.push(format!("created_by: {}", by));
    }
    lines.push("---".to_string());

    if fragment.body.is_empty() {
        lines.push(String::new());
    } else {
        // Ensure body is separated from frontmatter
        let body = fragment.body.trim_start_matches('\n');
        lines.push(String::new());
        lines.push(body.to_string());
    }

    lines.join("\n")
}

fn yaml_escape_string(s: &str) -> String {
    if s.is_empty()
        || s.contains(':')
        || s.contains('#')
        || s.contains('\'')
        || s.contains('"')
        || s.contains('\n')
        || s.starts_with(' ')
        || s.starts_with('{')
        || s.starts_with('[')
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

fn format_yaml_field(key: &str, value: &Value) -> String {
    match value {
        Value::String(s) => format!("{}: {}", key, s),
        Value::Number(n) => format!("{}: {}", key, n),
        Value::Bool(b) => format!("{}: {}", key, b),
        Value::Null => format!("{}: null", key),
        Value::Array(arr) => {
            if arr.is_empty() {
                format!("{}: []", key)
            } else {
                let mut lines = vec![format!("{}:", key)];
                for item in arr {
                    match item {
                        Value::String(s) => lines.push(format!("  - {}", s)),
                        other => lines.push(format!("  - {}", other)),
                    }
                }
                lines.join("\n")
            }
        }
        Value::Object(_) => format!("{}: {}", key, value),
    }
}

fn split_frontmatter(content: &str) -> Result<(String, String), ParcError> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return Err(ParcError::ValidationError(
            "fragment must start with YAML frontmatter (---)".into(),
        ));
    }

    let after_first = &content[3..];
    let after_first = after_first.trim_start_matches(['\r', '\n']);

    if let Some(end_pos) = after_first.find("\n---") {
        let frontmatter = after_first[..end_pos].to_string();
        let body_start = end_pos + 4; // skip \n---
        let body = if body_start < after_first.len() {
            let rest = &after_first[body_start..];
            // Skip the newline after closing ---
            rest.strip_prefix('\n').unwrap_or(rest).to_string()
        } else {
            String::new()
        };
        Ok((frontmatter, body))
    } else {
        Err(ParcError::ValidationError(
            "no closing --- for frontmatter".into(),
        ))
    }
}

// --- CRUD operations ---

/// Write a new fragment to disk. Returns the fragment ID.
pub fn create_fragment(vault: &Path, fragment: &Fragment) -> Result<String, ParcError> {
    let content = serialize_fragment(fragment);
    let path = vault
        .join("fragments")
        .join(format!("{}.md", fragment.id));
    std::fs::write(&path, content)?;
    Ok(fragment.id.clone())
}

/// Read a fragment by full ID or unique prefix.
pub fn read_fragment(vault: &Path, id_or_prefix: &str) -> Result<Fragment, ParcError> {
    let full_id = resolve_id(vault, id_or_prefix)?;
    let path = vault.join("fragments").join(format!("{}.md", full_id));
    let content = std::fs::read_to_string(&path)?;
    parse_fragment(&content)
}

/// Overwrite a fragment file. Saves a history snapshot before overwriting
/// if history is enabled in config.
pub fn write_fragment(vault: &Path, fragment: &Fragment) -> Result<(), ParcError> {
    // Save history snapshot of the current version before overwriting
    let config = crate::config::load_config(vault)?;
    if config.history_enabled {
        crate::history::save_snapshot(vault, &fragment.id)?;
    }

    let content = serialize_fragment(fragment);
    let path = vault
        .join("fragments")
        .join(format!("{}.md", fragment.id));
    std::fs::write(&path, content)?;
    Ok(())
}

/// Rewrite a fragment as another schema type while preserving its content.
pub fn promote_fragment(
    vault: &Path,
    id_or_prefix: &str,
    new_type: &str,
    overrides: BTreeMap<String, Value>,
) -> Result<Fragment, ParcError> {
    let schemas = crate::schema::load_schemas(vault)?;
    let schema = schemas
        .resolve(new_type)
        .ok_or_else(|| ParcError::SchemaNotFound(new_type.to_string()))?;

    let mut fragment = read_fragment(vault, id_or_prefix)?;
    fragment.fragment_type = schema.name.clone();

    for (key, value) in overrides {
        fragment.extra_fields.insert(key, value);
    }

    for field in &schema.fields {
        if !fragment.extra_fields.contains_key(&field.name) {
            if let Some(default) = &field.default {
                fragment
                    .extra_fields
                    .insert(field.name.clone(), Value::String(default.clone()));
            }
        }
    }

    validate_fragment(&fragment, schema)?;
    fragment.updated_at = Utc::now();
    write_fragment(vault, &fragment)?;

    Ok(fragment)
}

/// Soft-delete: move fragment file to trash/.
pub fn delete_fragment(vault: &Path, id_or_prefix: &str) -> Result<String, ParcError> {
    let full_id = resolve_id(vault, id_or_prefix)?;
    let src = vault.join("fragments").join(format!("{}.md", full_id));
    let dst = vault.join("trash").join(format!("{}.md", full_id));
    std::fs::rename(&src, &dst)?;
    Ok(full_id)
}

/// List all fragment IDs in the vault.
pub fn list_fragment_ids(vault: &Path) -> Result<Vec<String>, ParcError> {
    let fragments_dir = vault.join("fragments");
    if !fragments_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut ids = Vec::new();
    for entry in std::fs::read_dir(&fragments_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                ids.push(stem.to_string());
            }
        }
    }
    ids.sort();
    Ok(ids)
}

/// Resolve an ID prefix to a full ID.
pub fn resolve_id(vault: &Path, prefix: &str) -> Result<String, ParcError> {
    let upper_prefix = prefix.to_uppercase();
    let ids = list_fragment_ids(vault)?;
    let matches: Vec<&String> = ids.iter().filter(|id| id.starts_with(&upper_prefix)).collect();

    match matches.len() {
        0 => Err(ParcError::FragmentNotFound(prefix.to_string())),
        1 => Ok(matches[0].clone()),
        n => Err(ParcError::AmbiguousId(prefix.to_string(), n)),
    }
}

/// Validate a fragment against its schema.
pub fn validate_fragment(fragment: &Fragment, schema: &Schema) -> Result<(), ParcError> {
    for field in &schema.fields {
        let value = fragment.extra_fields.get(&field.name);

        if field.required && value.is_none() {
            // Check if there's a default — if so, it's fine
            if field.default.is_none() {
                return Err(ParcError::ValidationError(format!(
                    "required field '{}' is missing",
                    field.name
                )));
            }
        }

        if let Some(val) = value {
            match &field.field_type {
                FieldType::Enum(values) => {
                    if let Some(s) = val.as_str() {
                        if !values.contains(&s.to_string()) {
                            return Err(ParcError::ValidationError(format!(
                                "invalid value '{}' for field '{}': allowed values are {:?}",
                                s, field.name, values
                            )));
                        }
                    }
                }
                FieldType::Date => {
                    if let Some(s) = val.as_str() {
                        // Accept YYYY-MM-DD format
                        if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_err() {
                            return Err(ParcError::ValidationError(format!(
                                "invalid date '{}' for field '{}': expected YYYY-MM-DD",
                                s, field.name
                            )));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_fragment() -> Fragment {
        let mut extra = BTreeMap::new();
        extra.insert("status".to_string(), Value::String("open".to_string()));
        extra.insert(
            "priority".to_string(),
            Value::String("high".to_string()),
        );

        Fragment {
            id: "01JQ7V3XKP5GQZ2N8R6T1WBMVH".to_string(),
            fragment_type: "todo".to_string(),
            title: "Test task".to_string(),
            tags: vec!["backend".to_string(), "search".to_string()],
            links: vec!["01JQ7V4Y".to_string()],
            attachments: Vec::new(),
            created_at: DateTime::parse_from_rfc3339("2026-02-21T10:30:00Z")
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339("2026-02-21T10:30:00Z")
                .unwrap()
                .with_timezone(&Utc),
            created_by: Some("alice".to_string()),
            extra_fields: extra,
            body: "Body content with #inline-tag.\n".to_string(),
        }
    }

    #[test]
    fn test_round_trip() {
        let fragment = make_test_fragment();
        let serialized = serialize_fragment(&fragment);
        let parsed = parse_fragment(&serialized).unwrap();

        assert_eq!(parsed.id, fragment.id);
        assert_eq!(parsed.fragment_type, fragment.fragment_type);
        assert_eq!(parsed.title, fragment.title);
        assert_eq!(parsed.tags, fragment.tags);
        assert_eq!(parsed.links, fragment.links);
        assert_eq!(parsed.created_by, fragment.created_by);
        assert_eq!(parsed.extra_fields, fragment.extra_fields);
        assert_eq!(parsed.body.trim(), fragment.body.trim());
    }

    #[test]
    fn test_crud() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let fragment = make_test_fragment();
        create_fragment(&vault, &fragment).unwrap();

        // Read by full ID
        let read = read_fragment(&vault, &fragment.id).unwrap();
        assert_eq!(read.title, "Test task");

        // Read by prefix
        let read_prefix = read_fragment(&vault, "01JQ7V3X").unwrap();
        assert_eq!(read_prefix.id, fragment.id);

        // Delete
        delete_fragment(&vault, "01JQ7V3X").unwrap();
        assert!(read_fragment(&vault, &fragment.id).is_err());
        assert!(vault.join("trash").join(format!("{}.md", fragment.id)).exists());
    }

    #[test]
    fn test_validate_enum() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let schema = crate::schema::load_schemas(&vault).unwrap();
        let todo_schema = schema.resolve("todo").unwrap();

        let mut fragment = make_test_fragment();
        assert!(validate_fragment(&fragment, todo_schema).is_ok());

        fragment.extra_fields.insert(
            "status".to_string(),
            Value::String("invalid".to_string()),
        );
        assert!(validate_fragment(&fragment, todo_schema).is_err());
    }

    #[test]
    fn test_promote_fragment_preserves_content_and_applies_defaults() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let config = crate::config::load_config(&vault).unwrap();
        let schemas = crate::schema::load_schemas(&vault).unwrap();
        let note_schema = schemas.resolve("note").unwrap();

        let mut fragment = new_fragment("note", "Capture", note_schema, &config);
        fragment.tags = vec!["backend".to_string()];
        fragment.links = vec!["01JQ7V3XKP5GQZ2N8R6T1WBMVH".to_string()];
        fragment.body = "Look into connection pooling".to_string();
        create_fragment(&vault, &fragment).unwrap();

        let mut overrides = BTreeMap::new();
        overrides.insert("priority".to_string(), Value::String("high".to_string()));

        let promoted = promote_fragment(&vault, &fragment.id[..8], "todo", overrides).unwrap();

        assert_eq!(promoted.fragment_type, "todo");
        assert_eq!(promoted.title, "Capture");
        assert_eq!(promoted.tags, vec!["backend"]);
        assert_eq!(promoted.links, vec!["01JQ7V3XKP5GQZ2N8R6T1WBMVH"]);
        assert_eq!(promoted.body.trim(), "Look into connection pooling");
        assert_eq!(
            promoted.extra_fields.get("status"),
            Some(&Value::String("open".to_string()))
        );
        assert_eq!(
            promoted.extra_fields.get("priority"),
            Some(&Value::String("high".to_string()))
        );

        let read_back = read_fragment(&vault, &fragment.id).unwrap();
        assert_eq!(read_back.fragment_type, "todo");
    }
}
