use std::path::Path;

use anyhow::Result;
use parc_core::schema::{load_schemas, FieldType};

pub fn run(vault: &Path) -> Result<()> {
    let schemas = load_schemas(vault)?;
    let types = schemas.list();

    println!("{:<12} {:<7} FIELDS", "NAME", "ALIAS");
    for schema in types {
        let alias = schema.alias.as_deref().unwrap_or("-");
        let fields: Vec<String> = schema
            .fields
            .iter()
            .map(|f| {
                let type_hint = match &f.field_type {
                    FieldType::Enum(vals) => format!("({})", vals.join("|")),
                    FieldType::Date => "(date)".to_string(),
                    FieldType::ListOfStrings => "(list)".to_string(),
                    FieldType::String => String::new(),
                };
                if type_hint.is_empty() {
                    f.name.clone()
                } else {
                    format!("{} {}", f.name, type_hint)
                }
            })
            .collect();
        let fields_str = if fields.is_empty() {
            "(none)".to_string()
        } else {
            fields.join(", ")
        };
        println!("{:<12} {:<7} {}", schema.name, alias, fields_str);
    }

    Ok(())
}
