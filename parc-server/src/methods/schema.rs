use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use parc_core::config::{load_config, TypesConfig};
use parc_core::ops::ValueRename;
use parc_core::schema::{
    self, load_schemas, FieldDef, FieldType, Schema, SchemaRegistry, StatusCategory,
};

use crate::jsonrpc::RpcError;
use crate::methods::write::{with_warnings, writer};
use crate::router::{extract_params, map_parc_error};

/// What the vault's settings say about each type: on or off, and which one
/// captures start as.
struct TypeState {
    types: TypesConfig,
    default: Option<String>,
}

impl TypeState {
    fn load(vault: &Path, registry: &SchemaRegistry) -> Result<Self, RpcError> {
        let types = load_config(vault).map_err(map_parc_error)?.types;
        let default = types.default_type(registry).map(|s| s.name.clone());
        Ok(TypeState { types, default })
    }
}

/// Extract the body of a type's Markdown template (stripping optional YAML
/// frontmatter), or `None` when the template is blank.
fn template_body(vault: &Path, type_name: &str) -> Option<String> {
    let raw = schema::load_template(vault, type_name)?;
    let trimmed = raw.trim_start();
    let body = if let Some(rest) = trimmed.strip_prefix("---") {
        let rest = rest
            .strip_prefix("\r\n")
            .or_else(|| rest.strip_prefix('\n'))
            .unwrap_or(rest);
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            after
                .strip_prefix("\r\n")
                .or_else(|| after.strip_prefix('\n'))
                .unwrap_or(after)
        } else {
            raw.as_str()
        }
    } else {
        raw.as_str()
    };
    let clean = body.trim_start_matches(['\r', '\n']);
    (!clean.trim().is_empty()).then(|| clean.to_string())
}

/// A schema as clients see it. `type` keeps its original debug spelling for
/// existing clients; `kind`, `values` and `toggle` are what a form needs.
/// The status field carries `categories`: every value with what it means
/// (open, active, done or closed). `enabled` and `default` come from the
/// vault's `types:` settings; `builtin` marks the five types parc ships.
fn schema_to_json(vault: &Path, s: &Schema, state: &TypeState) -> Value {
    let fields: Vec<Value> = s
        .fields
        .iter()
        .map(|f| {
            let values = match &f.field_type {
                FieldType::Enum(values) => Some(values.clone()),
                _ => None,
            };
            let categories = (f.name == "status" && values.is_some()).then(|| {
                s.status_categories()
                    .into_iter()
                    .map(|(value, c)| (value, Value::from(c.as_str())))
                    .collect::<serde_json::Map<_, _>>()
            });
            serde_json::json!({
                "name": f.name,
                "type": format!("{:?}", f.field_type),
                "kind": f.field_type.kind(),
                "values": values,
                "required": f.required,
                "default": f.default,
                "toggle": f.toggle.as_ref().map(|(off, on)| [off, on]),
                "categories": categories,
            })
        })
        .collect();

    serde_json::json!({
        "name": s.name,
        "alias": s.alias,
        "editor_skip": s.editor_skip,
        "fields": fields,
        // The one-key toggle (the TUI's `t`), including the status open/done fallback.
        "toggle": s.toggle().map(|t| serde_json::json!({ "field": t.field, "off": t.off, "on": t.on })),
        "enabled": state.types.is_enabled(&s.name),
        "default": state.default.as_deref() == Some(s.name.as_str()),
        "builtin": schema::is_builtin(&s.name),
        "template": template_body(vault, &s.name),
    })
}

