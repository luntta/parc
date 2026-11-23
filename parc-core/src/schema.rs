use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
    /// `categories:` on the status field: what each status means. Values not
    /// listed are open. `None` when the schema doesn't say, in which case
    /// [`fallback_category`] decides.
    pub categories: Option<BTreeMap<String, StatusCategory>>,
}

/// What a status means for attention, whatever it is called: work that is
/// waiting (`open`), under way (`active`), finished (`done`) or dropped
/// (`closed`). Done and closed fragments are "settled": they leave open-work
/// lists, due and stale reports, and the desktop's Open filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusCategory {
    Open,
    Active,
    Done,
    Closed,
}

impl StatusCategory {
    pub const ALL: [StatusCategory; 4] = [
        StatusCategory::Open,
        StatusCategory::Active,
        StatusCategory::Done,
        StatusCategory::Closed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StatusCategory::Open => "open",
            StatusCategory::Active => "active",
            StatusCategory::Done => "done",
            StatusCategory::Closed => "closed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    pub fn is_settled(self) -> bool {
        matches!(self, StatusCategory::Done | StatusCategory::Closed)
    }
}

/// The category of a status word in a schema that declares none: what the
/// built-in types' words mean, so vaults made before `categories:` existed
/// keep working. Unknown words are open.
pub fn fallback_category(status: &str) -> StatusCategory {
    match status {
        "done" | "accepted" | "resolved" | "promoted" => StatusCategory::Done,
        "cancelled" | "superseded" | "deprecated" | "parked" | "discarded" => {
            StatusCategory::Closed
        }
        "in-progress" | "mitigating" | "exploring" => StatusCategory::Active,
        _ => StatusCategory::Open,
    }
}

/// Status words [`fallback_category`] settles, for fragments whose type has no schema.
const FALLBACK_SETTLED: &[&str] = &[
    "done",
    "accepted",
    "resolved",
    "promoted",
    "cancelled",
    "superseded",
    "deprecated",
    "parked",
    "discarded",
];

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

    pub fn field(&self, name: &str) -> Option<&FieldDef> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// What a status of this type means. A schema that declares
    /// `categories:` on its status field decides; otherwise the built-in
    /// words do ([`fallback_category`]).
    pub fn status_category(&self, status: &str) -> StatusCategory {
        match self.field("status").and_then(|f| f.categories.as_ref()) {
            Some(categories) => categories
                .get(status)
                .copied()
                .unwrap_or(StatusCategory::Open),
            None => fallback_category(status),
        }
    }

    /// Every status value with its category, in the schema's order. Empty
    /// when the type has no enum status field.
    pub fn status_categories(&self) -> Vec<(String, StatusCategory)> {
        match self.field("status").map(|f| &f.field_type) {
            Some(FieldType::Enum(values)) => values
                .iter()
                .map(|v| (v.clone(), self.status_category(v)))
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Date,
    Enum(Vec<String>),
    ListOfStrings,
}

impl FieldType {
    /// The spelling in schema files: `string`, `date`, `enum` or `list`.
    pub fn kind(&self) -> &'static str {
        match self {
            FieldType::String => "string",
            FieldType::Date => "date",
            FieldType::Enum(_) => "enum",
            FieldType::ListOfStrings => "list",
        }
    }
}

#[derive(Deserialize, Serialize)]
struct SchemaFile {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    alias: Option<String>,
    #[serde(default)]
    editor_skip: bool,
    #[serde(default)]
    fields: Vec<FieldDefFile>,
}

#[derive(Deserialize, Serialize)]
struct FieldDefFile {
    name: String,
    #[serde(rename = "type")]
    field_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    values: Vec<String>,
    #[serde(default)]
    required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    toggle: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    categories: Option<CategoriesFile>,
}

/// `categories:` lists the statuses that are not open; `open:` may be given
/// for completeness. A status listed nowhere is open.
#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
struct CategoriesFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    open: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    active: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    done: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    closed: Vec<String>,
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

