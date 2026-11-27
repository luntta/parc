use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

use parc_core::config::load_config;
use parc_core::fragment::{self, Fragment};
use parc_core::index;
use parc_core::schema::{load_schemas, FieldType, Schema};
use parc_core::search::{self, Filter, SearchQuery, SortOrder};

use crate::jsonrpc::RpcError;
use crate::methods::write;
use crate::router::{extract_params, map_parc_error};

/// Set schema fields by name, e.g. `{"status": "done", "due": null}`. Only
/// fields the type's schema declares are accepted; `null` removes an optional
/// field. Enum values are checked when the writer validates the fragment.
fn apply_fields(
    frag: &mut Fragment,
    schema: Option<&Schema>,
    fields: Map<String, Value>,
) -> Result<(), RpcError> {
    for (name, value) in fields {
        let def = schema
            .and_then(|s| s.fields.iter().find(|f| f.name == name))
            .ok_or_else(|| {
                RpcError::invalid_params(&format!(
                    "unknown field '{}' for type '{}'",
                    name, frag.fragment_type
                ))
            })?;
        let value = match (value, &def.field_type) {
            (Value::Null, _) if def.required => {
                return Err(RpcError::invalid_params(&format!(
                    "field '{}' is required",
                    name
                )))
            }
            (Value::Null, _) => {
                frag.extra_fields.remove(&name);
                continue;
            }
            (Value::String(s), FieldType::Date) => Value::String(
                parc_core::date::resolve_due_date(&s)
                    .map_err(|e| RpcError::invalid_params(&e.to_string()))?,
            ),
            (Value::Array(items), FieldType::ListOfStrings)
                if items.iter().all(Value::is_string) =>
            {
                Value::Array(items)
            }
            (Value::String(s), FieldType::String | FieldType::Enum(_)) => Value::String(s),
            _ => {
                return Err(RpcError::invalid_params(&format!(
                    "field '{}' expects {}",
                    name,
                    match def.field_type {
                        FieldType::ListOfStrings => "a list of strings",
                        _ => "a string",
                    }
                )))
            }
        };
        frag.extra_fields.insert(name, value);
    }
    Ok(())
}

/// The fragment as saved, with the `revision` of its file for `expected_revision`.
pub(crate) fn saved_json(vault: &Path, f: &Fragment) -> Result<Value, RpcError> {
    let mut value = fragment_to_json(f);
    value["revision"] = Value::String(fragment::revision(vault, &f.id).map_err(map_parc_error)?);
    Ok(value)
}

pub(crate) fn fragment_to_json(f: &Fragment) -> Value {
    let mut obj = serde_json::json!({
        "id": f.id,
        "type": f.fragment_type,
        "title": f.title,
        "tags": f.tags,
        "links": f.links,
        "attachments": f.attachments,
        "created_at": f.created_at.to_rfc3339(),
        "updated_at": f.updated_at.to_rfc3339(),
        "body": f.body,
    });

    if let Some(ref by) = f.created_by {
        obj["created_by"] = Value::String(by.clone());
    }

    // Merge extra_fields into the top level
    if let Value::Object(ref mut map) = obj {
        for (k, v) in &f.extra_fields {
            map.insert(k.clone(), v.clone());
        }
    }

    obj
}

#[derive(Deserialize)]
pub struct CreateParams {
    #[serde(rename = "type")]
    pub fragment_type: String,
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub links: Vec<String>,
    pub due: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    /// Any schema field by name; see [`apply_fields`].
    #[serde(default)]
    pub fields: Map<String, Value>,
}

pub fn create(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: CreateParams = extract_params(params)?;

    let config = load_config(vault).map_err(map_parc_error)?;
    let schemas = load_schemas(vault).map_err(map_parc_error)?;

    let schema = schemas
        .resolve(&p.fragment_type)
        .ok_or_else(|| RpcError::invalid_params(&format!("unknown type: {}", p.fragment_type)))?;

    let resolved_type = schema.name.clone();
    let title = p.title.as_deref().unwrap_or("");
    let mut frag = fragment::new_fragment(&resolved_type, title, schema, &config);

    for tag in &p.tags {
        if !frag.tags.contains(tag) {
            frag.tags.push(tag.clone());
        }
    }
    frag.links = p.links;

    if let Some(body) = p.body {
        frag.body = body;
    }
    if let Some(status) = p.status {
        frag.extra_fields
            .insert("status".to_string(), Value::String(status));
    }
    if let Some(due) = p.due {
        let resolved = parc_core::date::resolve_due_date(&due)
            .map_err(|e| RpcError::invalid_params(&e.to_string()))?;
        frag.extra_fields
            .insert("due".to_string(), Value::String(resolved));
    }
    if let Some(priority) = p.priority {
        frag.extra_fields
            .insert("priority".to_string(), Value::String(priority));
    }
    if let Some(assignee) = p.assignee {
        frag.extra_fields
            .insert("assignee".to_string(), Value::String(assignee));
    }
    apply_fields(&mut frag, Some(schema), p.fields)?;

    let mut writer = write::writer(vault);
    let frag = writer.create(frag).map_err(map_parc_error)?;
    Ok(write::with_warnings(saved_json(vault, &frag)?, &mut writer))
}