pub fn list(vault: &Path, _params: Value) -> Result<Value, RpcError> {
    let registry = load_schemas(vault).map_err(map_parc_error)?;
    let state = TypeState::load(vault, &registry)?;
    Ok(Value::Array(
        registry
            .list()
            .iter()
            .map(|s| schema_to_json(vault, s, &state))
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct GetParams {
    #[serde(rename = "type")]
    pub type_name: String,
}

pub fn get(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    let registry = load_schemas(vault).map_err(map_parc_error)?;

    let schema = registry
        .resolve(&p.type_name)
        .ok_or_else(|| RpcError::invalid_params(&format!("unknown type: {}", p.type_name)))?;

    Ok(schema_to_json(
        vault,
        schema,
        &TypeState::load(vault, &registry)?,
    ))
}

/// The definition parc ships for a built-in type, to put an edited one back.
pub fn builtin(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    let schema = schema::builtin_schema(&p.type_name).ok_or_else(|| {
        RpcError::invalid_params(&format!("'{}' is not a built-in type", p.type_name))
    })?;
    let registry = load_schemas(vault).map_err(map_parc_error)?;
    Ok(schema_to_json(
        vault,
        &schema,
        &TypeState::load(vault, &registry)?,
    ))
}

/// How many fragments a type has, and how many hold each value of each of
/// its choice fields: `{ "total", "values": { field: { value: count } } }`.
pub fn usage(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    let usage = schema::type_usage(vault, &p.type_name).map_err(map_parc_error)?;
    serde_json::to_value(usage).map_err(|e| RpcError::internal_error(&e.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldInput {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub values: Option<Vec<String>>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub toggle: Option<[String; 2]>,
    /// Status value → open, active, done or closed. Omitted: the built-in words decide.
    #[serde(default)]
    pub categories: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaInput {
    pub name: String,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub editor_skip: bool,
    #[serde(default)]
    pub fields: Vec<FieldInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameInput {
    pub field: String,
    pub from: String,
    pub to: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveParams {
    pub schema: SchemaInput,
    /// true for a new type; false to change one that exists.
    #[serde(default)]
    pub create: bool,
    /// Fragments to move from one value of a choice field to another: a
    /// renamed value, or a removed one.
    #[serde(default)]
    pub renames: Vec<RenameInput>,
}

fn schema_from_input(input: SchemaInput) -> Result<Schema, RpcError> {
    let fields = input
        .fields
        .into_iter()
        .map(|f| {
            let field_type = match f.kind.as_str() {
                "string" => FieldType::String,
                "date" => FieldType::Date,
                "list" => FieldType::ListOfStrings,
                "enum" => FieldType::Enum(f.values.unwrap_or_default()),
                other => {
                    return Err(RpcError::invalid_params(&format!(
                        "field '{}': unknown kind '{}' (string, date, enum or list)",
                        f.name, other
                    )))
                }
            };
            let categories = f
                .categories
                .map(|map| {
                    map.into_iter()
                        .map(|(value, c)| {
                            StatusCategory::parse(&c)
                                .map(|c| (value, c))
                                .ok_or_else(|| {
                                    RpcError::invalid_params(&format!(
                                        "field '{}': unknown category '{}' (open, active, done or closed)",
                                        f.name, c
                                    ))
                                })
                        })
                        .filter(|r| !matches!(r, Ok((_, StatusCategory::Open))))
                        .collect::<Result<BTreeMap<_, _>, _>>()
                })
                .transpose()?;
            Ok(FieldDef {
                name: f.name,
                field_type,
                required: f.required,
                default: f.default.filter(|d| !d.is_empty()),
                toggle: f.toggle.map(|[off, on]| (off, on)),
                categories,
            })
        })
        .collect::<Result<_, RpcError>>()?;
    Ok(Schema {
        name: input.name,
        alias: input.alias.filter(|a| !a.is_empty()),
        editor_skip: input.editor_skip,
        fields,
    })
}

/// Create a type, or change one and move its fragments along. Refused, with
/// nothing written, when the change would leave a fragment invalid.
/// Result: `{ "schema", "migrated", "warnings"? }`.
pub fn save(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: SaveParams = extract_params(params)?;
    let schema = schema_from_input(p.schema)?;
    let renames: Vec<ValueRename> = p
        .renames
        .into_iter()
        .map(|r| ValueRename {
            field: r.field,
            from: r.from,
            to: r.to,
        })
        .collect();
    let mut w = writer(vault);
    let migrated = w
        .save_schema(&schema, p.create, &renames)
        .map_err(map_parc_error)?;
    let registry = load_schemas(vault).map_err(map_parc_error)?;
    let saved = registry
        .get_by_name(&schema.name)
        .ok_or_else(|| RpcError::internal_error("the saved type did not load"))?;
    let result = serde_json::json!({
        "schema": schema_to_json(vault, saved, &TypeState::load(vault, &registry)?),
        "migrated": migrated,
    });
    Ok(with_warnings(result, &mut w))
}

/// Remove a type no fragment uses. Result: `{ "deleted": true }`.
pub fn delete(vault: &Path, params: Value) -> Result<Value, RpcError> {
    let p: GetParams = extract_params(params)?;
    schema::delete_schema(vault, &p.type_name).map_err(map_parc_error)?;
    Ok(serde_json::json!({ "deleted": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        parc_core::vault::init_vault(&vault).unwrap();
        (tmp, vault)
    }

    #[test]
    fn list_reports_state_and_categories() {
        let (_tmp, vault) = vault();
        let list = list(&vault, Value::Null).unwrap();
        let todo = list
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "todo")
            .unwrap();
        assert_eq!(todo["enabled"], true);
        assert_eq!(todo["default"], false);
        assert_eq!(todo["builtin"], true);
        let status = &todo["fields"][0];
        assert_eq!(status["categories"]["in-progress"], "active");
        assert_eq!(status["categories"]["open"], "open");
        assert!(todo["fields"][1]["categories"].is_null());
    }

    #[test]
    fn what_list_returns_saves_back_unchanged() {
        let (_tmp, vault) = vault();
        let before = std::fs::read_to_string(vault.join("schemas/risk.yml")).unwrap();
        let risk = get(&vault, serde_json::json!({ "type": "risk" })).unwrap();
        let fields: Vec<Value> = risk["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                serde_json::json!({
                    "name": f["name"], "kind": f["kind"], "values": f["values"],
                    "required": f["required"], "default": f["default"],
                    "toggle": f["toggle"], "categories": f["categories"],
                })
            })
            .collect();
        let params = serde_json::json!({
            "schema": { "name": "risk", "alias": risk["alias"], "editor_skip": false, "fields": fields },
        });
        let saved = save(&vault, params).unwrap();
        assert_eq!(saved["migrated"], 0);
        assert_eq!(saved["schema"], risk);
        let after = parc_core::schema::parse_schema(
            &std::fs::read_to_string(vault.join("schemas/risk.yml")).unwrap(),
        )
        .unwrap();
        let original = parc_core::schema::parse_schema(&before).unwrap();
        assert_eq!(after.fields[0].categories, original.fields[0].categories);
    }

    #[test]
    fn save_rejects_unknown_kinds_and_categories() {
        let (_tmp, vault) = vault();
        let bad_kind = serde_json::json!({
            "schema": { "name": "bug", "fields": [{ "name": "n", "kind": "number" }] }, "create": true,
        });
        assert!(save(&vault, bad_kind).is_err());
        let bad_category = serde_json::json!({
            "schema": { "name": "bug", "fields": [{ "name": "status", "kind": "enum", "values": ["a"], "categories": { "a": "finished" } }] },
            "create": true,
        });
        assert!(save(&vault, bad_category).is_err());
        assert!(!vault.join("schemas/bug.yml").exists());
    }
}
