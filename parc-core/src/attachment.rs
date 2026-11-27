use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::ParcError;
use crate::fragment;
use crate::secure_fs;

/// Reject filenames that could escape the per-fragment attachment directory or
/// collide with internal metadata directories (`.text`).
/// Filenames must be a single path component with no separators or `..`.
pub fn validate_attachment_filename(name: &str) -> Result<(), ParcError> {
    if name.is_empty() {
        return Err(ParcError::ValidationError(
            "attachment filename cannot be empty".into(),
        ));
    }
    if name == "." || name == ".." || name == TEXT_SIDECAR_DIR {
        return Err(ParcError::ValidationError(format!(
            "invalid attachment filename '{}'",
            name
        )));
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        return Err(ParcError::ValidationError(format!(
            "attachment filename '{}' must not contain path separators",
            name
        )));
    }
    // A real Path::file_name() round-trip should be a no-op for valid names.
    let p = Path::new(name);
    if p.file_name().and_then(|n| n.to_str()) != Some(name) {
        return Err(ParcError::ValidationError(format!(
            "invalid attachment filename '{}'",
            name
        )));
    }
    Ok(())
}

/// Directory inside `attachments/<id>/` that holds extracted text sidecars.
/// Excluded from attachment listings, counts, and doctor mismatch checks.
pub const TEXT_SIDECAR_DIR: &str = ".text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachMode {
    Copy,
    Move,
}

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub filename: String,
    pub size: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AttachmentRef {
    pub filename: String,
    pub display_text: Option<String>,
}

/// Path to the `.text/` sidecar directory for a fragment.
pub fn sidecar_dir(vault: &Path, full_id: &str) -> PathBuf {
    vault
        .join("attachments")
        .join(full_id)
        .join(TEXT_SIDECAR_DIR)
}

/// Sidecar paths a given attachment filename could have (`.txt` or `.md`).
pub fn sidecar_paths(vault: &Path, full_id: &str, filename: &str) -> [PathBuf; 2] {
    let dir = sidecar_dir(vault, full_id);
    [
        dir.join(format!("{filename}.txt")),
        dir.join(format!("{filename}.md")),
    ]
}

/// Remove any `.text/` sidecars belonging to `filename`, and clean up `.text/`
/// if it is now empty.
pub fn remove_sidecars(vault: &Path, full_id: &str, filename: &str) -> Result<(), ParcError> {
    validate_attachment_filename(filename)?;
    for path in sidecar_paths(vault, full_id, filename) {
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if !meta.file_type().is_symlink() && meta.is_file() => {
                std::fs::remove_file(&path)?;
            }
            Ok(_) | Err(_) => {}
        }
    }
    cleanup_empty_dir(&sidecar_dir(vault, full_id));
    Ok(())
}

/// Move any `.text/` sidecars belonging to `filename` from one fragment to
/// another.
pub fn move_sidecars(
    vault: &Path,
    from_id: &str,
    to_id: &str,
    filename: &str,
) -> Result<(), ParcError> {
    validate_attachment_filename(filename)?;
    let from_paths = sidecar_paths(vault, from_id, filename);
    let to_paths = sidecar_paths(vault, to_id, filename);
    for (src, dst) in from_paths.iter().zip(to_paths.iter()) {
        if let Ok(meta) = std::fs::symlink_metadata(src) {
            if !meta.file_type().is_symlink() && meta.is_file() {
                if let Some(parent) = dst.parent() {
                    secure_fs::create_private_dir_all(parent)?;
                }
                secure_fs::rename_private_file(src, dst)?;
            }
        }
    }
    cleanup_empty_dir(&sidecar_dir(vault, from_id));
    cleanup_empty_dir(&vault.join("attachments").join(from_id));
    Ok(())
}

