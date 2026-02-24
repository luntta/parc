use std::collections::BTreeMap;

use chrono::Utc;
use serde::Deserialize;
use tauri::State;

use parc_core::config::load_config;
use parc_core::fragment;
use parc_core::index;
use parc_core::schema::load_schemas;
use parc_core::search::{self, Filter, SearchQuery, SortOrder};

use crate::dto::{FragmentDto, FragmentSummaryDto};
use crate::error::GuiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListParams {
    #[serde(rename = "type")]
    pub fragment_type: Option<String>,
    pub status: Option<String>,
    pub tag: Option<String>,
    pub limit: Option<usize>,
}

#[tauri::command]
pub fn list_fragments(
    state: State<'_, AppState>,
    params: ListParams,
) -> Result<Vec<FragmentSummaryDto>, GuiError> {
    let vault = state.vault_path();

    let mut filters = Vec::new();
    if let Some(t) = params.fragment_type {
        filters.push(Filter::Type { value: t, negated: false });
    }
    if let Some(s) = params.status {
        filters.push(Filter::Status { value: s, negated: false });
    }
    if let Some(tag) = params.tag {
        filters.push(Filter::Tag { value: tag, negated: false });
    }

    let query = SearchQuery {
        text_terms: Vec::new(),
        filters,
        sort: SortOrder::UpdatedDesc,
        limit: params.limit,
    };

    let conn = index::open_index(&vault)?;
    let results = search::search(&conn, &query)?;

    Ok(results
        .into_iter()
        .map(|r| FragmentSummaryDto {
            id: r.id,
            fragment_type: r.fragment_type,
            title: r.title,
            status: r.status,
            tags: r.tags,
            updated_at: r.updated_at,
        })
        .collect())
}

#[tauri::command]
pub fn get_fragment(
    state: State<'_, AppState>,
    id: String,
) -> Result<FragmentDto, GuiError> {
    let vault = state.vault_path();
    let frag = fragment::read_fragment(&vault, &id)?;
    Ok(FragmentDto::from(&frag))
}

#[derive(Debug, Deserialize)]
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

#[tauri::command]
pub fn create_fragment(
    state: State<'_, AppState>,
    params: CreateParams,
) -> Result<FragmentDto, GuiError> {
    let vault = state.vault_path();
    let config = load_config(&vault)?;
    let schemas = load_schemas(&vault)?;

    let schema = schemas
        .resolve(&params.fragment_type)
        .ok_or_else(|| GuiError::Other(format!("unknown type: {}", params.fragment_type)))?;

    let resolved_type = schema.name.clone();
    let title = params.title.as_deref().unwrap_or("");
    let mut frag = fragment::new_fragment(&resolved_type, title, schema, &config);

    for tag in &params.tags {
        if !frag.tags.contains(tag) {
            frag.tags.push(tag.clone());
        }
    }
    frag.links = params.links;

    if let Some(body) = params.body {
        frag.body = body;
    }
    if let Some(status) = params.status {
        frag.extra_fields
            .insert("status".to_string(), serde_json::Value::String(status));
    }
    if let Some(due) = params.due {
        let resolved = parc_core::date::resolve_due_date(&due)
            .map_err(|e| GuiError::Other(e.to_string()))?;
        frag.extra_fields
            .insert("due".to_string(), serde_json::Value::String(resolved));
    }
    if let Some(priority) = params.priority {
        frag.extra_fields
            .insert("priority".to_string(), serde_json::Value::String(priority));
    }
    if let Some(assignee) = params.assignee {
        frag.extra_fields
            .insert("assignee".to_string(), serde_json::Value::String(assignee));
    }

    fragment::validate_fragment(&frag, schema)?;
    fragment::create_fragment(&vault, &frag)?;

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &frag, &vault)?;

    Ok(FragmentDto::from(&frag))
}

#[derive(Debug, Deserialize)]
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
    pub extra_fields: Option<BTreeMap<String, serde_json::Value>>,
}

#[tauri::command]
pub fn update_fragment(
    state: State<'_, AppState>,
    params: UpdateParams,
) -> Result<FragmentDto, GuiError> {
    let vault = state.vault_path();
    let mut frag = fragment::read_fragment(&vault, &params.id)?;

    if let Some(title) = params.title {
        frag.title = title;
    }
    if let Some(tags) = params.tags {
        frag.tags = tags;
    }
    if let Some(body) = params.body {
        frag.body = body;
    }
    if let Some(links) = params.links {
        frag.links = links;
    }
    if let Some(status) = params.status {
        frag.extra_fields
            .insert("status".to_string(), serde_json::Value::String(status));
    }
    if let Some(due) = params.due {
        let resolved = parc_core::date::resolve_due_date(&due)
            .map_err(|e| GuiError::Other(e.to_string()))?;
        frag.extra_fields
            .insert("due".to_string(), serde_json::Value::String(resolved));
    }
    if let Some(priority) = params.priority {
        frag.extra_fields
            .insert("priority".to_string(), serde_json::Value::String(priority));
    }
    if let Some(assignee) = params.assignee {
        frag.extra_fields
            .insert("assignee".to_string(), serde_json::Value::String(assignee));
    }
    if let Some(extra) = params.extra_fields {
        for (k, v) in extra {
            frag.extra_fields.insert(k, v);
        }
    }

    frag.updated_at = Utc::now();
    fragment::write_fragment(&vault, &frag)?;

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &frag, &vault)?;

    Ok(FragmentDto::from(&frag))
}

#[tauri::command]
pub fn delete_fragment(
    state: State<'_, AppState>,
    id: String,
) -> Result<String, GuiError> {
    let vault = state.vault_path();
    let full_id = fragment::delete_fragment(&vault, &id)?;

    let conn = index::open_index(&vault)?;
    index::remove_from_index(&conn, &full_id)?;

    Ok(full_id)
}

#[tauri::command]
pub fn archive_fragment(
    state: State<'_, AppState>,
    id: String,
    undo: Option<bool>,
) -> Result<FragmentDto, GuiError> {
    let vault = state.vault_path();
    let mut frag = fragment::read_fragment(&vault, &id)?;

    if undo.unwrap_or(false) {
        frag.extra_fields.remove("archived");
    } else {
        frag.extra_fields
            .insert("archived".to_string(), serde_json::Value::Bool(true));
    }

    frag.updated_at = Utc::now();
    fragment::write_fragment(&vault, &frag)?;

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &frag, &vault)?;

    Ok(FragmentDto::from(&frag))
}
