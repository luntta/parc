use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ParcError;
use crate::schema::{Schema, SchemaRegistry};
use crate::secure_fs;

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
    /// `editor:` from the global vault's config.yml. Other vaults can't set
    /// it: see [`get_editor`].
    pub editor: Option<String>,
    /// `editor:` from a vault that isn't the global one, which parc ignores
    /// and frontends mention, so the setting doesn't fail silently.
    pub ignored_editor: Option<String>,
    pub default_tags: Vec<String>,
    pub date_format: DateFormat,
    pub id_display_length: usize,
    pub color: ColorMode,
    pub aliases: BTreeMap<String, String>,
    pub history_enabled: bool,
    pub server: ServerConfig,
    pub hooks: ScriptHooksConfig,
    pub resurfacing: ResurfacingConfig,
    pub types: TypesConfig,
    pub attachments: AttachmentsConfig,
    pub plugins: HashMap<String, serde_yaml_ng::Value>,
}

/// Where extracted attachment text is kept (`attachments:` in config.yml).
/// - `sidecar` (default): writes `attachments/<id>/.text/<filename>.txt|.md`
///   beside the file so it travels over git and other machines need not re-extract.
/// - `index`: keeps extracted text only in `index.db` (`attachment_text`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentTextMode {
    #[default]
    Sidecar,
    Index,
}

impl AttachmentTextMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentTextMode::Sidecar => "sidecar",
            AttachmentTextMode::Index => "index",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sidecar" => Some(AttachmentTextMode::Sidecar),
            "index" => Some(AttachmentTextMode::Index),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentsConfig {
    #[serde(default)]
    pub text: AttachmentTextMode,
}

/// Which types new fragments can use (`types:` in config.yml). Turning a type
/// off keeps its schema and its fragments: they open, validate and change as
/// before, but nothing new is created as that type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypesConfig {
    /// The type a capture starts as when it doesn't pick one. Unset: note,
    /// or the first type that is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl TypesConfig {
    pub fn is_enabled(&self, name: &str) -> bool {
        !self.disabled.iter().any(|t| t == name)
    }

    /// The type captures start as: `default` when it exists and is on, else
    /// note when it is on, else the first type that is on.
    pub fn default_type<'a>(&self, registry: &'a SchemaRegistry) -> Option<&'a Schema> {
        let usable = |name: &str| {
            registry
                .get_by_name(name)
                .filter(|s| self.is_enabled(&s.name))
        };
        self.default
            .as_deref()
            .and_then(usable)
            .or_else(|| usable("note"))
            .or_else(|| {
                registry
                    .list()
                    .into_iter()
                    .find(|s| self.is_enabled(&s.name))
            })
    }

    /// Refuse a fragment of a type that is turned off. Used when a fragment
    /// is created or converted, never when one is edited.
    pub fn check_enabled(&self, name: &str) -> Result<(), ParcError> {
        if self.is_enabled(name) {
            Ok(())
        } else {
            Err(ParcError::ValidationError(format!(
                "the type '{}' is turned off in this vault; turn it on in vault settings, or remove it from types.disabled in config.yml",
                name
            )))
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub transport: String,
    pub socket_path: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            transport: "stdio".to_string(),
            socket_path: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScriptHooksConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ResurfacingConfig {
    pub stale_days: u64,
    pub review_window: String,
    pub today_section_limit: usize,
}

impl Default for ResurfacingConfig {
    fn default() -> Self {
        ResurfacingConfig {
            stale_days: 30,
            review_window: "this-week".to_string(),
            today_section_limit: 10,
        }
    }
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
            ignored_editor: None,
            default_tags: Vec::new(),
            date_format: DateFormat::Relative,
            id_display_length: 8,
            color: ColorMode::Auto,
            aliases,
            history_enabled: true,
            server: ServerConfig::default(),
            hooks: ScriptHooksConfig::default(),
            resurfacing: ResurfacingConfig::default(),
            types: TypesConfig::default(),
            attachments: AttachmentsConfig::default(),
            plugins: HashMap::new(),
        }
    }
}

