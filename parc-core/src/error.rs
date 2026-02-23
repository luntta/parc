use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ParcError {
    #[error("vault not found: {0}")]
    VaultNotFound(PathBuf),

    #[error("vault already exists: {0}")]
    VaultAlreadyExists(PathBuf),

    #[error("fragment not found: {0}")]
    FragmentNotFound(String),

    #[error("ambiguous ID prefix '{0}': matches {1} fragments")]
    AmbiguousId(String, usize),

    #[error("schema not found for type: {0}")]
    SchemaNotFound(String),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("index error: {0}")]
    IndexError(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("plugin error: {0}")]
    PluginError(String),
}
