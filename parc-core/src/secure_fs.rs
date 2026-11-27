use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::ParcError;

#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;

pub fn create_private_dir_all(path: &Path) -> Result<(), ParcError> {
    fs::create_dir_all(path)?;
    set_private_dir_permissions(path)?;
    Ok(())
}

pub fn prepare_private_file(path: &Path) -> Result<(), ParcError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            reject_symlink_metadata(path, &metadata)?;
            if metadata.is_dir() {
                return Err(ParcError::ValidationError(format!(
                    "refusing to use directory '{}' as a file",
                    path.display()
                )));
            }
            set_private_file_permissions(path)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => write_private_new(path, []),
        Err(err) => Err(ParcError::Io(err)),
    }
}

pub fn write_private(path: &Path, contents: impl AsRef<[u8]>) -> Result<(), ParcError> {
    reject_existing_symlink(path)?;
    let mut file = private_open_options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(contents.as_ref())?;
    set_private_file_permissions(path)?;
    Ok(())
}

pub fn write_private_new(path: &Path, contents: impl AsRef<[u8]>) -> Result<(), ParcError> {
    reject_existing_symlink(path)?;
    let mut file = private_open_options()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(contents.as_ref())?;
    set_private_file_permissions(path)?;
    Ok(())
}

/// Replace a file's contents in one step: write a sibling temporary file,
/// then rename it over `path`, so a reader (or a crash) never sees half a file.
pub fn write_private_atomic(path: &Path, contents: impl AsRef<[u8]>) -> Result<(), ParcError> {
    reject_existing_symlink(path)?;
    let name = path.file_name().ok_or_else(|| {
        ParcError::ValidationError(format!("not a file path: {}", path.display()))
    })?;
    let temp = path.with_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        ulid::Ulid::new()
    ));
    write_private_new(&temp, contents)?;
    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(ParcError::Io(err));
    }
    Ok(())
}

pub fn write_private_temp(
    prefix: &str,
    suffix: &str,
    contents: impl AsRef<[u8]> + Copy,
) -> Result<PathBuf, ParcError> {
    let dir = std::env::temp_dir();
    for _ in 0..32 {
        let path = dir.join(format!("{}-{}{}", prefix, ulid::Ulid::new(), suffix));
        match write_private_new(&path, contents) {
            Ok(()) => return Ok(path),
            Err(ParcError::Io(err)) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }

    Err(ParcError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "failed to create a unique temporary file",
    )))
}

pub fn copy_private_new(source: &Path, dest: &Path) -> Result<u64, ParcError> {
    reject_source_symlink(source)?;
    reject_existing_symlink(dest)?;

    let mut input = fs::File::open(source)?;
    let mut output = private_open_options()
        .create_new(true)
        .write(true)
        .open(dest)?;
    let bytes = std::io::copy(&mut input, &mut output)?;
    set_private_file_permissions(dest)?;
    Ok(bytes)
}

pub fn rename_private_file(source: &Path, dest: &Path) -> Result<(), ParcError> {
    reject_source_symlink(source)?;
    reject_existing_symlink(dest)?;
    fs::rename(source, dest)?;
    set_private_file_permissions(dest)?;
    Ok(())
}

/// Make the attachment private before rename. Once the original has moved
/// no fallible chmod may turn a committed transfer into a reported failure.
pub(crate) fn rename_private_attachment(source: &Path, dest: &Path) -> Result<(), ParcError> {
    let meta = fs::symlink_metadata(source)?;
    if !meta.is_file() {
        return Err(ParcError::ValidationError(
            "attachment is not a regular file".into(),
        ));
    }
    reject_existing_symlink(dest)?;
    set_private_file_permissions(source)?;
    fs::rename(source, dest)?;
    Ok(())
}

/// Remove a regular file, refusing symlinks and directories.
pub fn remove_private_file(path: &Path) -> Result<(), ParcError> {
    let metadata = fs::symlink_metadata(path)?;
    reject_symlink_metadata(path, &metadata)?;
    if !metadata.is_file() {
        return Err(ParcError::ValidationError(format!(
            "refusing to remove non-file '{}'",
            path.display()
        )));
    }
    fs::remove_file(path)?;
    Ok(())
}

/// Recursively remove a directory tree after checking that neither `path` nor
/// any entry inside it is a symlink. Missing paths are a no-op.
pub fn remove_private_dir_all(path: &Path) -> Result<(), ParcError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(ParcError::Io(e)),
    };
    reject_symlink_metadata(path, &metadata)?;
    if !metadata.is_dir() {
        return Err(ParcError::ValidationError(format!(
            "refusing to remove non-directory '{}'",
            path.display()
        )));
    }
    check_tree_no_symlinks(path)?;
    remove_dir_contents(path)?;
    fs::remove_dir(path)?;
    Ok(())
}

fn check_tree_no_symlinks(dir: &Path) -> Result<(), ParcError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let child = entry.path();
        let meta = fs::symlink_metadata(&child)?;
        reject_symlink_metadata(&child, &meta)?;
        if meta.is_dir() {
            check_tree_no_symlinks(&child)?;
        }
    }
    Ok(())
}

fn remove_dir_contents(dir: &Path) -> Result<(), ParcError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let child = entry.path();
        let meta = fs::symlink_metadata(&child)?;
        reject_symlink_metadata(&child, &meta)?;
        if meta.is_dir() {
            remove_dir_contents(&child)?;
            fs::remove_dir(&child)?;
        } else {
            fs::remove_file(&child)?;
        }
    }
    Ok(())
}

pub fn set_private_file_permissions(path: &Path) -> Result<(), ParcError> {
    reject_existing_symlink(path)?;
    set_file_mode(path)
}

pub fn set_private_dir_permissions(path: &Path) -> Result<(), ParcError> {
    reject_existing_symlink(path)?;
    set_dir_mode(path)
}

fn reject_source_symlink(path: &Path) -> Result<(), ParcError> {
    let metadata = fs::symlink_metadata(path)?;
    reject_symlink_metadata(path, &metadata)
}

fn reject_existing_symlink(path: &Path) -> Result<(), ParcError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => reject_symlink_metadata(path, &metadata),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(ParcError::Io(err)),
    }
}

fn reject_symlink_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), ParcError> {
    if metadata.file_type().is_symlink() {
        return Err(ParcError::ValidationError(format!(
            "refusing to follow symlink '{}'",
            path.display()
        )));
    }
    Ok(())
}

fn private_open_options() -> fs::OpenOptions {
    let mut options = fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(PRIVATE_FILE_MODE);
    }
    options
}

#[cfg(unix)]
fn set_file_mode(path: &Path) -> Result<(), ParcError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_file_mode(_path: &Path) -> Result<(), ParcError> {
    Ok(())
}

#[cfg(unix)]
fn set_dir_mode(path: &Path) -> Result<(), ParcError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_dir_mode(_path: &Path) -> Result<(), ParcError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    #[cfg(unix)]
    fn write_private_uses_owner_only_permissions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("secret.txt");

        write_private(&path, b"secret").unwrap();

        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    #[cfg(unix)]
    fn create_private_dir_all_uses_owner_only_permissions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("vault");

        create_private_dir_all(&path).unwrap();

        assert_eq!(mode(&path), 0o700);
    }
}