#[derive(Deserialize, Default)]
struct HistoryConfig {
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Deserialize)]
struct ServerConfigFile {
    #[serde(default = "default_transport")]
    transport: String,
    socket_path: Option<String>,
}

#[derive(Deserialize, Default)]
struct ScriptHooksConfigFile {
    #[serde(default)]
    enabled: bool,
}

#[derive(Deserialize, Default)]
struct ResurfacingConfigFile {
    stale_days: Option<u64>,
    review_window: Option<String>,
    today_section_limit: Option<usize>,
}

impl Default for ServerConfigFile {
    fn default() -> Self {
        ServerConfigFile {
            transport: "stdio".to_string(),
            socket_path: None,
        }
    }
}

fn default_transport() -> String {
    "stdio".to_string()
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
    #[serde(default)]
    server: Option<ServerConfigFile>,
    #[serde(default)]
    hooks: Option<ScriptHooksConfigFile>,
    #[serde(default)]
    resurfacing: Option<ResurfacingConfigFile>,
    #[serde(default)]
    types: Option<TypesConfig>,
    #[serde(default)]
    attachments: Option<AttachmentsConfig>,
    #[serde(default)]
    plugins: HashMap<String, serde_yaml_ng::Value>,
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
    load_config_as(vault, is_global_vault(vault))
}

/// Whether `vault` is the global vault (`~/.parc`), the user's own, as
/// opposed to one that may have arrived with a cloned repository.
pub fn is_global_vault(vault: &Path) -> bool {
    let Ok(global) = crate::vault::global_vault_path() else {
        return false;
    };
    match (std::fs::canonicalize(vault), std::fs::canonicalize(&global)) {
        (Ok(a), Ok(b)) => a == b,
        _ => vault == global,
    }
}

/// [`load_config`] with the trust decision made by the caller: `personal`
/// is true for the global vault, whose `editor:` setting is honoured.
fn load_config_as(vault: &Path, personal: bool) -> Result<Config, ParcError> {
    let config_path = vault.join("config.yml");
    if !config_path.exists() {
        return Ok(Config::default());
    }

    let content = std::fs::read_to_string(&config_path)?;
    let raw: ConfigFile = serde_yaml_ng::from_str(&content)?;

    let mut config = Config::default();

    if raw.user.is_some() {
        config.user = raw.user;
    }
    // A vault's config.yml travels with it (git is the transport), so a
    // project vault naming the program parc runs would let any cloned
    // repository run code on `parc edit`. Only the user's global vault may.
    if personal {
        config.editor = raw.editor;
    } else {
        config.ignored_editor = raw.editor;
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
    let approved = (raw.server.is_some() || raw.hooks.as_ref().is_some_and(|h| h.enabled))
        && crate::trust::approved_config(vault, content.as_bytes());
    if let Some(server) = raw.server.filter(|_| approved) {
        config.server = ServerConfig {
            transport: server.transport,
            socket_path: server.socket_path,
        };
    }
    if let Some(hooks) = raw.hooks {
        config.hooks.enabled = hooks.enabled && approved;
    }
    if let Some(resurfacing) = raw.resurfacing {
        if let Some(stale_days) = resurfacing.stale_days {
            config.resurfacing.stale_days = stale_days;
        }
        if let Some(review_window) = resurfacing.review_window {
            config.resurfacing.review_window = review_window;
        }
        if let Some(today_section_limit) = resurfacing.today_section_limit {
            config.resurfacing.today_section_limit = today_section_limit;
        }
    }
    if let Some(types) = raw.types {
        config.types = types;
    }
    if let Some(attachments) = raw.attachments {
        config.attachments = attachments;
    }
    config.plugins = raw.plugins;

    Ok(config)
}

/// Save the `attachments:` block in `config.yml`, rewriting only that block
/// and keeping the rest of the file (comments included) intact.
pub fn save_attachments(vault: &Path, attachments: &AttachmentsConfig) -> Result<(), ParcError> {
    let path = vault.join("config.yml");
    let current = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let next = replace_top_level_block(&current, "attachments", &render_attachments(attachments)?);
    serde_yaml_ng::from_str::<ConfigFile>(&next)?;
    secure_fs::write_private_atomic(&path, next)
}

/// The `attachments:` block as config.yml holds it.
pub fn render_attachments(attachments: &AttachmentsConfig) -> Result<String, ParcError> {
    #[derive(Serialize)]
    struct Block<'a> {
        attachments: &'a AttachmentsConfig,
    }
    Ok(serde_yaml_ng::to_string(&Block { attachments })?)
}

