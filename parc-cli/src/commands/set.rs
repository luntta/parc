use std::path::Path;

use anyhow::{bail, Result};
use chrono::Utc;
use parc_core::fragment::{read_fragment, validate_fragment, write_fragment};
use parc_core::hook::{self, HookEvent};
use parc_core::index;
use parc_core::schema::load_schemas;
use serde_json::Value;

use crate::hooks::CliHookRunner;

pub fn run(vault: &Path, id: &str, field: &str, value: &str, json: bool) -> Result<()> {
    let schemas = load_schemas(vault)?;
    let mut fragment = read_fragment(vault, id)?;

    // Set the field
    match field {
        "title" => {
            fragment.title = value.to_string();
        }
        "type" => {
            fragment.fragment_type = value.to_string();
        }
        _ => {
            // Check if it's a known schema field
            let schema = schemas.resolve(&fragment.fragment_type);
            if let Some(s) = &schema {
                let known = s.fields.iter().any(|f| f.name == field);
                if !known {
                    bail!(
                        "unknown field '{}' for type '{}'. Known fields: title, type, {}",
                        field,
                        fragment.fragment_type,
                        s.fields
                            .iter()
                            .map(|f| f.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            let final_value = if field == "due" {
                parc_core::date::resolve_due_date(value)?
            } else {
                value.to_string()
            };
            fragment
                .extra_fields
                .insert(field.to_string(), Value::String(final_value));
        }
    }

    // Validate
    if let Some(s) = schemas.resolve(&fragment.fragment_type) {
        validate_fragment(&fragment, s)?;
    }

    fragment.updated_at = Utc::now();

    let runner = CliHookRunner;
    let fragment = hook::run_pre_hooks(&runner, vault, HookEvent::PreUpdate, &fragment)?;

    write_fragment(vault, &fragment)?;

    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &fragment, vault)?;

    hook::run_post_hooks(&runner, vault, HookEvent::PostUpdate, &fragment);

    if json {
        let json_val = serde_json::json!({
            "id": fragment.id,
            "field": field,
            "value": value,
            "updated": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!("Updated {} field '{}' to '{}'", &fragment.id[..8], field, value);
    }
    Ok(())
}