#[derive(Deserialize)]
pub struct GetParams {
    pub id: String,
}

pub fn get(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    let frag = fragment::read_fragment(vault, &p.id).map_err(map_parc_error)?;
    saved_json(vault, &frag)
}

#[derive(Deserialize)]
pub struct UpdateParams {
    pub id: String,
    pub title: Option<String>,
    pub tags: Option<Vec<String>>,
    pub body: Option<String>,
    pub links: Option<Vec<String>>,
    pub due: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    /// Any schema field by name; see [`apply_fields`].
    #[serde(default)]
    pub fields: Map<String, Value>,
    /// The `updated_at` the client's copy was read at. When the fragment has
    /// been saved since, the update is refused with a conflict error.
    pub expected_updated_at: Option<String>,
    /// The `revision` the client's copy was read at; like `expected_updated_at`
    /// but exact, so it also catches a save within the same second.
    pub expected_revision: Option<String>,
}

pub fn update(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: UpdateParams = extract_params(params)?;
    let mut frag = fragment::read_fragment(vault, &p.id).map_err(map_parc_error)?;

    if let Some(expected) = &p.expected_updated_at {
        let expected = chrono::DateTime::parse_from_rfc3339(expected)
            .map_err(|e| RpcError::invalid_params(&format!("expected_updated_at: {}", e)))?;
        // Files store whole seconds, so that is the precision to compare at.
        if expected.timestamp() != frag.updated_at.timestamp() {
            return Err(RpcError::conflict(&frag.updated_at.to_rfc3339()));
        }
    }
    if let Some(expected) = &p.expected_revision {
        if *expected != fragment::revision(vault, &frag.id).map_err(map_parc_error)? {
            return Err(RpcError::conflict(&frag.updated_at.to_rfc3339()));
        }
    }

    if let Some(title) = p.title {
        frag.title = title;
    }
    if let Some(tags) = p.tags {
        frag.tags = tags;
    }
    if let Some(body) = p.body {
        frag.body = body;
    }
    if let Some(links) = p.links {
        frag.links = links;
    }
    if let Some(status) = p.status {
        frag.extra_fields
            .insert("status".to_string(), Value::String(status));
    }
    if let Some(due) = p.due {
        let resolved = parc_core::date::resolve_due_date(&due)
            .map_err(|e| RpcError::invalid_params(&e.to_string()))?;
        frag.extra_fields
            .insert("due".to_string(), Value::String(resolved));
    }
    if let Some(priority) = p.priority {
        frag.extra_fields
            .insert("priority".to_string(), Value::String(priority));
    }
    if let Some(assignee) = p.assignee {
        frag.extra_fields
            .insert("assignee".to_string(), Value::String(assignee));
    }
    if !p.fields.is_empty() {
        let schemas = load_schemas(vault).map_err(map_parc_error)?;
        let schema = schemas.resolve(&frag.fragment_type).cloned();
        apply_fields(&mut frag, schema.as_ref(), p.fields)?;
    }

    let mut writer = write::writer(vault);
    let frag = writer.update(frag).map_err(map_parc_error)?;
    Ok(write::with_warnings(saved_json(vault, &frag)?, &mut writer))
}

#[derive(Deserialize)]
pub struct PromoteParams {
    pub id: String,
    #[serde(rename = "type")]
    pub new_type: String,
    #[serde(default)]
    pub fields: Map<String, Value>,
    /// Refuse if the file changed since this revision, as in `fragment.update`.
    pub expected_revision: Option<String>,
}

/// Convert a saved fragment to another enabled schema type, preserving its body,
/// title, tags and links while applying defaults for the target type.
pub fn promote(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: PromoteParams = extract_params(params)?;
    let mut frag = fragment::read_fragment(vault, &p.id).map_err(map_parc_error)?;

    if let Some(expected) = &p.expected_revision {
        if *expected != fragment::revision(vault, &frag.id).map_err(map_parc_error)? {
            return Err(RpcError::conflict(&frag.updated_at.to_rfc3339()));
        }
    }

    let schemas = load_schemas(vault).map_err(map_parc_error)?;
    let schema = schemas
        .resolve(&p.new_type)
        .ok_or_else(|| RpcError::invalid_params(&format!("unknown type: {}", p.new_type)))?;

    fragment::apply_promotion(&mut frag, schema, BTreeMap::new()).map_err(map_parc_error)?;
    if !p.fields.is_empty() {
        apply_fields(&mut frag, Some(schema), p.fields)?;
    }

    let mut writer = write::writer(vault);
    let frag = writer.update(frag).map_err(map_parc_error)?;
    Ok(write::with_warnings(saved_json(vault, &frag)?, &mut writer))
}

