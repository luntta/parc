use std::path::{Path, PathBuf};

use crate::error::ParcError;
use crate::secure_fs;

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
            validate_safe_vault(&candidate)?;
            return Ok(candidate);
        }
        if !current.pop() {
            break;
        }
    }

    // Fall back to global vault
    let global = global_vault_path()?;
    if is_vault(&global) {
        validate_safe_vault(&global)?;
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

/// Resolve the global vault directly, bypassing PARC_VAULT and local discovery.
pub fn resolve_global_vault() -> Result<PathBuf, ParcError> {
    let global = global_vault_path()?;
    if !is_vault(&global) {
        return Err(ParcError::VaultNotFound(global));
    }
    validate_safe_vault(&global)?;
    Ok(global)
}

/// Resolve a vault path: if it ends with `.parc`, use directly; otherwise append `.parc`.
/// Returns VaultNotFound if the resolved path is not a valid vault.
fn resolve_vault_path(path: &Path) -> Result<PathBuf, ParcError> {
    let vault_path = if path.ends_with(".parc") {
        path.to_path_buf()
    } else {
        path.join(".parc")
    };

    if !is_vault(&vault_path) {
        return Err(ParcError::VaultNotFound(vault_path));
    }
    validate_safe_vault(&vault_path)?;
    Ok(vault_path)
}

/// Reject auto-discovered vaults that live under directories controlled by
/// another non-root user or writable by a group/world without sticky-bit
/// protection. Set PARC_SAFE_VAULTS to an OS path-list to explicitly trust a
/// shared vault path.
pub fn validate_safe_vault(path: &Path) -> Result<(), ParcError> {
    validate_safe_vault_impl(path)
}

#[cfg(unix)]
fn validate_safe_vault_impl(path: &Path) -> Result<(), ParcError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let direct_metadata = std::fs::symlink_metadata(path)?;
    if direct_metadata.file_type().is_symlink() {
        return Err(ParcError::ValidationError(format!(
            "refusing unsafe vault symlink '{}'",
            path.display()
        )));
    }

    let canonical = std::fs::canonicalize(path)?;
    if is_explicitly_safe_vault(&canonical) {
        return Ok(());
    }

    // SAFETY: geteuid has no preconditions and does not mutate memory.
    let current_uid = unsafe { libc::geteuid() };
    let home_dir = global_vault_path()
        .ok()
        .and_then(|global| global.parent().map(Path::to_path_buf))
        .and_then(|home| std::fs::canonicalize(home).ok())
        .filter(|home| canonical.starts_with(home));

    for ancestor in canonical.ancestors() {
        let metadata = std::fs::metadata(ancestor)?;
        let owner = metadata.uid();
        let mode = metadata.permissions().mode();
        let writable_without_sticky = mode & 0o022 != 0 && mode & 0o1000 == 0;

        if owner == current_uid {
            if writable_without_sticky {
                return Err(unsafe_vault_error(&canonical, ancestor));
            }
        } else if writable_without_sticky {
            return Err(unsafe_vault_error(&canonical, ancestor));
        } else if let Some(home) = &home_dir {
            if ancestor == home {
                break;
            }
            return Err(unsafe_vault_error(&canonical, ancestor));
        } else if owner != 0 && owner != 65_534 && mode & 0o1000 == 0 {
            return Err(ParcError::ValidationError(format!(
                "refusing unsafe vault '{}': ancestor '{}' is not owner-controlled",
                canonical.display(),
                ancestor.display()
            )));
        }

        if home_dir.as_deref() == Some(ancestor) {
            break;
        }
    }

    Ok(())
}

#[cfg(unix)]
fn unsafe_vault_error(canonical: &Path, ancestor: &Path) -> ParcError {
    ParcError::ValidationError(format!(
        "refusing unsafe vault '{}': ancestor '{}' is not owner-controlled",
        canonical.display(),
        ancestor.display()
    ))
}

#[cfg(not(unix))]
fn validate_safe_vault_impl(_path: &Path) -> Result<(), ParcError> {
    Ok(())
}

#[cfg(unix)]
fn is_explicitly_safe_vault(canonical: &Path) -> bool {
    std::env::var_os("PARC_SAFE_VAULTS")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|path| std::fs::canonicalize(path).is_ok_and(|safe| safe == canonical))
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

