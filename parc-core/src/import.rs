use std::path::Path;

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::error::ParcError;
use crate::fragment::{self, Fragment};

#[derive(Debug, Clone)]
pub enum ImportStatus {
    Created,
    Skipped(String),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ImportResult {
    pub id: String,
    pub title: String,
    pub status: ImportStatus,
}

/// Import fragments from a JSON string. Returns results for each entry.
/// If `dry_run` is true, validates but doesn't write anything.
pub fn import_json(
    vault: &Path,
    json_str: &str,
    dry_run: bool,
) -> Result<Vec<ImportResult>, ParcError> {
    let entries: Vec<Value> =
        serde_json::from_str(json_str).map_err(|e| ParcError::ParseError(e.to_string()))?;

    let existing_ids = fragment::list_fragment_ids(vault)?;
    let mut results = Vec::new();

    for entry in &entries {
        let obj = match entry.as_object() {
            Some(o) => o,
            None => {
                results.push(ImportResult {
                    id: String::new(),
                    title: String::new(),
                    status: ImportStatus::Error("entry is not an object".into()),
                });
                continue;
            }
        };

        let title = obj
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let fragment_type = match obj.get("type").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => {
                results.push(ImportResult {
                    id: String::new(),
                    title,
                    status: ImportStatus::Error("missing 'type' field".into()),
                });
                continue;
            }
        };

        // Check if ID already exists — if so, generate new one
        let original_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let id = if !original_id.is_empty() && existing_ids.contains(&original_id.to_string()) {
            fragment::new_id()
        } else if original_id.is_empty() {
            fragment::new_id()
        } else {
            original_id.to_string()
        };

        let tags: Vec<String> = obj
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let links: Vec<String> = obj
            .get("links")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let attachments: Vec<String> = obj
            .get("attachments")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let created_at = obj
            .get("created_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        let updated_at = obj
            .get("updated_at")
            .and_then(|v| v.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        let created_by = obj
            .get("created_by")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let body = obj
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let extra_fields: BTreeMap<String, Value> = obj
            .get("extra_fields")
            .and_then(|v| v.as_object())
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let frag = Fragment {
            id: id.clone(),
            fragment_type,
            title: title.clone(),
            tags,
            links,
            attachments,
            created_at,
            updated_at,
            created_by,
            extra_fields,
            body,
        };

        if dry_run {
            results.push(ImportResult {
                id,
                title,
                status: ImportStatus::Created,
            });
        } else {
            match fragment::create_fragment(vault, &frag) {
                Ok(_) => {
                    results.push(ImportResult {
                        id,
                        title,
                        status: ImportStatus::Created,
                    });
                }
                Err(e) => {
                    results.push(ImportResult {
                        id,
                        title,
                        status: ImportStatus::Error(e.to_string()),
                    });
                }
            }
        }
    }

    Ok(results)
}