#[derive(Deserialize)]
pub struct ArchiveParams {
    pub id: String,
    pub archived: bool,
    /// As in `fragment.update`: refuse if the file changed since this revision.
    pub expected_revision: Option<String>,
}

/// Archive or unarchive: archived fragments leave default searches but stay in the vault.
pub fn archive(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: ArchiveParams = extract_params(params)?;
    if let Some(expected) = &p.expected_revision {
        if *expected != fragment::revision(vault, &p.id).map_err(map_parc_error)? {
            let current = fragment::read_fragment(vault, &p.id).map_err(map_parc_error)?;
            return Err(RpcError::conflict(&current.updated_at.to_rfc3339()));
        }
    }
    let mut writer = write::writer(vault);
    let frag = writer
        .set_archived(&p.id, p.archived)
        .map_err(map_parc_error)?;
    Ok(write::with_warnings(saved_json(vault, &frag)?, &mut writer))
}

#[derive(Deserialize)]
pub struct DeleteParams {
    pub id: String,
}

pub fn delete(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: DeleteParams = extract_params(params)?;
    let mut writer = write::writer(vault);
    let deleted = writer.delete(&p.id).map_err(map_parc_error)?;
    let result = serde_json::json!({
        "id": deleted.id,
        "deleted": true,
    });
    Ok(write::with_warnings(result, &mut writer))
}

#[derive(Deserialize)]
pub struct ListParams {
    #[serde(rename = "type")]
    pub fragment_type: Option<String>,
    pub status: Option<String>,
    pub tag: Option<String>,
    pub limit: Option<usize>,
    pub sort: Option<String>,
}

pub fn list(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: ListParams = extract_params(params)?;

    let mut filters = Vec::new();
    if let Some(t) = p.fragment_type {
        filters.push(Filter::Type {
            value: t,
            negated: false,
        });
    }
    if let Some(s) = p.status {
        filters.push(Filter::Status {
            value: s,
            negated: false,
        });
    }
    if let Some(tag) = p.tag {
        filters.push(Filter::Tag {
            value: tag,
            negated: false,
        });
    }

    let query = SearchQuery {
        text_terms: Vec::new(),
        filters,
        sort: SortOrder::UpdatedDesc,
        limit: p.limit,
    };

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    let results = search::search(&conn, &query).map_err(map_parc_error)?;

    let items: Vec<Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "type": r.fragment_type,
                "title": r.title,
                "status": r.status,
                "tags": r.tags,
                "updated_at": r.updated_at,
                "created_at": r.created_at,
            })
        })
        .collect();

    Ok(Value::Array(items))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrepParams {
    pub pattern: String,
    pub limit: Option<usize>,
}

/// Lines of fragment bodies, and of their attachments' extracted text, that
/// contain `pattern` (literal text, smart case).
pub fn grep(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GrepParams = extract_params(params)?;
    let limit = p.limit.unwrap_or(200).min(1000);
    let conn = index::open_index(vault).map_err(map_parc_error)?;
    let result = parc_core::grep::grep(&conn, &p.pattern, limit).map_err(map_parc_error)?;
    serde_json::to_value(result).map_err(|e| RpcError::internal_error(&e.to_string()))
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub query: String,
    pub limit: Option<usize>,
    pub sort: Option<String>,
}

pub fn search(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: SearchParams = extract_params(params)?;

    let mut query = search::parse_query(&p.query).map_err(map_parc_error)?;
    if let Some(limit) = p.limit {
        query.limit = Some(limit);
    }

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    let results = search::search(&conn, &query).map_err(map_parc_error)?;

    let items: Vec<Value> = results
        .iter()
        .map(|r| {
            let mut obj = serde_json::json!({
                "id": r.id,
                "type": r.fragment_type,
                "title": r.title,
                "status": r.status,
                "tags": r.tags,
                "updated_at": r.updated_at,
                "created_at": r.created_at,
                "snippet": r.snippet,
                "archived": r.archived,
                "fields": r.fields,
            });
            if !r.file_hits.is_empty() {
                obj["file_hits"] = serde_json::to_value(&r.file_hits).unwrap_or(Value::Null);
            }
            obj
        })
        .collect();

    Ok(Value::Array(items))
}
