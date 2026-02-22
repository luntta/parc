use std::path::{Path, PathBuf};

use crate::error::ParcError;

/// Returns true if a valid vault exists at the given path.
pub fn is_vault(path: &Path) -> bool {
    path.join("config.yml").exists() && path.join("fragments").is_dir()
}

/// Walks up from `start_dir` looking for `.parc/`, falls back to `~/.parc`.
pub fn discover_vault_from(start_dir: &Path) -> Result<PathBuf, ParcError> {
    let mut current = start_dir.to_path_buf();
    loop {
        let candidate = current.join(".parc");
        if is_vault(&candidate) {
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }

    // Fall back to global vault
    let global = global_vault_path()?;
    if is_vault(&global) {
        return Ok(global);
    }

    Err(ParcError::VaultNotFound(start_dir.to_path_buf()))
}

/// Discover vault from CWD.
pub fn discover_vault() -> Result<PathBuf, ParcError> {
    let cwd = std::env::current_dir()?;
    discover_vault_from(&cwd)
}

/// Returns the default global vault path (~/.parc).
pub fn global_vault_path() -> Result<PathBuf, ParcError> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| {
            ParcError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "HOME directory not found",
            ))
        })?;
    Ok(PathBuf::from(home).join(".parc"))
}

/// Creates a new vault at the given path with the default directory structure.
pub fn init_vault(path: &Path) -> Result<(), ParcError> {
    if is_vault(path) {
        return Err(ParcError::VaultAlreadyExists(path.to_path_buf()));
    }

    // Create directory structure
    std::fs::create_dir_all(path.join("schemas"))?;
    std::fs::create_dir_all(path.join("templates"))?;
    std::fs::create_dir_all(path.join("fragments"))?;
    std::fs::create_dir_all(path.join("attachments"))?;
    std::fs::create_dir_all(path.join("history"))?;
    std::fs::create_dir_all(path.join("trash"))?;
    std::fs::create_dir_all(path.join("plugins"))?;
    std::fs::create_dir_all(path.join("hooks"))?;

    // Write default config
    std::fs::write(path.join("config.yml"), DEFAULT_CONFIG)?;

    // Write built-in schemas
    for (name, content) in BUILTIN_SCHEMAS {
        std::fs::write(path.join("schemas").join(name), content)?;
    }

    // Write built-in templates
    for (name, content) in BUILTIN_TEMPLATES {
        std::fs::write(path.join("templates").join(name), content)?;
    }

    Ok(())
}

const DEFAULT_CONFIG: &str = include_str!("builtin/config.yml");

pub const BUILTIN_SCHEMAS: &[(&str, &str)] = &[
    ("note.yml", include_str!("builtin/schemas/note.yml")),
    ("todo.yml", include_str!("builtin/schemas/todo.yml")),
    (
        "decision.yml",
        include_str!("builtin/schemas/decision.yml"),
    ),
    ("risk.yml", include_str!("builtin/schemas/risk.yml")),
    ("idea.yml", include_str!("builtin/schemas/idea.yml")),
];

const BUILTIN_TEMPLATES: &[(&str, &str)] = &[
    ("note.md", include_str!("builtin/templates/note.md")),
    ("todo.md", include_str!("builtin/templates/todo.md")),
    (
        "decision.md",
        include_str!("builtin/templates/decision.md"),
    ),
    ("risk.md", include_str!("builtin/templates/risk.md")),
    ("idea.md", include_str!("builtin/templates/idea.md")),
];

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_vault() {
        let tmp = TempDir::new().unwrap();
        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        assert!(is_vault(&vault_path));
        assert!(vault_path.join("schemas/note.yml").exists());
        assert!(vault_path.join("schemas/todo.yml").exists());
        assert!(vault_path.join("templates/note.md").exists());
        assert!(vault_path.join("fragments").is_dir());
        assert!(vault_path.join("config.yml").exists());
    }

    #[test]
    fn test_init_vault_already_exists() {
        let tmp = TempDir::new().unwrap();
        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        let result = init_vault(&vault_path);
        assert!(matches!(result, Err(ParcError::VaultAlreadyExists(_))));
    }

    #[test]
    fn test_discover_vault_local() {
        let tmp = TempDir::new().unwrap();
        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        let subdir = tmp.path().join("sub/deep");
        std::fs::create_dir_all(&subdir).unwrap();

        let discovered = discover_vault_from(&subdir).unwrap();
        assert_eq!(discovered, vault_path);
    }
}