/// Save which types are on and the default capture type. Checks that the
/// default is a type that exists and is on, and that some type stays on.
/// Only the `types:` block of config.yml is rewritten; the rest of the file,
/// comments included, is kept as it is.
pub fn save_types(vault: &Path, types: &TypesConfig) -> Result<(), ParcError> {
    let registry = crate::schema::load_schemas(vault)?;
    let mut types = types.clone();
    let mut seen = std::collections::BTreeSet::new();
    types.disabled.retain(|t| seen.insert(t.clone()));
    for name in &types.disabled {
        if registry.get_by_name(name).is_none() {
            return Err(ParcError::SchemaNotFound(name.clone()));
        }
    }
    if let Some(default) = &types.default {
        if registry.get_by_name(default).is_none() {
            return Err(ParcError::SchemaNotFound(default.clone()));
        }
        if !types.is_enabled(default) {
            return Err(ParcError::ValidationError(format!(
                "'{}' is the type new captures start as, so it can't be turned off: choose another default first",
                default
            )));
        }
    }
    if !registry.list().is_empty() && types.default_type(&registry).is_none() {
        return Err(ParcError::ValidationError(
            "at least one type has to stay on".into(),
        ));
    }

    let path = vault.join("config.yml");
    let current = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let next = replace_top_level_block(&current, "types", &render_types(&types)?);
    // Never write a file the loader would refuse.
    serde_yaml_ng::from_str::<ConfigFile>(&next)?;
    secure_fs::write_private_atomic(&path, next)
}

/// The `types:` block as config.yml holds it.
pub fn render_types(types: &TypesConfig) -> Result<String, ParcError> {
    #[derive(Serialize)]
    struct Block<'a> {
        types: &'a TypesConfig,
    }
    Ok(serde_yaml_ng::to_string(&Block { types })?)
}

/// Replace the top-level `key:` block of a block-style YAML document (the key
/// line and every indented, blank or comment line under it) with `block`, or
/// append `block` when the key is absent. Text outside the block is kept
/// byte for byte.
fn replace_top_level_block(doc: &str, key: &str, block: &str) -> String {
    let lines: Vec<&str> = doc.split_inclusive('\n').collect();
    let is_key = |line: &str| {
        line.strip_prefix(key)
            .is_some_and(|rest| rest.starts_with(':'))
    };
    let Some(start) = lines.iter().position(|l| is_key(l)) else {
        let mut out = doc.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(block);
        return out;
    };
    let mut end = start + 1;
    while end < lines.len() {
        let line = lines[end];
        let continues = line.starts_with(' ') || line.starts_with('\t') || line.trim().is_empty();
        if !continues {
            break;
        }
        end += 1;
    }
    // Blank lines after the block belong to what follows it.
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut out: String = lines[..start].concat();
    out.push_str(block);
    out.push_str(&lines[end..].concat());
    out
}

/// The editor command: `editor:` in the global vault's config.yml
/// (`~/.parc`), else `$EDITOR`, else vim. A project vault's own `editor:` is
/// never used (see [`Config::ignored_editor`]): config.yml travels with the
/// vault, and the editor is a program parc runs.
pub fn get_editor(config: &Config) -> String {
    let set = |v: Option<String>| v.filter(|e| !e.trim().is_empty());
    set(config.editor.clone())
        .or_else(|| set(global_editor()))
        .or_else(|| set(std::env::var("EDITOR").ok()))
        .unwrap_or_else(|| "vim".to_string())
}