fn parse_categories(
    schema: &str,
    field: &FieldDefFile,
) -> Result<Option<BTreeMap<String, StatusCategory>>, ParcError> {
    let Some(file) = &field.categories else {
        return Ok(None);
    };
    let invalid = |why: String| {
        ParcError::ValidationError(format!(
            "schema '{}', field '{}': categories {}",
            schema, field.name, why
        ))
    };
    if field.name != "status" || field.field_type != "enum" {
        return Err(invalid(
            "are only supported on an enum field named status".into(),
        ));
    }
    let mut categories = BTreeMap::new();
    for (category, values) in [
        (StatusCategory::Open, &file.open),
        (StatusCategory::Active, &file.active),
        (StatusCategory::Done, &file.done),
        (StatusCategory::Closed, &file.closed),
    ] {
        for value in values {
            if !field.values.contains(value) {
                return Err(invalid(format!(
                    "name '{}', which is not one of {:?}",
                    value, field.values
                )));
            }
            if categories.insert(value.clone(), category).is_some() {
                return Err(invalid(format!(
                    "put '{}' in more than one category",
                    value
                )));
            }
        }
    }
    categories.retain(|_, c| *c != StatusCategory::Open);
    Ok(Some(categories))
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
            let categories = parse_categories(&raw.name, &f)?;
            Ok(FieldDef {
                name: f.name,
                field_type: parse_field_type(&f.field_type, f.values),
                required: f.required,
                default: f.default,
                toggle,
                categories,
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

/// A schema as it is written to `schemas/<name>.yml`.
pub fn schema_to_yaml(schema: &Schema) -> Result<String, ParcError> {
    let file = SchemaFile {
        name: schema.name.clone(),
        alias: schema.alias.clone(),
        editor_skip: schema.editor_skip,
        fields: schema
            .fields
            .iter()
            .map(|f| {
                let values = match &f.field_type {
                    FieldType::Enum(values) => values.clone(),
                    _ => Vec::new(),
                };
                let categories = f.categories.as_ref().map(|map| {
                    let mut file = CategoriesFile::default();
                    // In the field's value order, so the file reads like the field.
                    for value in &values {
                        match map.get(value) {
                            Some(StatusCategory::Active) => file.active.push(value.clone()),
                            Some(StatusCategory::Done) => file.done.push(value.clone()),
                            Some(StatusCategory::Closed) => file.closed.push(value.clone()),
                            _ => {}
                        }
                    }
                    file
                });
                FieldDefFile {
                    name: f.name.clone(),
                    field_type: f.field_type.kind().to_string(),
                    values,
                    required: f.required,
                    default: f.default.clone(),
                    toggle: f
                        .toggle
                        .as_ref()
                        .map(|(off, on)| vec![off.clone(), on.clone()])
                        .unwrap_or_default(),
                    categories,
                }
            })
            .collect(),
    };
    Ok(serde_yaml_ng::to_string(&file)?)
}

pub struct SchemaRegistry {
    schemas: Vec<Schema>,
    paths: Vec<PathBuf>,
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

    /// The file a schema was loaded from.
    pub fn path_of(&self, name: &str) -> Option<&Path> {
        let i = self.schemas.iter().position(|s| s.name == name)?;
        Some(self.paths[i].as_path())
    }

    /// Status values that are settled (done or closed) in some type, for
    /// filters that span types such as "open work". Words the built-in types
    /// settle count too unless some schema here uses them as open, so
    /// fragments whose type has no schema still drop out.
    pub fn settled_statuses(&self) -> BTreeSet<String> {
        let mut settled = BTreeSet::new();
        let mut open = BTreeSet::new();
        for schema in &self.schemas {
            for (value, category) in schema.status_categories() {
                if category.is_settled() {
                    settled.insert(value);
                } else {
                    open.insert(value);
                }
            }
        }
        for word in FALLBACK_SETTLED {
            if !open.contains(*word) {
                settled.insert(word.to_string());
            }
        }
        settled
    }
}

/// Load all schemas from the vault's schemas/ directory.
pub fn load_schemas(vault_path: &Path) -> Result<SchemaRegistry, ParcError> {
    let schemas_dir = vault_path.join("schemas");
    let mut schemas = Vec::new();
    let mut paths = Vec::new();

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
            paths.push(entry.path());
        }
    }

    Ok(SchemaRegistry { schemas, paths })
}

/// The schema parc ships for a built-in type (note, todo, decision, risk, idea).
pub fn builtin_schema(name: &str) -> Option<Schema> {
    crate::vault::BUILTIN_SCHEMAS
        .iter()
        .map(|(_, yaml)| parse_schema(yaml).expect("built-in schemas parse"))
        .find(|s| s.name == name)
}

pub fn is_builtin(name: &str) -> bool {
    BUILTIN_TYPES.contains(&name)
}

/// The built-in types, in the order `parc init` offers them.
pub const BUILTIN_TYPES: &[&str] = &["note", "todo", "decision", "risk", "idea"];

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
    let content = std::fs::read_to_string(source_path)?;
    let schema = parse_schema(&content)?;
    // Loading is lenient (an unknown field type reads as string); adding is not.
    let raw: SchemaFile = serde_yaml_ng::from_str(&content)?;
    if let Some(field) = raw
        .fields
        .iter()
        .find(|f| !["string", "date", "enum", "list"].contains(&f.field_type.as_str()))
    {
        return Err(ParcError::ValidationError(format!(
            "type '{}': field '{}' has type '{}'; use string, date, enum or list",
            schema.name, field.name, field.field_type
        )));
    }
    validate_schema(&schema)?;
    let registry = load_schemas(vault_path)?;

    if registry.get_by_name(&schema.name).is_some() {
        return Err(ParcError::ValidationError(format!(
            "schema '{}' already exists in vault",
            schema.name
        )));
    }
    for other in registry.list() {
        if other.alias.as_deref() == Some(schema.name.as_str())
            || schema
                .alias
                .as_ref()
                .is_some_and(|a| a == &other.name || other.alias.as_ref() == Some(a))
        {
            return Err(ParcError::ValidationError(format!(
                "'{}' collides with the name or shortcut of type '{}'",
                schema.alias.as_deref().unwrap_or(&schema.name),
                other.name
            )));
        }
    }

    // Copy to schemas/
    let dest = vault_path
        .join("schemas")
        .join(format!("{}.yml", schema.name));
    secure_fs::copy_private_new(source_path, &dest)?;
    ensure_template(vault_path, &schema.name)?;

    Ok(schema.name)
}

