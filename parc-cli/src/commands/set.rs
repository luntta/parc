use anyhow::{bail, Result};
use chrono::Utc;
use parc_core::fragment::{read_fragment, validate_fragment, write_fragment};
use parc_core::index;
use parc_core::schema::load_schemas;
use parc_core::vault::discover_vault;
use serde_json::Value;

pub fn run(id: &str, field: &str, value: &str) -> Result<()> {
    let vault = discover_vault()?;
    let schemas = load_schemas(&vault)?;
    let mut fragment = read_fragment(&vault, id)?;

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
            fragment
                .extra_fields
                .insert(field.to_string(), Value::String(value.to_string()));
        }
    }

    // Validate
    if let Some(s) = schemas.resolve(&fragment.fragment_type) {
        validate_fragment(&fragment, s)?;
    }

    fragment.updated_at = Utc::now();
    write_fragment(&vault, &fragment)?;

    let conn = index::open_index(&vault)?;
    index::index_fragment_auto(&conn, &fragment, &vault)?;

    println!("Updated {} field '{}' to '{}'", &fragment.id[..8], field, value);
    Ok(())
}