/// `editor:` in the global vault's config.yml, when there is one.
fn global_editor() -> Option<String> {
    let path = crate::vault::global_vault_path().ok()?.join("config.yml");
    let text = std::fs::read_to_string(path).ok()?;
    serde_yaml_ng::from_str::<ConfigFile>(&text).ok()?.editor
}

/// The editor as a program and its arguments, ready to run with the file to
/// edit appended: `EDITOR="code --wait"` is `code` with `--wait`. See
/// [`split_command`].
pub fn editor_command(config: &Config) -> Result<Vec<String>, ParcError> {
    split_command(&get_editor(config))
}

/// Split a command line into words the way a shell would, without running
/// one: whitespace separates words, '…' and "…" group them, and outside
/// single quotes a backslash escapes the next character (not on Windows,
/// where it separates paths). Nothing is expanded, so `$VAR`, `;` and `|`
/// are just characters. A value that names an existing file is one word,
/// so an unquoted path with spaces still works.
pub fn split_command(command: &str) -> Result<Vec<String>, ParcError> {
    let command = command.trim();
    if command.is_empty() {
        return Err(ParcError::ValidationError(
            "the editor command is empty".into(),
        ));
    }
    if Path::new(command).is_file() {
        return Ok(vec![command.to_string()]);
    }
    let escapes = !cfg!(windows);
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(unclosed(command)),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if escapes => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(unclosed(command)),
                        },
                        Some(c) => word.push(c),
                        None => return Err(unclosed(command)),
                    }
                }
            }
            '\\' if escapes => {
                in_word = true;
                if let Some(c) = chars.next() {
                    word.push(c);
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

fn unclosed(command: &str) -> ParcError {
    ParcError::ValidationError(format!(
        "the editor command has an unclosed quote: {}",
        command
    ))
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
        assert!(!config.hooks.enabled);
        assert_eq!(config.resurfacing.stale_days, 30);
        assert_eq!(config.resurfacing.today_section_limit, 10);
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

    #[test]
    fn test_load_config_hooks_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("config.yml"), "hooks:\n  enabled: true\n").unwrap();

        let config = load_config(tmp.path()).unwrap();

        assert!(!config.hooks.enabled);
        crate::trust::grant(tmp.path()).unwrap();
        assert!(load_config(tmp.path()).unwrap().hooks.enabled);
    }

    #[test]
    fn test_only_the_global_vault_chooses_the_editor() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("config.yml"), "editor: /bin/sh\n").unwrap();

        let project = load_config_as(tmp.path(), false).unwrap();
        assert_eq!(project.editor, None);
        assert_eq!(project.ignored_editor.as_deref(), Some("/bin/sh"));
        assert_ne!(get_editor(&project), "/bin/sh");

        let global = load_config_as(tmp.path(), true).unwrap();
        assert_eq!(global.editor.as_deref(), Some("/bin/sh"));
        assert_eq!(global.ignored_editor, None);
        assert_eq!(get_editor(&global), "/bin/sh");

        assert!(!is_global_vault(tmp.path()));
    }

    #[test]
    fn test_split_command() {
        let split = |s: &str| split_command(s).unwrap();
        assert_eq!(split("vim"), vec!["vim"]);
        assert_eq!(
            split("  omarchy-launch-editor --inline "),
            vec!["omarchy-launch-editor", "--inline"]
        );
        assert_eq!(split("code --wait"), vec!["code", "--wait"]);
        assert_eq!(
            split("'/opt/My Editor/ed' -f"),
            vec!["/opt/My Editor/ed", "-f"]
        );
        assert_eq!(
            split("\"/opt/My Editor/ed\" -f"),
            vec!["/opt/My Editor/ed", "-f"]
        );
        assert_eq!(
            split("emacs -nw --eval '(setq x \"y\")'"),
            vec!["emacs", "-nw", "--eval", "(setq x \"y\")"]
        );
        assert_eq!(split("ed '' x"), vec!["ed", "", "x"]);
        // Nothing is expanded or run: shell syntax is just text.
        assert_eq!(
            split("vim; rm -rf $HOME"),
            vec!["vim;", "rm", "-rf", "$HOME"]
        );
        #[cfg(unix)]
        assert_eq!(split("/opt/My\\ Editor/ed"), vec!["/opt/My Editor/ed"]);
        assert!(split_command("vim 'oops").is_err());
        assert!(split_command("   ").is_err());

        // An existing file is one word, spaces and all.
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("my editor");
        std::fs::write(&path, "").unwrap();
        let path = path.to_str().unwrap();
        assert_eq!(split(path), vec![path]);
    }

    #[test]
    fn test_types_default_and_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let registry = crate::schema::load_schemas(&vault).unwrap();
        let config = load_config(&vault).unwrap();
        assert_eq!(config.types.default.as_deref(), Some("note"));
        assert!(config.types.disabled.is_empty());

        let types = TypesConfig {
            default: Some("todo".into()),
            disabled: vec!["note".into()],
        };
        assert_eq!(types.default_type(&registry).unwrap().name, "todo");
        assert!(!types.is_enabled("note"));
        let fallback = TypesConfig {
            default: None,
            disabled: vec!["note".into()],
        };
        // Note is off, so the first type that is on (schemas load in file order).
        assert_eq!(fallback.default_type(&registry).unwrap().name, "decision");
    }

    #[test]
    fn test_save_types_keeps_the_rest_of_the_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let before = std::fs::read_to_string(vault.join("config.yml")).unwrap();
        let types = TypesConfig {
            default: Some("todo".into()),
            disabled: vec!["risk".into(), "idea".into(), "risk".into()],
        };
        save_types(&vault, &types).unwrap();
        let after = std::fs::read_to_string(vault.join("config.yml")).unwrap();
        assert!(after.starts_with("# parc vault configuration\n# user: alice\n"));
        assert!(after.contains("resurfacing:\n  stale_days: 30\n"));
        assert_eq!(
            before.split("types:").next().unwrap(),
            after.split("types:").next().unwrap()
        );
        let config = load_config(&vault).unwrap();
        assert_eq!(config.types.default.as_deref(), Some("todo"));
        assert_eq!(config.types.disabled, vec!["risk", "idea"]);
    }

    #[test]
    fn test_save_types_refuses_impossible_choices() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let save = |default: Option<&str>, disabled: &[&str]| {
            save_types(
                &vault,
                &TypesConfig {
                    default: default.map(String::from),
                    disabled: disabled.iter().map(|s| s.to_string()).collect(),
                },
            )
            .map_err(|e| e.to_string())
        };
        assert!(save(Some("note"), &["note"])
            .unwrap_err()
            .contains("can't be turned off"));
        assert!(save(Some("bug"), &[]).is_err());
        assert!(save(None, &["bug"]).is_err());
        let all = ["note", "todo", "decision", "risk", "idea", "source"];
        assert!(save(None, &all).unwrap_err().contains("stay on"));
        save(None, &["note"]).unwrap();
    }

    #[test]
    fn test_replace_top_level_block() {
        let block = "types:\n  default: note\n";
        assert_eq!(
            replace_top_level_block("a: 1\n", "types", block),
            "a: 1\ntypes:\n  default: note\n"
        );
        assert_eq!(
            replace_top_level_block("a: 1", "types", block),
            "a: 1\ntypes:\n  default: note\n"
        );
        assert_eq!(
            replace_top_level_block(
                "a: 1\ntypes:\n  # old\n  disabled: [x]\n\n# next\nb: 2\n",
                "types",
                block
            ),
            "a: 1\ntypes:\n  default: note\n\n# next\nb: 2\n"
        );
        assert_eq!(
            replace_top_level_block("types: {disabled: [x]}\nb: 2\n", "types", block),
            "types:\n  default: note\nb: 2\n"
        );
        // A key that merely starts with the name is someone else's.
        assert_eq!(
            replace_top_level_block("types_extra: 1\n", "types", block),
            "types_extra: 1\ntypes:\n  default: note\n"
        );
    }
}