/// Give a new type an empty body template unless it has one.
fn ensure_template(vault_path: &Path, name: &str) -> Result<(), ParcError> {
    let template_dir = vault_path.join("templates");
    let template_path = template_dir.join(format!("{}.md", name));
    if template_path.exists() {
        return Ok(());
    }
    if !template_dir.exists() {
        secure_fs::create_private_dir_all(&template_dir)?;
    }
    secure_fs::write_private_new(
        &template_path,
        format!("---\ntype: {}\ntitle: \"\"\n---\n\n", name),
    )
}

/// Frontmatter keys parc itself uses, and keys the JSON views of a fragment
/// put beside its fields. A schema field can't take one of these names.
pub const RESERVED_FIELDS: &[&str] = &[
    "id",
    "type",
    "title",
    "tags",
    "links",
    "attachments",
    "created_at",
    "updated_at",
    "created_by",
    "archived",
    "body",
    "revision",
    "backlinks",
    "attachment_files",
    "extra_fields",
    "fragment_type",
    "snippet",
    "warnings",
    "vault",
    "conflict",
];

const MAX_FIELDS: usize = 32;
const MAX_VALUES: usize = 32;

/// A field name: a letter or `_`, then letters, digits, `_` or `-`; at most 64.
fn is_field_name(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// An enum value: a word the search language can carry (`status:in-progress`).
fn is_value_word(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_alphanumeric())
        && s.chars().count() <= 64
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// Check a schema strictly before it is written. `parse_schema` accepts what
/// older hand-written files contain; a schema parc writes must also have
/// usable names, values and defaults.
pub fn validate_schema(schema: &Schema) -> Result<(), ParcError> {
    let invalid =
        |why: String| ParcError::ValidationError(format!("type '{}': {}", schema.name, why));
    validate_schema_name(&schema.name)?;
    if let Some(alias) = &schema.alias {
        let mut chars = alias.chars();
        let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
            && alias.len() <= 16
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !ok {
            return Err(invalid(format!(
                "shortcut '{}' must start with a letter and use only letters, digits, '_' or '-' (16 at most)",
                alias
            )));
        }
    }
    if schema.fields.len() > MAX_FIELDS {
        return Err(invalid(format!("at most {} fields", MAX_FIELDS)));
    }
    let mut names = BTreeSet::new();
    for field in &schema.fields {
        let name = &field.name;
        if !is_field_name(name) {
            return Err(invalid(format!(
                "field name '{}' must start with a letter or '_' and use only letters, digits, '_' or '-'",
                name
            )));
        }
        if RESERVED_FIELDS.contains(&name.to_ascii_lowercase().as_str()) {
            return Err(invalid(format!(
                "'{}' is used by every fragment and can't be a field name",
                name
            )));
        }
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(invalid(format!("two fields are named '{}'", name)));
        }
        let field_invalid = |why: String| invalid(format!("field '{}': {}", name, why));
        if let FieldType::Enum(values) = &field.field_type {
            if values.is_empty() {
                return Err(field_invalid(
                    "a choice field needs at least one value".into(),
                ));
            }
            if values.len() > MAX_VALUES {
                return Err(field_invalid(format!("at most {} values", MAX_VALUES)));
            }
            let mut seen = BTreeSet::new();
            for value in values {
                if !is_value_word(value) {
                    return Err(field_invalid(format!(
                        "value '{}' must be one word: letters, digits, '_' or '-'",
                        value
                    )));
                }
                if !seen.insert(value) {
                    return Err(field_invalid(format!("value '{}' is listed twice", value)));
                }
            }
        }
        if let Some(default) = &field.default {
            match &field.field_type {
                FieldType::Enum(values) if !values.contains(default) => {
                    return Err(field_invalid(format!(
                        "default '{}' is not one of its values",
                        default
                    )))
                }
                FieldType::Date
                    if chrono::NaiveDate::parse_from_str(default, "%Y-%m-%d").is_err() =>
                {
                    return Err(field_invalid(format!(
                        "default '{}' is not a YYYY-MM-DD date",
                        default
                    )))
                }
                FieldType::ListOfStrings => {
                    return Err(field_invalid("a list field can't have a default".into()))
                }
                FieldType::String if default.len() > 200 || default.contains('\n') => {
                    return Err(field_invalid(
                        "a default must be one line of at most 200 characters".into(),
                    ))
                }
                _ => {}
            }
        }
        if let Some((off, on)) = &field.toggle {
            let FieldType::Enum(values) = &field.field_type else {
                return Err(field_invalid("only a choice field can toggle".into()));
            };
            if off == on || !values.contains(off) || !values.contains(on) {
                return Err(field_invalid(
                    "a toggle needs two different values of the field".into(),
                ));
            }
        }
        if let Some(categories) = &field.categories {
            let FieldType::Enum(values) = &field.field_type else {
                return Err(field_invalid("only a choice field has categories".into()));
            };
            if name != "status" {
                return Err(field_invalid("only the status field has categories".into()));
            }
            if let Some(stray) = categories.keys().find(|v| !values.contains(v)) {
                return Err(field_invalid(format!(
                    "category given for '{}', which is not one of its values",
                    stray
                )));
            }
        }
    }
    Ok(())
}