fn cleanup_empty_dir(dir: &Path) {
    if dir.is_dir() {
        if let Ok(mut entries) = std::fs::read_dir(dir) {
            if entries.next().is_none() {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }
}

/// Copy or move `source_path` into `attachments/<full_id>/` without modifying
/// fragment frontmatter. Frontends should call [`crate::ops::Writer::attach`].
pub fn store_attachment_file(
    vault: &Path,
    full_id: &str,
    source_path: &Path,
    mode: AttachMode,
) -> Result<String, ParcError> {
    fragment::validate_id(full_id)?;
    let meta = std::fs::symlink_metadata(source_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ParcError::ValidationError(format!("file not found: {}", source_path.display()))
        } else {
            ParcError::Io(e)
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(ParcError::ValidationError(format!(
            "refusing to follow symlink '{}'",
            source_path.display()
        )));
    }
    if !meta.is_file() {
        return Err(ParcError::ValidationError(format!(
            "not a regular file: {}",
            source_path.display()
        )));
    }

    let filename = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ParcError::ValidationError("invalid filename".into()))?
        .to_string();
    validate_attachment_filename(&filename)?;

    let attach_dir = vault.join("attachments").join(full_id);
    secure_fs::create_private_dir_all(&attach_dir)?;

    let dest = attach_dir.join(&filename);
    if dest.exists() || std::fs::symlink_metadata(&dest).is_ok() {
        return Err(ParcError::ValidationError(format!(
            "attachment '{}' already exists for fragment '{}'",
            filename,
            &full_id[..8.min(full_id.len())]
        )));
    }

    match mode {
        AttachMode::Move => {
            // Try rename first, fall back to copy+delete for cross-device moves
            match secure_fs::rename_private_attachment(source_path, &dest) {
                Ok(()) => {}
                Err(ParcError::Io(err)) if err.kind() == std::io::ErrorKind::CrossesDevices => {
                    secure_fs::copy_private_new(source_path, &dest)?;
                    std::fs::remove_file(source_path)?;
                }
                Err(err) => return Err(err),
            }
        }
        AttachMode::Copy => {
            secure_fs::copy_private_new(source_path, &dest)?;
        }
    }

    Ok(filename)
}

/// Remove an attachment file (and its text sidecars) from `attachments/<full_id>/`
/// without modifying fragment frontmatter. Frontends should call [`crate::ops::Writer::detach`].
pub fn remove_attachment_file(
    vault: &Path,
    full_id: &str,
    filename: &str,
) -> Result<(), ParcError> {
    fragment::validate_id(full_id)?;
    validate_attachment_filename(filename)?;

    let attach_path = vault.join("attachments").join(full_id).join(filename);
    match std::fs::symlink_metadata(&attach_path) {
        Ok(_) => secure_fs::remove_private_file(&attach_path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(ParcError::Io(e)),
    }

    remove_sidecars(vault, full_id, filename)?;

    // Remove empty attachment directory
    cleanup_empty_dir(&vault.join("attachments").join(full_id));
    Ok(())
}

/// Move the original attachment file (derived sidecars are handled after commit) from `from_id` to `to_id`
/// without modifying frontmatter. Frontends should call [`crate::ops::Writer::move_attachment`].
pub fn move_attachment_file(
    vault: &Path,
    from_id: &str,
    to_id: &str,
    filename: &str,
) -> Result<(), ParcError> {
    fragment::validate_id(from_id)?;
    fragment::validate_id(to_id)?;
    validate_attachment_filename(filename)?;

    let src = vault.join("attachments").join(from_id).join(filename);
    if std::fs::symlink_metadata(&src).is_err() {
        return Err(ParcError::ValidationError(format!(
            "attachment '{}' not found on fragment '{}'",
            filename,
            &from_id[..8.min(from_id.len())]
        )));
    }

    let dest_dir = vault.join("attachments").join(to_id);
    secure_fs::create_private_dir_all(&dest_dir)?;
    let dest = dest_dir.join(filename);
    if dest.exists() || std::fs::symlink_metadata(&dest).is_ok() {
        return Err(ParcError::ValidationError(format!(
            "attachment '{}' already exists for fragment '{}'",
            filename,
            &to_id[..8.min(to_id.len())]
        )));
    }

    secure_fs::rename_private_attachment(&src, &dest)?;
    cleanup_empty_dir(&vault.join("attachments").join(from_id));
    Ok(())
}

/// Attach a file to a fragment. Prefer [`crate::ops::Writer::attach`], which
/// runs hooks, records history, and updates the index.
pub fn attach_file(
    vault: &Path,
    id_or_prefix: &str,
    source_path: &Path,
    move_file: bool,
) -> Result<String, ParcError> {
    let full_id = fragment::resolve_id(vault, id_or_prefix)?;
    let mode = if move_file {
        AttachMode::Move
    } else {
        AttachMode::Copy
    };
    let filename = store_attachment_file(vault, &full_id, source_path, mode)?;

    // Update fragment frontmatter
    let mut frag = fragment::read_fragment(vault, &full_id)?;
    if !frag.attachments.contains(&filename) {
        frag.attachments.push(filename.clone());
        fragment::write_fragment(vault, &frag)?;
    }

    Ok(filename)
}

/// Remove an attachment from a fragment. Prefer [`crate::ops::Writer::detach`],
/// which runs hooks, records history, and updates the index.
pub fn detach_file(vault: &Path, id_or_prefix: &str, filename: &str) -> Result<(), ParcError> {
    validate_attachment_filename(filename)?;
    let full_id = fragment::resolve_id(vault, id_or_prefix)?;

    remove_attachment_file(vault, &full_id, filename)?;

    // Update fragment frontmatter
    let mut frag = fragment::read_fragment(vault, &full_id)?;
    frag.attachments.retain(|a| a != filename);
    fragment::write_fragment(vault, &frag)?;

    Ok(())
}

/// List attachments for a fragment (from filesystem). Excludes internal
/// directories such as `.text/` and skips symlinks.
pub fn list_attachments(
    vault: &Path,
    id_or_prefix: &str,
) -> Result<Vec<AttachmentInfo>, ParcError> {
    let full_id = fragment::resolve_id(vault, id_or_prefix)?;
    list_attachments_by_id(vault, &full_id)
}

/// List attachments for a fragment whose full ID is already known.
pub fn list_attachments_by_id(
    vault: &Path,
    full_id: &str,
) -> Result<Vec<AttachmentInfo>, ParcError> {
    fragment::validate_id(full_id)?;
    let attach_dir = vault.join("attachments").join(full_id);
    if !attach_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut attachments = Vec::new();
    for entry in std::fs::read_dir(&attach_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == TEXT_SIDECAR_DIR {
            continue;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_file() && !metadata.file_type().is_symlink() {
            attachments.push(AttachmentInfo {
                filename: name.to_string(),
                size: metadata.len(),
                path,
            });
        }
    }

    attachments.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(attachments)
}

/// Parse `![[attach:filename]]` and `![[attach:filename|display text]]` references
/// from Markdown body text. Ignores references inside fenced code blocks and inline code.
pub fn parse_attachment_refs(body: &str) -> Vec<AttachmentRef> {
    let re = Regex::new(r"!\[\[attach:([^\]|]+?)(?:\|([^\]]+?))?\]\]").unwrap();
    let mut refs = Vec::new();

    // Track fenced code block state
    let mut in_fenced_block = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fenced_block = !in_fenced_block;
            continue;
        }
        if in_fenced_block {
            continue;
        }

        // Remove inline code spans before matching
        let cleaned = remove_inline_code(line);
        for caps in re.captures_iter(&cleaned) {
            refs.push(AttachmentRef {
                filename: caps[1].trim().to_string(),
                display_text: caps.get(2).map(|m| m.as_str().trim().to_string()),
            });
        }
    }

    refs
}

fn remove_inline_code(line: &str) -> String {
    let re = Regex::new(r"`[^`]+`").unwrap();
    re.replace_all(line, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::load_config;
    use crate::fragment::{create_fragment, new_fragment};
    use crate::schema::load_schemas;

    fn setup_vault() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        (tmp, vault)
    }

    fn create_test_fragment(vault: &Path) -> String {
        let config = load_config(vault).unwrap();
        let schemas = load_schemas(vault).unwrap();
        let schema = schemas.resolve("note").unwrap();
        let frag = new_fragment("note", "Test", schema, &config);
        create_fragment(vault, &frag).unwrap()
    }

    #[test]
    fn test_validate_attachment_filename() {
        assert!(validate_attachment_filename("ok.txt").is_ok());
        assert!(validate_attachment_filename("with space.png").is_ok());
        assert!(validate_attachment_filename("").is_err());
        assert!(validate_attachment_filename(".").is_err());
        assert!(validate_attachment_filename("..").is_err());
        assert!(validate_attachment_filename("../sibling.txt").is_err());
        assert!(validate_attachment_filename("a/b.txt").is_err());
        assert!(validate_attachment_filename("a\\b.txt").is_err());
    }

    #[test]
    fn test_detach_rejects_traversal() {
        let (_tmp, vault) = setup_vault();
        let id = create_test_fragment(&vault);
        // Plant a sibling file we want to protect
        let sibling = vault.join("attachments").join("sibling.txt");
        std::fs::create_dir_all(sibling.parent().unwrap()).unwrap();
        std::fs::write(&sibling, "do not delete").unwrap();

        let result = detach_file(&vault, &id, "../sibling.txt");
        assert!(matches!(result, Err(ParcError::ValidationError(_))));
        assert!(sibling.exists());
    }

    #[test]
    fn test_attach_and_list() {
        let (_tmp, vault) = setup_vault();
        let id = create_test_fragment(&vault);

        // Create a test file
        let test_file = _tmp.path().join("test.txt");
        std::fs::write(&test_file, "hello").unwrap();

        let filename = attach_file(&vault, &id, &test_file, false).unwrap();
        assert_eq!(filename, "test.txt");

        // File should be copied to attachments dir
        assert!(vault
            .join("attachments")
            .join(&id)
            .join("test.txt")
            .exists());
        // Source should still exist (copy, not move)
        assert!(test_file.exists());

        let list = list_attachments(&vault, &id).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].filename, "test.txt");

        // Fragment frontmatter should include attachment
        let frag = fragment::read_fragment(&vault, &id).unwrap();
        assert_eq!(frag.attachments, vec!["test.txt"]);
    }

    #[test]
    fn test_attach_move() {
        let (_tmp, vault) = setup_vault();
        let id = create_test_fragment(&vault);

        let test_file = _tmp.path().join("moveme.txt");
        std::fs::write(&test_file, "data").unwrap();

        attach_file(&vault, &id, &test_file, true).unwrap();

        // Source should be gone
        assert!(!test_file.exists());
        // File should be in attachments
        assert!(vault
            .join("attachments")
            .join(&id)
            .join("moveme.txt")
            .exists());
    }

    #[test]
    fn test_attach_duplicate() {
        let (_tmp, vault) = setup_vault();
        let id = create_test_fragment(&vault);

        let test_file = _tmp.path().join("dup.txt");
        std::fs::write(&test_file, "data").unwrap();

        attach_file(&vault, &id, &test_file, false).unwrap();
        let result = attach_file(&vault, &id, &test_file, false);
        assert!(result.is_err());
    }

    #[test]
    fn test_detach() {
        let (_tmp, vault) = setup_vault();
        let id = create_test_fragment(&vault);

        let test_file = _tmp.path().join("remove.txt");
        std::fs::write(&test_file, "data").unwrap();

        attach_file(&vault, &id, &test_file, false).unwrap();
        detach_file(&vault, &id, "remove.txt").unwrap();

        let list = list_attachments(&vault, &id).unwrap();
        assert!(list.is_empty());

        // Fragment frontmatter should be updated
        let frag = fragment::read_fragment(&vault, &id).unwrap();
        assert!(frag.attachments.is_empty());
    }

    #[test]
    fn test_attach_file_not_found() {
        let (_tmp, vault) = setup_vault();
        let id = create_test_fragment(&vault);

        let result = attach_file(&vault, &id, Path::new("/nonexistent/file.txt"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_attachment_refs() {
        let body = r#"
See the diagram: ![[attach:diagram.svg]]
The spec: ![[attach:spec.pdf|Project Spec]]
Normal link: [[01JQ7V4Y]]
"#;
        let refs = parse_attachment_refs(body);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].filename, "diagram.svg");
        assert!(refs[0].display_text.is_none());
        assert_eq!(refs[1].filename, "spec.pdf");
        assert_eq!(refs[1].display_text.as_deref(), Some("Project Spec"));
    }

    #[test]
    fn test_parse_attachment_refs_in_code_block() {
        let body = r#"
Normal ref: ![[attach:real.png]]

```markdown
Not a ref: ![[attach:fake.png]]
```

`Also ignored: ![[attach:inline.png]]`
"#;
        let refs = parse_attachment_refs(body);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].filename, "real.png");
    }
}
