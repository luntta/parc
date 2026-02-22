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

/// Resolve the active vault using the priority chain:
/// explicit path > PARC_VAULT env > local discovery (CWD walk-up) > global ~/.parc
pub fn resolve_vault(explicit: Option<&Path>) -> Result<PathBuf, ParcError> {
    // 1. Explicit --vault flag (highest priority)
    if let Some(path) = explicit {
        return resolve_vault_path(path);
    }

    // 2. PARC_VAULT environment variable
    if let Ok(env_val) = std::env::var("PARC_VAULT") {
        if !env_val.is_empty() {
            return resolve_vault_path(Path::new(&env_val));
        }
    }

    // 3. Local discovery (walk up from CWD) + 4. Global fallback
    discover_vault()
}

/// Resolve a vault path: if it ends with `.parc`, use directly; otherwise append `.parc`.
/// Returns VaultNotFound if the resolved path is not a valid vault.
fn resolve_vault_path(path: &Path) -> Result<PathBuf, ParcError> {
    let vault_path = if path.ends_with(".parc") {
        path.to_path_buf()
    } else {
        path.join(".parc")
    };

    if is_vault(&vault_path) {
        Ok(vault_path)
    } else {
        Err(ParcError::VaultNotFound(vault_path))
    }
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultScope {
    Local,
    Global,
}

impl std::fmt::Display for VaultScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultScope::Local => write!(f, "local"),
            VaultScope::Global => write!(f, "global"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VaultInfo {
    pub path: PathBuf,
    pub scope: VaultScope,
    pub fragment_count: usize,
}

/// Returns metadata about a vault: path, scope, and fragment count.
pub fn vault_info(vault_path: &Path) -> Result<VaultInfo, ParcError> {
    if !is_vault(vault_path) {
        return Err(ParcError::VaultNotFound(vault_path.to_path_buf()));
    }

    let scope = match global_vault_path() {
        Ok(global) if vault_path == global.as_path() => VaultScope::Global,
        _ => VaultScope::Local,
    };

    let fragments_dir = vault_path.join("fragments");
    let fragment_count = if fragments_dir.is_dir() {
        std::fs::read_dir(&fragments_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "md")
            })
            .count()
    } else {
        0
    };

    Ok(VaultInfo {
        path: vault_path.to_path_buf(),
        scope,
        fragment_count,
    })
}

/// Discover all known vaults: global (if exists) + local (if found from CWD).
/// Does not maintain a persistent registry -- just checks known locations.
pub fn discover_all_vaults() -> Result<Vec<VaultInfo>, ParcError> {
    let mut vaults = Vec::new();

    // Check for local vault by walking up from CWD
    let cwd = std::env::current_dir()?;
    let mut current = cwd.clone();
    let mut local_vault: Option<PathBuf> = None;
    loop {
        let candidate = current.join(".parc");
        if is_vault(&candidate) {
            local_vault = Some(candidate);
            break;
        }
        if !current.pop() {
            break;
        }
    }

    // Check global vault
    let global_path = global_vault_path()?;
    let global_exists = is_vault(&global_path);

    // Add local vault first (if it exists and is different from global)
    if let Some(ref local) = local_vault {
        if !global_exists || local != &global_path {
            vaults.push(vault_info(local)?);
        }
    }

    // Add global vault
    if global_exists {
        vaults.push(vault_info(&global_path)?);
    }

    Ok(vaults)
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

    #[test]
    fn test_resolve_vault_explicit() {
        let tmp = TempDir::new().unwrap();
        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        // Explicit with .parc suffix
        let resolved = resolve_vault(Some(&vault_path)).unwrap();
        assert_eq!(resolved, vault_path);

        // Explicit without .parc suffix — appends .parc
        let resolved = resolve_vault(Some(tmp.path())).unwrap();
        assert_eq!(resolved, vault_path);
    }

    #[test]
    fn test_resolve_vault_invalid_explicit() {
        let tmp = TempDir::new().unwrap();
        let result = resolve_vault(Some(&tmp.path().join("nonexistent")));
        assert!(matches!(result, Err(ParcError::VaultNotFound(_))));
    }

    #[test]
    fn test_resolve_vault_env() {
        let tmp = TempDir::new().unwrap();
        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        // Set PARC_VAULT and test (no explicit)
        std::env::set_var("PARC_VAULT", vault_path.to_str().unwrap());
        let resolved = resolve_vault(None).unwrap();
        assert_eq!(resolved, vault_path);
        std::env::remove_var("PARC_VAULT");
    }

    #[test]
    fn test_resolve_vault_explicit_over_env() {
        let tmp1 = TempDir::new().unwrap();
        let vault1 = tmp1.path().join(".parc");
        init_vault(&vault1).unwrap();

        let tmp2 = TempDir::new().unwrap();
        let vault2 = tmp2.path().join(".parc");
        init_vault(&vault2).unwrap();

        // Set env to vault2, explicit to vault1 — explicit wins
        std::env::set_var("PARC_VAULT", vault2.to_str().unwrap());
        let resolved = resolve_vault(Some(&vault1)).unwrap();
        assert_eq!(resolved, vault1);
        std::env::remove_var("PARC_VAULT");
    }

    #[test]
    fn test_vault_info_fragment_count() {
        let tmp = TempDir::new().unwrap();
        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        // Empty vault
        let info = vault_info(&vault_path).unwrap();
        assert_eq!(info.fragment_count, 0);
        assert_eq!(info.scope, VaultScope::Local);

        // Add a fragment file
        std::fs::write(vault_path.join("fragments/test.md"), "---\n---\n").unwrap();
        let info = vault_info(&vault_path).unwrap();
        assert_eq!(info.fragment_count, 1);
    }

    #[test]
    fn test_vault_info_not_found() {
        let tmp = TempDir::new().unwrap();
        let result = vault_info(&tmp.path().join("nonexistent"));
        assert!(matches!(result, Err(ParcError::VaultNotFound(_))));
    }

    #[test]
    fn test_discover_all_vaults_with_global() {
        let tmp = TempDir::new().unwrap();
        // Create a "global" vault by setting HOME
        let home_dir = tmp.path().join("home");
        std::fs::create_dir_all(&home_dir).unwrap();
        let global_path = home_dir.join(".parc");
        init_vault(&global_path).unwrap();

        std::env::set_var("HOME", home_dir.to_str().unwrap());
        let vaults = discover_all_vaults().unwrap();
        std::env::remove_var("HOME");

        assert!(!vaults.is_empty());
        assert!(vaults.iter().any(|v| v.scope == VaultScope::Global));
    }
}
