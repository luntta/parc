use serde::Deserialize;
use tauri::State;

use parc_core::schema::{self, FieldType};

use crate::dto::{SchemaDto, SchemaFieldDto};
use crate::error::GuiError;
use crate::state::AppState;

fn schema_to_dto(s: &schema::Schema) -> SchemaDto {
    SchemaDto {
        name: s.name.clone(),
        alias: s.alias.clone(),
        editor_skip: s.editor_skip,
        fields: s
            .fields
            .iter()
            .map(|f| {
                let (type_str, values) = match &f.field_type {
                    FieldType::String => ("string".to_string(), vec![]),
                    FieldType::Date => ("date".to_string(), vec![]),
                    FieldType::Enum(vals) => ("enum".to_string(), vals.clone()),
                    FieldType::ListOfStrings => ("list".to_string(), vec![]),
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
}

#[tauri::command]
pub fn list_schemas(state: State<'_, AppState>) -> Result<Vec<SchemaDto>, GuiError> {
    let vault = state.vault_path();
    let registry = schema::load_schemas(&vault)?;
    Ok(registry.list().iter().map(|s| schema_to_dto(s)).collect())
}

#[derive(Debug, Deserialize)]
pub struct GetSchemaParams {
    #[serde(rename = "type")]
    pub type_name: String,
}

#[tauri::command]
pub fn get_schema(
    state: State<'_, AppState>,
    params: GetSchemaParams,
) -> Result<SchemaDto, GuiError> {
    let vault = state.vault_path();
    let registry = schema::load_schemas(&vault)?;
    let schema = registry
        .resolve(&params.type_name)
        .ok_or_else(|| GuiError::Other(format!("unknown type: {}", params.type_name)))?;
    Ok(schema_to_dto(schema))
}
