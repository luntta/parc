use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::ParcError;

#[derive(Debug, Clone, PartialEq)]
pub enum DateFormat {
    Relative,
    Iso,
    Short,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub user: Option<String>,
    pub editor: Option<String>,
    pub default_tags: Vec<String>,
    pub date_format: DateFormat,
    pub id_display_length: usize,
    pub color: ColorMode,
    pub aliases: BTreeMap<String, String>,
    pub history_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        let mut aliases = BTreeMap::new();
        aliases.insert("n".to_string(), "note".to_string());
        aliases.insert("t".to_string(), "todo".to_string());
        aliases.insert("d".to_string(), "decision".to_string());
        aliases.insert("r".to_string(), "risk".to_string());
        aliases.insert("i".to_string(), "idea".to_string());

        Config {
            user: None,
            editor: None,
            default_tags: Vec::new(),
            date_format: DateFormat::Relative,
            id_display_length: 8,
            color: ColorMode::Auto,
            aliases,
            history_enabled: true,
        }
    }
}

#[derive(Deserialize, Default)]
struct HistoryConfig {
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Default)]
struct ConfigFile {
    user: Option<String>,
    editor: Option<String>,
    #[serde(default)]
    default_tags: Vec<String>,
    #[serde(default = "default_date_format")]
    date_format: String,
    #[serde(default = "default_id_display_length")]
    id_display_length: usize,
    #[serde(default = "default_color")]
    color: String,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    history: Option<HistoryConfig>,
}

fn default_date_format() -> String {
    "relative".to_string()
}
fn default_id_display_length() -> usize {
    8
}
fn default_color() -> String {
    "auto".to_string()
}

/// Load config from the vault's config.yml. Missing file uses defaults.
pub fn load_config(vault: &Path) -> Result<Config, ParcError> {
    let config_path = vault.join("config.yml");
    if !config_path.exists() {
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(&config_path)?;
    let raw: ConfigFile = serde_yaml::from_str(&content)?;

    let mut config = Config::default();

    if raw.user.is_some() {
        config.user = raw.user;
    }
    if raw.editor.is_some() {
        config.editor = raw.editor;
    }
    if !raw.default_tags.is_empty() {
        config.default_tags = raw.default_tags;
    }
    config.date_format = match raw.date_format.as_str() {
        "iso" => DateFormat::Iso,
        "short" => DateFormat::Short,
        _ => DateFormat::Relative,
    };
    config.id_display_length = raw.id_display_length;
    config.color = match raw.color.as_str() {
        "always" => ColorMode::Always,
        "never" => ColorMode::Never,
        _ => ColorMode::Auto,
    };
    if !raw.aliases.is_empty() {
        config.aliases = raw.aliases;
    }
    if let Some(history) = raw.history {
        config.history_enabled = history.enabled;
    }

    Ok(config)
}

/// Get the editor command: config > $EDITOR > vim.
pub fn get_editor(config: &Config) -> String {
    config
        .editor
        .clone()
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vim".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.id_display_length, 8);
        assert_eq!(config.date_format, DateFormat::Relative);
        assert_eq!(config.aliases.get("t").unwrap(), "todo");
    }

    #[test]
    fn test_load_config_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.id_display_length, 8);
    }

    #[test]
    fn test_load_config_from_vault() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let config = load_config(&vault).unwrap();
        assert_eq!(config.id_display_length, 8);
        assert_eq!(config.aliases.get("n").unwrap(), "note");
    }
}
