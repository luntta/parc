use std::path::Path;

use serde::Deserialize;

use crate::error::ParcError;
use crate::secure_fs;

#[derive(Debug, Clone)]
pub struct Schema {
    pub name: String,
    pub alias: Option<String>,
    pub editor_skip: bool,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub default: Option<String>,
    /// `toggle: [off, on]` on an enum field: the pair of values a one-key
    /// toggle (the TUI's `t`) flips between.
    pub toggle: Option<(String, String)>,
}

/// Two values of one field that a one-key toggle flips between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toggle {
    pub field: String,
    pub off: String,
    pub on: String,
}

impl Toggle {
    /// `on` goes back to `off`; any other value, or none, goes to `on`.
    /// So for `[open, done]`, an in-progress todo is finished, not reopened.
    pub fn next(&self, current: Option<&str>) -> &str {
        if current == Some(self.on.as_str()) {
            &self.off
        } else {
            &self.on
        }
    }
}

impl Schema {
    /// The field and values a one-key toggle should flip: the first field
    /// that declares `toggle:`. Schemas without one fall back to
    /// `status: open <-> done` when that enum has both values, which keeps
    /// todo schemas written before `toggle:` existed working.
    pub fn toggle(&self) -> Option<Toggle> {
        if let Some(field) = self.fields.iter().find(|f| f.toggle.is_some()) {
            let (off, on) = field.toggle.clone()?;
            return Some(Toggle {
                field: field.name.clone(),
                off,
                on,
            });
        }
        let status = self.fields.iter().find(|f| f.name == "status")?;
        match &status.field_type {
            FieldType::Enum(values)
                if values.iter().any(|v| v == "open") && values.iter().any(|v| v == "done") =>
            {
                Some(Toggle {
                    field: "status".to_string(),
                    off: "open".to_string(),
                    on: "done".to_string(),
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum FieldType {
    String,
    Date,
    Enum(Vec<String>),
    ListOfStrings,
}

#[derive(Deserialize)]
struct SchemaFile {
    name: String,
    alias: Option<String>,
    #[serde(default)]
    editor_skip: bool,
    #[serde(default)]
    fields: Vec<FieldDefFile>,
}

#[derive(Deserialize)]
struct FieldDefFile {
    name: String,
    #[serde(rename = "type")]
    field_type: String,
    #[serde(default)]
    values: Vec<String>,
    #[serde(default)]
    required: bool,
    default: Option<String>,
    #[serde(default)]
    toggle: Vec<String>,
}

fn parse_toggle(schema: &str, field: &FieldDefFile) -> Result<Option<(String, String)>, ParcError> {
    if field.toggle.is_empty() {
        return Ok(None);
    }
    let invalid = |why: String| {
        ParcError::ValidationError(format!(
            "schema '{}', field '{}': toggle {}",
            schema, field.name, why
        ))
    };
    if field.field_type != "enum" {
        return Err(invalid("is only supported on enum fields".into()));
    }
    let [off, on] = field.toggle.as_slice() else {
        return Err(invalid(format!(
            "needs exactly two values, e.g. [open, done]; got {}",
            field.toggle.len()
        )));
    };
    for value in [off, on] {
        if !field.values.contains(value) {
            return Err(invalid(format!(
                "value '{}' is not one of {:?}",
                value, field.values
            )));
        }
    }
    Ok(Some((off.clone(), on.clone())))
}

fn parse_field_type(type_str: &str, values: Vec<String>) -> FieldType {
    match type_str {
        "string" => FieldType::String,
        "date" => FieldType::Date,
        "enum" => FieldType::Enum(values),
        "list" => FieldType::ListOfStrings,
        _ => FieldType::String,
    }
}

pub fn parse_schema(yaml: &str) -> Result<Schema, ParcError> {
    let raw: SchemaFile = serde_yaml_ng::from_str(yaml)?;
    let fields = raw
        .fields
        .into_iter()
        .map(|f| {
            let toggle = parse_toggle(&raw.name, &f)?;
            Ok(FieldDef {
                name: f.name,
                field_type: parse_field_type(&f.field_type, f.values),
                required: f.required,
                default: f.default,
                toggle,
            })
        })
        .collect::<Result<_, ParcError>>()?;
    Ok(Schema {
        name: raw.name,
        alias: raw.alias,
        editor_skip: raw.editor_skip,
        fields,
    })
}

pub struct SchemaRegistry {
    schemas: Vec<Schema>,
}

impl SchemaRegistry {
    pub fn get_by_name(&self, name: &str) -> Option<&Schema> {
        self.schemas.iter().find(|s| s.name == name)
    }

    pub fn get_by_alias(&self, alias: &str) -> Option<&Schema> {
        self.schemas
            .iter()
            .find(|s| s.alias.as_deref() == Some(alias))
    }

    pub fn resolve(&self, name_or_alias: &str) -> Option<&Schema> {
        self.get_by_name(name_or_alias)
            .or_else(|| self.get_by_alias(name_or_alias))
    }

    pub fn list(&self) -> Vec<&Schema> {
        self.schemas.iter().collect()
    }
}

/// Load all schemas from the vault's schemas/ directory.
pub fn load_schemas(vault_path: &Path) -> Result<SchemaRegistry, ParcError> {
    let schemas_dir = vault_path.join("schemas");
    let mut schemas = Vec::new();

    if schemas_dir.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(&schemas_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let content = std::fs::read_to_string(entry.path())?;
            let schema = parse_schema(&content)?;
            schemas.push(schema);
        }
    }

    Ok(SchemaRegistry { schemas })
}

/// Get the built-in template content for a type.
pub fn get_builtin_template(type_name: &str) -> Option<&'static str> {
    match type_name {
        "note" => Some(include_str!("builtin/templates/note.md")),
        "todo" => Some(include_str!("builtin/templates/todo.md")),
        "decision" => Some(include_str!("builtin/templates/decision.md")),
        "risk" => Some(include_str!("builtin/templates/risk.md")),
        "idea" => Some(include_str!("builtin/templates/idea.md")),
        _ => None,
    }
}

/// Load template from vault or fall back to built-in.
pub fn load_template(vault_path: &Path, type_name: &str) -> Option<String> {
    let template_path = vault_path
        .join("templates")
        .join(format!("{}.md", type_name));
    if template_path.exists() {
        std::fs::read_to_string(template_path).ok()
    } else {
        get_builtin_template(type_name).map(|s| s.to_string())
    }
}

/// Validate a schema YAML file at the given path. Returns the parsed Schema.
pub fn validate_schema_file(source_path: &Path) -> Result<Schema, ParcError> {
    let content = std::fs::read_to_string(source_path)?;
    parse_schema(&content)
}

/// Schema names become file names in `schemas/` and `templates/`. Restrict to
/// a strict identifier so a malicious schema cannot overwrite arbitrary files
/// (e.g. name='../config').
pub fn validate_schema_name(name: &str) -> Result<(), ParcError> {
    if name.is_empty() || name.len() > 64 {
        return Err(ParcError::ValidationError(format!(
            "schema name '{}' must be 1-64 characters",
            name
        )));
    }
    let first = name.as_bytes()[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_alphabetic()) {
        return Err(ParcError::ValidationError(format!(
            "schema name '{}' must start with a letter",
            name
        )));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(ParcError::ValidationError(format!(
            "schema name '{}' must contain only letters, digits, '_' or '-'",
            name
        )));
    }
    Ok(())
}

/// Register a user-defined schema by copying it into the vault's schemas/ directory.
/// Returns the schema name. Errors if a schema with the same name already exists.
pub fn add_schema(vault_path: &Path, source_path: &Path) -> Result<String, ParcError> {
    let schema = validate_schema_file(source_path)?;
    validate_schema_name(&schema.name)?;
    let registry = load_schemas(vault_path)?;

    if registry.get_by_name(&schema.name).is_some() {
        return Err(ParcError::ValidationError(format!(
            "schema '{}' already exists in vault",
            schema.name
        )));
    }

    // Copy to schemas/
    let dest = vault_path
        .join("schemas")
        .join(format!("{}.yml", schema.name));
    secure_fs::copy_private_new(source_path, &dest)?;

    // Create empty template if none exists
    let template_path = vault_path
        .join("templates")
        .join(format!("{}.md", schema.name));
    if !template_path.exists() {
        let template_dir = vault_path.join("templates");
        if !template_dir.exists() {
            secure_fs::create_private_dir_all(&template_dir)?;
        }
        secure_fs::write_private_new(
            &template_path,
            format!("---\ntype: {}\ntitle: \"\"\n---\n\n", schema.name),
        )?;
    }

    Ok(schema.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_todo_schema() {
        let yaml = include_str!("builtin/schemas/todo.yml");
        let schema = parse_schema(yaml).unwrap();
        assert_eq!(schema.name, "todo");
        assert_eq!(schema.alias.as_deref(), Some("t"));
        assert!(!schema.editor_skip);
        assert_eq!(schema.fields.len(), 4);
        assert_eq!(schema.fields[0].name, "status");
        assert!(schema.fields[0].required);
        assert_eq!(schema.fields[0].default.as_deref(), Some("open"));
    }

    #[test]
    fn test_builtin_toggles() {
        let toggle = |yaml: &str| parse_schema(yaml).unwrap().toggle();
        let pair = |t: Option<Toggle>| t.map(|t| (t.field, t.off, t.on));
        let s = |v: &str| v.to_string();

        assert_eq!(
            pair(toggle(include_str!("builtin/schemas/todo.yml"))),
            Some((s("status"), s("open"), s("done")))
        );
        assert_eq!(
            pair(toggle(include_str!("builtin/schemas/risk.yml"))),
            Some((s("status"), s("identified"), s("resolved")))
        );
        assert_eq!(
            pair(toggle(include_str!("builtin/schemas/decision.yml"))),
            Some((s("status"), s("proposed"), s("accepted")))
        );
        assert_eq!(toggle(include_str!("builtin/schemas/idea.yml")), None);
        assert_eq!(toggle(include_str!("builtin/schemas/note.yml")), None);
    }

    #[test]
    fn test_toggle_next() {
        let t = Toggle {
            field: "status".into(),
            off: "open".into(),
            on: "done".into(),
        };
        assert_eq!(t.next(Some("open")), "done");
        assert_eq!(t.next(Some("in-progress")), "done");
        assert_eq!(t.next(None), "done");
        assert_eq!(t.next(Some("done")), "open");
    }

    #[test]
    fn test_toggle_falls_back_to_open_done_without_declaration() {
        // A todo schema copied into a vault before `toggle:` existed.
        let yaml = "name: todo\nfields:\n  - name: status\n    type: enum\n    values: [open, in-progress, done]\n";
        let t = parse_schema(yaml).unwrap().toggle().unwrap();
        assert_eq!((t.off.as_str(), t.on.as_str()), ("open", "done"));
    }

    #[test]
    fn test_toggle_on_custom_field() {
        let yaml = "name: bug\nfields:\n  - name: state\n    type: enum\n    values: [new, fixed, wontfix]\n    toggle: [new, fixed]\n";
        let t = parse_schema(yaml).unwrap().toggle().unwrap();
        assert_eq!(t.field, "state");
        assert_eq!(t.next(Some("wontfix")), "fixed");
    }

    #[test]
    fn test_toggle_rejects_bad_declarations() {
        let field = |extra: &str| {
            format!(
                "name: bug\nfields:\n  - name: state\n    type: enum\n    values: [new, fixed]\n    {}\n",
                extra
            )
        };
        let err = |yaml: String| parse_schema(&yaml).unwrap_err().to_string();

        assert!(err(field("toggle: [new]")).contains("exactly two values"));
        assert!(err(field("toggle: [new, closed]")).contains("'closed' is not one of"));
        let not_enum = "name: x\nfields:\n  - name: owner\n    type: string\n    toggle: [a, b]\n";
        assert!(err(not_enum.to_string()).contains("only supported on enum"));
    }

    #[test]
    fn test_parse_all_schemas() {
        for (_name, content) in crate::vault::BUILTIN_SCHEMAS {
            let schema = parse_schema(content).unwrap();
            assert!(!schema.name.is_empty());
        }
    }

    #[test]
    fn test_schema_registry_resolve() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let registry = load_schemas(&vault).unwrap();

        assert!(registry.resolve("todo").is_some());
        assert!(registry.resolve("t").is_some());
        assert_eq!(
            registry.resolve("todo").unwrap().name,
            registry.resolve("t").unwrap().name
        );
        assert!(registry.resolve("nonexistent").is_none());
        assert_eq!(registry.list().len(), 5);
    }
}