/// Walk up from `start` looking for a `.git` directory, returning the git root if found.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Add an entry to a .gitignore file if it's not already present.
/// Creates the file if it doesn't exist.
fn add_gitignore_entry(gitignore_path: &Path, entry: &str) -> Result<(), ParcError> {
    if gitignore_path.exists() {
        let contents = std::fs::read_to_string(gitignore_path)?;
        if contents.lines().any(|line| line.trim() == entry) {
            return Ok(());
        }
        let separator = if contents.ends_with('\n') || contents.is_empty() {
            ""
        } else {
            "\n"
        };
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(gitignore_path)?;
        std::io::Write::write_all(&mut file, format!("{}{}\n", separator, entry).as_bytes())?;
    } else {
        std::fs::write(gitignore_path, format!("{}\n", entry))?;
    }
    Ok(())
}

/// Creates a new vault at the given path with the default directory structure.
pub fn init_vault(path: &Path) -> Result<(), ParcError> {
    if is_vault(path) {
        return Err(ParcError::VaultAlreadyExists(path.to_path_buf()));
    }

    // Create directory structure
    secure_fs::create_private_dir_all(path)?;
    for dir in VAULT_DIRS {
        secure_fs::create_private_dir_all(&path.join(dir))?;
    }

    // Write default config
    secure_fs::write_private(&path.join("config.yml"), DEFAULT_CONFIG)?;

    // Write built-in schemas
    for (name, content) in BUILTIN_SCHEMAS {
        secure_fs::write_private(&path.join("schemas").join(name), content)?;
    }

    // Write built-in templates
    for (name, content) in BUILTIN_TEMPLATES {
        secure_fs::write_private(&path.join("templates").join(name), content)?;
    }

    // If the vault's parent directory is inside a git repository,
    // ensure index.db is listed in .gitignore
    if let Some(parent) = path.parent() {
        if let Some(git_root) = find_git_root(parent) {
            let gitignore_path = git_root.join(".gitignore");
            let relative = path
                .strip_prefix(&git_root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let entry = format!("{}/index.db", relative);
            add_gitignore_entry(&gitignore_path, &entry)?;
        }
    }

    Ok(())
}

const VAULT_DIRS: &[&str] = &[
    "schemas",
    "templates",
    "fragments",
    "attachments",
    "history",
    "trash",
    "plugins",
    "hooks",
];

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
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "md"))
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
    ("decision.yml", include_str!("builtin/schemas/decision.yml")),
    ("risk.yml", include_str!("builtin/schemas/risk.yml")),
    ("idea.yml", include_str!("builtin/schemas/idea.yml")),
];

const BUILTIN_TEMPLATES: &[(&str, &str)] = &[
    ("note.md", include_str!("builtin/templates/note.md")),
    ("todo.md", include_str!("builtin/templates/todo.md")),
    ("decision.md", include_str!("builtin/templates/decision.md")),
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
    fn test_init_vault_creates_gitignore_in_git_repo() {
        let tmp = TempDir::new().unwrap();
        // Simulate a git repo by creating a .git directory
        std::fs::create_dir(tmp.path().join(".git")).unwrap();

        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        let gitignore = tmp.path().join(".gitignore");
        assert!(gitignore.exists());
        let contents = std::fs::read_to_string(&gitignore).unwrap();
        assert!(contents.contains(".parc/index.db"));
    }

    #[test]
    fn test_init_vault_appends_to_existing_gitignore() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "node_modules/\n").unwrap();

        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        let contents = std::fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(contents.starts_with("node_modules/\n"));
        assert!(contents.contains(".parc/index.db"));
    }

    #[test]
    fn test_init_vault_no_gitignore_without_git() {
        let tmp = TempDir::new().unwrap();
        // No .git directory

        let vault_path = tmp.path().join(".parc");
        init_vault(&vault_path).unwrap();

        assert!(!tmp.path().join(".gitignore").exists());
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
    #[cfg(unix)]
    fn test_discover_rejects_symlinked_vault() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let real_vault = tmp.path().join(".parc-real");
        init_vault(&real_vault).unwrap();
        symlink(&real_vault, tmp.path().join(".parc")).unwrap();

        let result = discover_vault_from(tmp.path());

        assert!(matches!(result, Err(ParcError::ValidationError(_))));
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
