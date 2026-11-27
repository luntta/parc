//! Sidecar files storing extracted attachment text beside the attachment in
//! `attachments/<id>/.text/<filename>.txt` (PDF, plain text) or `.md` (docx,
//! pptx, HTML, Markdown).
//!
//! File format:
//! - Line 1: `<!-- parc-text {"v":1,"sha256":"...","unit":"page","format":"text","title":"..."} -->`
//! - Remaining bytes: parts joined by ASCII form feed (`\x0c`).
//!
//! Freshness rule:
//! - Hash differs or `v < TEXT_FORMAT` -> [`SidecarState::Stale`] (redo).
//! - `v > TEXT_FORMAT` -> [`SidecarState::NewerVersion`] (use for indexing,
//!   never overwrite on disk, so two computers on different parc versions don't
//!   keep rewriting each other's sidecars).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::attachment::{self, sidecar_dir, sidecar_paths, validate_attachment_filename};
use crate::error::ParcError;
use crate::extract::{Extracted, PartUnit, TextFormat, TEXT_FORMAT};
use crate::fragment;
use crate::secure_fs;

const HEADER_PREFIX: &str = "<!-- parc-text ";
const HEADER_SUFFIX: &str = " -->";
const PART_SEP: char = '\x0c';

#[derive(Debug, Serialize, Deserialize)]
struct SidecarHeader {
    v: u32,
    sha256: String,
    unit: PartUnit,
    format: TextFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

/// Result of inspecting an attachment's sidecar on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarState {
    Fresh(Extracted),
    NewerVersion(Extracted),
    Stale,
    Missing,
}

/// Inspect any existing sidecar for `(fragment_id, filename)`.
pub fn read_sidecar(
    vault: &Path,
    fragment_id: &str,
    filename: &str,
    expected_sha256: &str,
) -> SidecarState {
    if fragment::validate_id(fragment_id).is_err()
        || validate_attachment_filename(filename).is_err()
    {
        return SidecarState::Missing;
    }

    for path in sidecar_paths(vault, fragment_id, filename) {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            return SidecarState::Stale;
        };
        let Some((header, parts)) = parse_sidecar(&content) else {
            return SidecarState::Stale;
        };
        if header.sha256 != expected_sha256 {
            return SidecarState::Stale;
        }
        let extracted = Extracted {
            title: header.title,
            unit: header.unit,
            format: header.format,
            parts,
        };
        if header.v > TEXT_FORMAT {
            return SidecarState::NewerVersion(extracted);
        }
        if header.v < TEXT_FORMAT {
            return SidecarState::Stale;
        }
        return SidecarState::Fresh(extracted);
    }

    SidecarState::Missing
}

/// Write a sidecar file atomically with `secure_fs`. Leaves any existing
/// newer-version sidecar untouched.
pub fn write_sidecar(
    vault: &Path,
    fragment_id: &str,
    filename: &str,
    sha256: &str,
    extracted: &Extracted,
) -> Result<PathBuf, ParcError> {
    fragment::validate_id(fragment_id)?;
    validate_attachment_filename(filename)?;

    // Never downgrade or overwrite a sidecar written by a newer parc.
    if matches!(
        read_sidecar(vault, fragment_id, filename, sha256),
        SidecarState::NewerVersion(_)
    ) {
        return Ok(sidecar_path(vault, fragment_id, filename, extracted.format));
    }

    let dir = sidecar_dir(vault, fragment_id);
    secure_fs::create_private_dir_all(&dir)?;

    let target = sidecar_path(vault, fragment_id, filename, extracted.format);
    let body = render_sidecar(sha256, extracted)?;
    secure_fs::write_private_atomic(&target, body)?;

    // Remove the opposite extension if the file's format changed (e.g. .txt -> .md).
    let other_ext = match extracted.format {
        TextFormat::Text => "md",
        TextFormat::Markdown => "txt",
    };
    let other = dir.join(format!("{filename}.{other_ext}"));
    if let Ok(meta) = std::fs::symlink_metadata(&other) {
        if !meta.file_type().is_symlink() && meta.is_file() {
            let _ = std::fs::remove_file(&other);
        }
    }

    Ok(target)
}

