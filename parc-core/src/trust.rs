//! Approval lives outside the vault. Bind each approval to a canonical path and
//! the exact config, hook and plugin files reviewed there (not the fragments).
use crate::{secure_fs, ParcError};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn home() -> Result<PathBuf, ParcError> {
    #[cfg(test)]
    {
        static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        Ok(HOME
            .get_or_init(|| tempfile::tempdir().unwrap())
            .path()
            .into())
    }
    #[cfg(not(test))]
    Ok(crate::vault::global_vault_path()?.parent().unwrap().into())
}

fn part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn fingerprint(vault: &Path, config: &[u8]) -> Result<String, ParcError> {
    let mut hash = Sha256::new();
    part(&mut hash, config);
    for name in ["hooks", "plugins"] {
        let dir = secure_fs::real_dir(vault, &[name])?;
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        let mut files = entries.collect::<io::Result<Vec<_>>>()?;
        files.sort_by_key(|f| f.file_name());
        for entry in files {
            if !entry.file_type()?.is_file() {
                return Err(ParcError::ValidationError(format!(
                    "expected a regular file in {name}/"
                )));
            }
            part(&mut hash, name.as_bytes());
            part(&mut hash, entry.file_name().as_encoded_bytes());
            let mut file_hash = Sha256::new();
            io::copy(&mut fs::File::open(entry.path())?, &mut file_hash)?;
            part(&mut hash, &file_hash.finalize());
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn record(vault: &Path, home: &Path, create: bool) -> Result<PathBuf, ParcError> {
    let vault = vault.canonicalize()?;
    let home = home.canonicalize()?;
    let dir = secure_fs::real_dir(&home, &[".parc-trust"])?;
    if dir.starts_with(&vault) {
        return Err(ParcError::ValidationError(
            "trust store must be outside vault".into(),
        ));
    }
    if create && !dir.exists() {
        secure_fs::create_private_dir_all(&dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = fs::symlink_metadata(&dir)?;
        if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            return Err(ParcError::ValidationError(
                "trust store must be owner-private".into(),
            ));
        }
    }
    Ok(dir.join(format!(
        "{:x}",
        Sha256::digest(vault.as_os_str().as_encoded_bytes())
    )))
}

/// Trust exactly the current contents after an explicit user request.
pub fn grant(vault: &Path) -> Result<(), ParcError> {
    grant_for_home(vault, &home()?)
}

pub fn grant_for_home(vault: &Path, home: &Path) -> Result<(), ParcError> {
    crate::vault::validate_safe_vault(vault)?;
    let hash = fingerprint(vault, &read_config(vault)?)?;
    secure_fs::write_private_atomic(&record(vault, home, true)?, hash)
}

fn read_config(vault: &Path) -> Result<Vec<u8>, ParcError> {
    let path = vault.join("config.yml");
    if !fs::symlink_metadata(&path)?.is_file() {
        return Err(ParcError::ValidationError(
            "expected a regular config.yml".into(),
        ));
    }
    Ok(fs::read(path)?)
}

pub fn revoke(vault: &Path) -> Result<(), ParcError> {
    match record(vault, &home()?, false).and_then(|path| secure_fs::remove_private_file(&path)) {
        Err(ParcError::Io(e)) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub(crate) fn approved_config(vault: &Path, config: &[u8]) -> bool {
    let check = || -> Result<bool, ParcError> {
        let path = record(vault, &home()?, false)?;
        if !fs::symlink_metadata(&path)?.is_file() {
            return Ok(false);
        }
        Ok(fs::read_to_string(path)? == fingerprint(vault, config)?)
    };
    check().unwrap_or(false)
}

#[cfg(feature = "wasm-plugins")]
pub(crate) fn approved(vault: &Path) -> bool {
    read_config(vault).is_ok_and(|config| approved_config(vault, &config))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_is_path_and_content_bound_and_revocable() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        fs::write(
            vault.join("config.yml"),
            "hooks: {enabled: true}\nserver: {transport: socket, socket_path: custom.sock}",
        )
        .unwrap();
        let active = |vault: &Path| {
            let c = crate::config::load_config(vault).unwrap();
            assert_eq!(c.hooks.enabled, c.server.socket_path.is_some());
            assert_eq!(c.hooks.enabled, c.server.transport == "socket");
            c.hooks.enabled
        };
        assert!(!active(&vault));
        grant(&vault).unwrap();
        assert!(active(&vault));
        fs::write(vault.join("fragments/note.md"), "ordinary edit").unwrap();
        assert!(active(&vault));
        for path in ["hooks/pre-create", "plugins/tool.toml", "plugins/tool.wasm"] {
            fs::write(vault.join(path), "first").unwrap();
            assert!(!active(&vault));
            grant(&vault).unwrap();
            fs::write(vault.join(path), "other").unwrap();
            assert!(!active(&vault));
            grant(&vault).unwrap();
        }
        let moved = tmp.path().join("moved");
        fs::rename(&vault, &moved).unwrap();
        assert!(!active(&moved));
        grant(&moved).unwrap();
        revoke(&moved).unwrap();
        assert!(!active(&moved));
    }
}