/// Write a schema to the vault, after [`validate_schema`] and a check that
/// its name and shortcut don't collide with another type's. An existing
/// type is written over the file it was loaded from (`create: false`); a new
/// one goes to `schemas/<name>.yml` and gets an empty template.
///
/// Fragments are not touched: changing a type that has fragments goes
/// through [`crate::ops::Writer::save_schema`], which migrates them.
pub fn write_schema(vault: &Path, schema: &Schema, create: bool) -> Result<(), ParcError> {
    validate_schema(schema)?;
    let registry = load_schemas(vault)?;
    let exists = registry.get_by_name(&schema.name).is_some();
    if create && exists {
        return Err(ParcError::ValidationError(format!(
            "a type named '{}' already exists",
            schema.name
        )));
    }
    if !create && !exists {
        return Err(ParcError::SchemaNotFound(schema.name.clone()));
    }
    for other in registry
        .list()
        .into_iter()
        .filter(|s| s.name != schema.name)
    {
        if other.alias.as_deref() == Some(schema.name.as_str()) {
            return Err(ParcError::ValidationError(format!(
                "'{}' is already the shortcut of type '{}'",
                schema.name, other.name
            )));
        }
        if let Some(alias) = &schema.alias {
            if &other.name == alias || other.alias.as_ref() == Some(alias) {
                return Err(ParcError::ValidationError(format!(
                    "the shortcut '{}' is already taken by type '{}'",
                    alias, other.name
                )));
            }
        }
    }
    let path = match registry.path_of(&schema.name) {
        Some(path) => path.to_path_buf(),
        None => {
            let dir = vault.join("schemas");
            if !dir.exists() {
                secure_fs::create_private_dir_all(&dir)?;
            }
            dir.join(format!("{}.yml", schema.name))
        }
    };
    secure_fs::write_private_atomic(&path, schema_to_yaml(schema)?)?;
    if create {
        ensure_template(vault, &schema.name)?;
    }
    Ok(())
}