/// Strip the sidecar header line if present, returning raw body text.
pub fn strip_header(content: &str) -> &str {
    if let Some((first, rest)) = content.split_once('\n') {
        if first.starts_with(HEADER_PREFIX) && first.ends_with(HEADER_SUFFIX) {
            return rest;
        }
    }
    content
}

pub fn sidecar_path(
    vault: &Path,
    fragment_id: &str,
    filename: &str,
    format: TextFormat,
) -> PathBuf {
    attachment::sidecar_dir(vault, fragment_id).join(format!("{filename}.{}", format.sidecar_ext()))
}

fn render_sidecar(sha256: &str, extracted: &Extracted) -> Result<String, ParcError> {
    let header = SidecarHeader {
        v: TEXT_FORMAT,
        sha256: sha256.to_string(),
        unit: extracted.unit,
        format: extracted.format,
        title: extracted.title.clone(),
    };
    let header_json = serde_json::to_string(&header)?;
    let joined = extracted
        .parts
        .iter()
        .map(|p| p.replace(PART_SEP, "\n"))
        .collect::<Vec<_>>()
        .join(&PART_SEP.to_string());
    Ok(format!(
        "{HEADER_PREFIX}{header_json}{HEADER_SUFFIX}\n{joined}"
    ))
}

fn parse_sidecar(content: &str) -> Option<(SidecarHeader, Vec<String>)> {
    let (first_line, body) = content.split_once('\n')?;
    let first_line = first_line.strip_suffix('\r').unwrap_or(first_line);
    let json = first_line
        .strip_prefix(HEADER_PREFIX)?
        .strip_suffix(HEADER_SUFFIX)?;
    let header: SidecarHeader = serde_json::from_str(json).ok()?;
    if body.is_empty() {
        return Some((header, Vec::new()));
    }
    let parts = body.split(PART_SEP).map(str::to_string).collect();
    Some((header, parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, String) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let id = fragment::new_id();
        (tmp, vault, id)
    }

    #[test]
    fn roundtrip_and_version_freshness() {
        let (_tmp, vault, id) = setup();
        let extracted = Extracted {
            title: Some("Quarterly Report".into()),
            unit: PartUnit::Page,
            format: TextFormat::Text,
            parts: vec!["Page one".into(), "Page two".into()],
        };
        let hash = "a".repeat(64);

        let path = write_sidecar(&vault, &id, "report.pdf", &hash, &extracted).unwrap();
        assert!(path.ends_with(".text/report.pdf.txt"));

        assert_eq!(
            read_sidecar(&vault, &id, "report.pdf", &hash),
            SidecarState::Fresh(extracted.clone())
        );

        // Different hash -> stale
        let other_hash = "b".repeat(64);
        assert_eq!(
            read_sidecar(&vault, &id, "report.pdf", &other_hash),
            SidecarState::Stale
        );

        // Lower TEXT_FORMAT -> stale
        let old = format!(
            "<!-- parc-text {{\"v\":0,\"sha256\":\"{hash}\",\"unit\":\"page\",\"format\":\"text\"}} -->\nOld"
        );
        secure_fs::write_private(&path, old).unwrap();
        assert_eq!(
            read_sidecar(&vault, &id, "report.pdf", &hash),
            SidecarState::Stale
        );

        // Higher TEXT_FORMAT -> NewerVersion, and write_sidecar leaves it alone
        let newer_v = TEXT_FORMAT + 1;
        let newer = format!(
            "<!-- parc-text {{\"v\":{newer_v},\"sha256\":\"{hash}\",\"unit\":\"page\",\"format\":\"text\"}} -->\nFrom future"
        );
        secure_fs::write_private(&path, &newer).unwrap();
        match read_sidecar(&vault, &id, "report.pdf", &hash) {
            SidecarState::NewerVersion(ext) => assert_eq!(ext.parts, vec!["From future"]),
            other => panic!("expected NewerVersion, got {other:?}"),
        }
        write_sidecar(&vault, &id, "report.pdf", &hash, &extracted).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
    }
}
