use std::path::Path;

use chrono::Utc;
use serde::Deserialize;
use serde_json::Value;

use parc_core::config::load_config;
use parc_core::fragment::{self, Fragment};
use parc_core::index;
use parc_core::schema::load_schemas;
use parc_core::search::{self, Filter, SearchQuery, SortOrder};

use crate::jsonrpc::RpcError;
use crate::router::{extract_params, map_parc_error};

fn fragment_to_json(f: &Fragment) -> Value {
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

    fragment::validate_fragment(&frag, schema).map_err(map_parc_error)?;
    fragment::create_fragment(vault, &frag).map_err(map_parc_error)?;

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    index::index_fragment_auto(&conn, &frag, vault).map_err(map_parc_error)?;

    Ok(fragment_to_json(&frag))
}

#[derive(Deserialize)]
pub struct GetParams {
    pub id: String,
}

pub fn get(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    let frag = fragment::read_fragment(vault, &p.id).map_err(map_parc_error)?;
    Ok(fragment_to_json(&frag))
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
}

pub fn update(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: UpdateParams = extract_params(params)?;
    let mut frag = fragment::read_fragment(vault, &p.id).map_err(map_parc_error)?;

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

    frag.updated_at = Utc::now();
    fragment::write_fragment(vault, &frag).map_err(map_parc_error)?;

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    index::index_fragment_auto(&conn, &frag, vault).map_err(map_parc_error)?;

    Ok(fragment_to_json(&frag))
}

#[derive(Deserialize)]
pub struct DeleteParams {
    pub id: String,
}

pub fn delete(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: DeleteParams = extract_params(params)?;
    let full_id = fragment::delete_fragment(vault, &p.id).map_err(map_parc_error)?;

    let conn = index::open_index(vault).map_err(map_parc_error)?;
    index::remove_from_index(&conn, &full_id).map_err(map_parc_error)?;

    Ok(serde_json::json!({
        "id": full_id,
        "deleted": true,
    }))
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
            })
        })
        .collect();

    Ok(Value::Array(items))
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
            serde_json::json!({
                "id": r.id,
                "type": r.fragment_type,
                "title": r.title,
                "status": r.status,
                "tags": r.tags,
                "updated_at": r.updated_at,
                "snippet": r.snippet,
            })
        })
        .collect();

    Ok(Value::Array(items))
}