/// How a type is used: its fragments, and how many hold each value of each
/// of its choice fields (`values[field][value]`). Unreadable fragment files
/// are skipped; `parc doctor` reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TypeUsage {
    pub total: usize,
    pub values: BTreeMap<String, BTreeMap<String, usize>>,
}

pub fn type_usage(vault: &Path, name: &str) -> Result<TypeUsage, ParcError> {
    let registry = load_schemas(vault)?;
    let enum_fields: Vec<String> = registry
        .get_by_name(name)
        .map(|s| {
            s.fields
                .iter()
                .filter(|f| matches!(f.field_type, FieldType::Enum(_)))
                .map(|f| f.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut usage = TypeUsage::default();
    for fragment in crate::fragment::read_all(vault)? {
        if fragment.fragment_type != name {
            continue;
        }
        usage.total += 1;
        for field in &enum_fields {
            if let Some(value) = fragment.extra_fields.get(field).and_then(|v| v.as_str()) {
                *usage
                    .values
                    .entry(field.clone())
                    .or_default()
                    .entry(value.to_string())
                    .or_default() += 1;
            }
        }
    }
    Ok(usage)
}

/// Remove a type that no fragment uses. A type with fragments can be turned
/// off instead (`types.disabled` in config.yml), which keeps them readable.
pub fn delete_schema(vault: &Path, name: &str) -> Result<(), ParcError> {
    let registry = load_schemas(vault)?;
    let path = registry
        .path_of(name)
        .ok_or_else(|| ParcError::SchemaNotFound(name.to_string()))?
        .to_path_buf();
    let used = type_usage(vault, name)?.total;
    if used > 0 {
        return Err(ParcError::ValidationError(format!(
            "{} {} of type '{}': turn the type off instead, or change {} type first",
            used,
            if used == 1 {
                "fragment is"
            } else {
                "fragments are"
            },
            name,
            if used == 1 { "its" } else { "their" }
        )));
    }
    let mut types = crate::config::load_config(vault)?.types;
    if types.default.as_deref() == Some(name) {
        return Err(ParcError::ValidationError(format!(
            "'{}' is the type new captures start as: choose another first",
            name
        )));
    }
    std::fs::remove_file(&path)?;
    if types.disabled.iter().any(|t| t == name) {
        types.disabled.retain(|t| t != name);
        crate::config::save_types(vault, &types)?;
    }
    Ok(())
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

    #[test]
    fn test_builtin_categories() {
        let todo = parse_schema(include_str!("builtin/schemas/todo.yml")).unwrap();
        let cats: Vec<(String, &str)> = todo
            .status_categories()
            .into_iter()
            .map(|(v, c)| (v, c.as_str()))
            .collect();
        assert_eq!(
            cats,
            vec![
                ("open".to_string(), "open"),
                ("in-progress".to_string(), "active"),
                ("done".to_string(), "done"),
                ("cancelled".to_string(), "closed"),
            ]
        );
        let risk = parse_schema(include_str!("builtin/schemas/risk.yml")).unwrap();
        assert_eq!(risk.status_category("accepted"), StatusCategory::Closed);
        assert!(parse_schema(include_str!("builtin/schemas/note.yml"))
            .unwrap()
            .status_categories()
            .is_empty());
    }

    #[test]
    fn test_schema_without_categories_uses_the_builtin_words() {
        let yaml = "name: todo\nfields:\n  - name: status\n    type: enum\n    values: [open, in-progress, done, wontfix]\n";
        let schema = parse_schema(yaml).unwrap();
        assert_eq!(schema.status_category("done"), StatusCategory::Done);
        assert_eq!(
            schema.status_category("in-progress"),
            StatusCategory::Active
        );
        assert_eq!(schema.status_category("wontfix"), StatusCategory::Open);
    }

    #[test]
    fn test_categories_reject_bad_declarations() {
        let err = |yaml: &str| parse_schema(yaml).unwrap_err().to_string();
        let status = |cats: &str| {
            format!(
                "name: bug\nfields:\n  - name: status\n    type: enum\n    values: [new, fixed]\n    categories:\n{}",
                cats
            )
        };
        assert!(err(&status("      done: [shipped]\n")).contains("'shipped'"));
        assert!(err(&status("      done: [fixed]\n      closed: [fixed]\n"))
            .contains("more than one category"));
        assert!(parse_schema(&status("      finished: [fixed]\n")).is_err());
        let other = "name: bug\nfields:\n  - name: state\n    type: enum\n    values: [new, fixed]\n    categories:\n      done: [fixed]\n";
        assert!(err(other).contains("named status"));
    }

    #[test]
    fn test_schema_yaml_round_trips() {
        for (_, yaml) in crate::vault::BUILTIN_SCHEMAS {
            let schema = parse_schema(yaml).unwrap();
            let again = parse_schema(&schema_to_yaml(&schema).unwrap()).unwrap();
            assert_eq!(again.name, schema.name);
            assert_eq!(again.alias, schema.alias);
            assert_eq!(again.editor_skip, schema.editor_skip);
            assert_eq!(again.fields.len(), schema.fields.len());
            for (a, b) in again.fields.iter().zip(&schema.fields) {
                assert_eq!(a.name, b.name);
                assert_eq!(a.field_type, b.field_type);
                assert_eq!(a.required, b.required);
                assert_eq!(a.default, b.default);
                assert_eq!(a.toggle, b.toggle);
                assert_eq!(a.categories, b.categories);
            }
            validate_schema(&schema).unwrap();
        }
    }

    #[test]
    fn test_validate_schema_rejects_what_the_editor_must_not_write() {
        let check =
            |yaml: &str| validate_schema(&parse_schema(yaml).unwrap()).map_err(|e| e.to_string());
        let ok = "name: bug\nfields:\n  - name: status\n    type: enum\n    values: [new, fixed]\n    default: new\n";
        check(ok).unwrap();
        let bad = [
            ("name: bug\nfields:\n  - name: title\n    type: string\n", "can't be a field name"),
            ("name: bug\nfields:\n  - name: a\n    type: string\n  - name: A\n    type: date\n", "two fields"),
            ("name: bug\nfields:\n  - name: status\n    type: enum\n    values: []\n", "at least one value"),
            ("name: bug\nfields:\n  - name: status\n    type: enum\n    values: [won't fix]\n", "one word"),
            ("name: bug\nfields:\n  - name: status\n    type: enum\n    values: [a, a]\n", "twice"),
            ("name: bug\nfields:\n  - name: status\n    type: enum\n    values: [a]\n    default: b\n", "not one of its values"),
            ("name: bug\nfields:\n  - name: due\n    type: date\n    default: soon\n", "YYYY-MM-DD"),
            ("name: bug\nfields:\n  - name: owners\n    type: list\n    default: me\n", "can't have a default"),
            ("name: bug\nfields:\n  - name: status\n    type: enum\n    values: [a, b]\n    toggle: [a, a]\n", "two different values"),
            ("name: bug\nalias: 9\nfields: []\n", "shortcut"),
            ("name: ../x\nfields: []\n", "schema name"),
            ("name: bug\nfields:\n  - name: 1st\n    type: string\n", "field name"),
        ];
        for (yaml, why) in bad {
            let err = check(yaml).unwrap_err();
            assert!(err.contains(why), "{} → {}", yaml, err);
        }
    }

    #[test]
    fn test_write_schema_checks_collisions_and_keeps_the_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let bug = |alias: &str| {
            parse_schema(&format!("name: bug\nalias: {}\nfields: []\n", alias)).unwrap()
        };
        let err = write_schema(&vault, &bug("t"), true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("taken by type 'todo'"), "{}", err);
        let err = write_schema(&vault, &bug("todo"), true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("taken by type 'todo'"), "{}", err);
        write_schema(&vault, &bug("b"), true).unwrap();
        assert!(write_schema(&vault, &bug("b"), true).is_err());
        let missing = parse_schema("name: task\nfields: []\n").unwrap();
        assert!(matches!(
            write_schema(&vault, &missing, false),
            Err(ParcError::SchemaNotFound(_))
        ));

        // A type whose file isn't named after it is written back to that file.
        std::fs::rename(
            vault.join("schemas/todo.yml"),
            vault.join("schemas/tasks.yaml"),
        )
        .unwrap();
        let mut todo = load_schemas(&vault)
            .unwrap()
            .get_by_name("todo")
            .unwrap()
            .clone();
        todo.alias = Some("td".into());
        write_schema(&vault, &todo, false).unwrap();
        assert!(!vault.join("schemas/todo.yml").exists());
        assert_eq!(
            load_schemas(&vault).unwrap().resolve("td").unwrap().name,
            "todo"
        );
    }

    #[test]
    fn test_type_usage_and_delete_schema() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let config = crate::config::load_config(&vault).unwrap();
        let registry = load_schemas(&vault).unwrap();
        let mut todo = crate::fragment::new_fragment(
            "todo",
            "One",
            registry.resolve("todo").unwrap(),
            &config,
        );
        crate::fragment::create_fragment(&vault, &todo).unwrap();
        todo.id = crate::fragment::new_id();
        todo.extra_fields
            .insert("status".into(), serde_json::Value::String("done".into()));
        crate::fragment::create_fragment(&vault, &todo).unwrap();

        let usage = type_usage(&vault, "todo").unwrap();
        assert_eq!(usage.total, 2);
        assert_eq!(usage.values["status"]["open"], 1);
        assert_eq!(usage.values["status"]["done"], 1);
        assert_eq!(usage.values["priority"]["medium"], 2);

        let err = delete_schema(&vault, "todo").unwrap_err().to_string();
        assert!(err.contains("2 fragments are of type 'todo'"), "{}", err);
        let err = delete_schema(&vault, "note").unwrap_err().to_string();
        assert!(err.contains("new captures start as"), "{}", err);

        let mut types = crate::config::load_config(&vault).unwrap().types;
        types.disabled.push("idea".into());
        crate::config::save_types(&vault, &types).unwrap();
        delete_schema(&vault, "idea").unwrap();
        assert!(load_schemas(&vault).unwrap().get_by_name("idea").is_none());
        assert!(crate::config::load_config(&vault)
            .unwrap()
            .types
            .disabled
            .is_empty());
    }

    #[test]
    fn test_add_schema_checks_types_and_collisions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let file = tmp.path().join("x.yml");
        let add = |yaml: &str| {
            std::fs::write(&file, yaml).unwrap();
            add_schema(&vault, &file).map_err(|e| e.to_string())
        };
        assert!(
            add("name: meeting\nfields:\n  - name: minutes\n    type: integer\n")
                .unwrap_err()
                .contains("type 'integer'")
        );
        assert!(add("name: meeting\nalias: t\nfields: []\n")
            .unwrap_err()
            .contains("collides"));
        assert!(add("name: meeting\nfields:\n  - name: title\n    type: string\n").is_err());
        assert_eq!(
            add("name: meeting\nalias: m\nfields: []\n").unwrap(),
            "meeting"
        );
    }

    #[test]
    fn test_settled_statuses() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let settled = load_schemas(&vault).unwrap().settled_statuses();
        for word in [
            "done",
            "cancelled",
            "accepted",
            "resolved",
            "promoted",
            "parked",
        ] {
            assert!(settled.contains(word), "{}", word);
        }
        for word in ["open", "in-progress", "identified", "raw", "proposed"] {
            assert!(!settled.contains(word), "{}", word);
        }

        // A type that calls "done" open keeps it out, even though it's a built-in word.
        let odd = parse_schema("name: chore\nfields:\n  - name: status\n    type: enum\n    values: [done, shipped]\n    categories:\n      done: [shipped]\n").unwrap();
        write_schema(&vault, &odd, true).unwrap();
        let settled = load_schemas(&vault).unwrap().settled_statuses();
        assert!(settled.contains("shipped"));
        assert!(settled.contains("done"), "todo still settles it");
    }
}
