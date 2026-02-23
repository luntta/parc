use std::path::{Path, PathBuf};

use chrono::Utc;
use similar::{ChangeTag, TextDiff};

use crate::error::ParcError;
use crate::fragment::{self, Fragment};

#[derive(Debug, Clone)]
pub struct VersionEntry {
    pub timestamp: String,
    pub path: PathBuf,
    pub size: u64,
}

/// Save a snapshot of the current fragment file before it is overwritten.
/// If the fragment file doesn't exist (new fragment), this is a no-op.
pub fn save_snapshot(vault: &Path, fragment_id: &str) -> Result<(), ParcError> {
    let fragment_path = vault
        .join("fragments")
        .join(format!("{}.md", fragment_id));

    if !fragment_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&fragment_path)?;

    // Use current time as the snapshot timestamp (when the snapshot was taken).
    // Micros precision to avoid collisions in fast operations.
    let timestamp = Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);

    let history_dir = vault.join("history").join(fragment_id);
    std::fs::create_dir_all(&history_dir)?;

    let snapshot_path = history_dir.join(format!("{}.md", timestamp));

    // Don't overwrite if this exact snapshot already exists
    if !snapshot_path.exists() {
        std::fs::write(&snapshot_path, &content)?;
    }

    Ok(())
}

/// List all version snapshots for a fragment, sorted newest-first.
pub fn list_versions(vault: &Path, fragment_id: &str) -> Result<Vec<VersionEntry>, ParcError> {
    let history_dir = vault.join("history").join(fragment_id);

    if !history_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut versions = Vec::new();
    for entry in std::fs::read_dir(&history_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let metadata = entry.metadata()?;
                versions.push(VersionEntry {
                    timestamp: stem.to_string(),
                    path,
                    size: metadata.len(),
                });
            }
        }
    }

    // Sort newest-first
    versions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    Ok(versions)
}

/// Read a specific historical version of a fragment.
pub fn read_version(
    vault: &Path,
    fragment_id: &str,
    timestamp: &str,
) -> Result<Fragment, ParcError> {
    let snapshot_path = vault
        .join("history")
        .join(fragment_id)
        .join(format!("{}.md", timestamp));

    if !snapshot_path.exists() {
        return Err(ParcError::ValidationError(format!(
            "version '{}' not found for fragment '{}'",
            timestamp,
            &fragment_id[..8.min(fragment_id.len())]
        )));
    }

    let content = std::fs::read_to_string(&snapshot_path)?;
    fragment::parse_fragment(&content)
}

/// Restore a previous version. This creates a new snapshot of the current
/// version first, then overwrites the fragment with the old version
/// (updating its `updated_at` to now).
pub fn restore_version(
    vault: &Path,
    fragment_id: &str,
    timestamp: &str,
) -> Result<Fragment, ParcError> {
    // Save current version as a snapshot first
    save_snapshot(vault, fragment_id)?;

    // Read the old version
    let mut old = read_version(vault, fragment_id, timestamp)?;
    old.updated_at = Utc::now();

    // Write it as the current version (this will also save a snapshot,
    // but write_fragment checks config — we already saved above manually,
    // so just write directly to avoid double-snapshot)
    let content = fragment::serialize_fragment(&old);
    let path = vault
        .join("fragments")
        .join(format!("{}.md", fragment_id));
    std::fs::write(&path, content)?;

    Ok(old)
}

/// Generate a unified diff between the current fragment and a historical version.
/// If no timestamp is provided, diffs against the most recent snapshot.
pub fn diff_versions(
    vault: &Path,
    fragment_id: &str,
    timestamp: Option<&str>,
) -> Result<String, ParcError> {
    let full_id = fragment::resolve_id(vault, fragment_id)?;

    // Read current version
    let current_path = vault
        .join("fragments")
        .join(format!("{}.md", full_id));
    let current_content = std::fs::read_to_string(&current_path)?;

    // Determine which historical version to diff against
    let ts = match timestamp {
        Some(t) => t.to_string(),
        None => {
            let versions = list_versions(vault, &full_id)?;
            if versions.is_empty() {
                return Err(ParcError::ValidationError(
                    "no history versions available".into(),
                ));
            }
            versions[0].timestamp.clone()
        }
    };

    let snapshot_path = vault
        .join("history")
        .join(&full_id)
        .join(format!("{}.md", ts));

    if !snapshot_path.exists() {
        return Err(ParcError::ValidationError(format!(
            "version '{}' not found",
            ts
        )));
    }

    let old_content = std::fs::read_to_string(&snapshot_path)?;

    let diff = TextDiff::from_lines(&old_content, &current_content);
    let mut output = String::new();

    output.push_str(&format!("--- {} ({})\n", &full_id[..8], ts));
    output.push_str(&format!("+++ {} (current)\n", &full_id[..8]));

    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        output.push_str(&format!("{}", hunk.header()));
        for change in hunk.iter_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            output.push_str(&format!("{}{}", sign, change));
            if change.missing_newline() {
                output.push('\n');
            }
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::serialize_fragment;
    use chrono::{DateTime, Utc};
    use std::collections::BTreeMap;

    fn make_fragment(id: &str, title: &str, body: &str) -> Fragment {
        Fragment {
            id: id.to_string(),
            fragment_type: "note".to_string(),
            title: title.to_string(),
            tags: Vec::new(),
            links: Vec::new(),
            attachments: Vec::new(),
            created_at: DateTime::parse_from_rfc3339("2026-02-21T10:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339("2026-02-21T10:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
            created_by: None,
            extra_fields: BTreeMap::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn test_save_snapshot_no_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        // Snapshot of non-existent fragment should be no-op
        assert!(save_snapshot(&vault, "NONEXISTENT").is_ok());
    }

    #[test]
    fn test_save_and_list_versions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag = make_fragment("01TEST000000000000000000AA", "V1", "Body v1");
        fragment::create_fragment(&vault, &frag).unwrap();

        // Save a snapshot
        save_snapshot(&vault, &frag.id).unwrap();

        let versions = list_versions(&vault, &frag.id).unwrap();
        assert_eq!(versions.len(), 1);
        // Timestamp is Utc::now() with millis, just check it starts with 20
        assert!(versions[0].timestamp.starts_with("20"));
    }

    #[test]
    fn test_read_version() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag = make_fragment("01TEST000000000000000000BB", "V1", "Body v1");
        fragment::create_fragment(&vault, &frag).unwrap();
        save_snapshot(&vault, &frag.id).unwrap();

        let versions = list_versions(&vault, &frag.id).unwrap();
        let old = read_version(&vault, &frag.id, &versions[0].timestamp).unwrap();
        assert_eq!(old.title, "V1");
        assert_eq!(old.body.trim(), "Body v1");
    }

    #[test]
    fn test_read_version_not_found() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag = make_fragment("01TEST000000000000000000CC", "V1", "Body");
        fragment::create_fragment(&vault, &frag).unwrap();

        assert!(read_version(&vault, &frag.id, "1999-01-01T00:00:00.000Z").is_err());
    }

    #[test]
    fn test_restore_version() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag = make_fragment("01TEST000000000000000000DD", "V1", "Body v1");
        fragment::create_fragment(&vault, &frag).unwrap();
        save_snapshot(&vault, &frag.id).unwrap();

        let v1_versions = list_versions(&vault, &frag.id).unwrap();
        let v1_ts = v1_versions[0].timestamp.clone();

        // Modify the fragment directly (simulating an edit)
        let mut v2 = frag.clone();
        v2.title = "V2".to_string();
        v2.body = "Body v2".to_string();
        v2.updated_at = DateTime::parse_from_rfc3339("2026-02-21T11:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let content = serialize_fragment(&v2);
        std::fs::write(
            vault.join("fragments").join(format!("{}.md", frag.id)),
            content,
        )
        .unwrap();

        // Restore v1
        let restored = restore_version(&vault, &frag.id, &v1_ts).unwrap();
        assert_eq!(restored.title, "V1");

        // Current file should now be v1 content (with updated timestamp)
        let current = fragment::read_fragment(&vault, &frag.id).unwrap();
        assert_eq!(current.title, "V1");

        // Should have 2 snapshots now (v1 original + v2 before restore)
        let versions = list_versions(&vault, &frag.id).unwrap();
        assert_eq!(versions.len(), 2);
    }

    #[test]
    fn test_diff_versions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let frag = make_fragment("01TEST000000000000000000EE", "V1", "Line one\nLine two\n");
        fragment::create_fragment(&vault, &frag).unwrap();
        save_snapshot(&vault, &frag.id).unwrap();

        let versions = list_versions(&vault, &frag.id).unwrap();
        let v1_ts = versions[0].timestamp.clone();

        // Modify the fragment
        let mut v2 = frag.clone();
        v2.title = "V2".to_string();
        v2.body = "Line one\nLine changed\n".to_string();
        v2.updated_at = DateTime::parse_from_rfc3339("2026-02-21T11:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let content = serialize_fragment(&v2);
        std::fs::write(
            vault.join("fragments").join(format!("{}.md", frag.id)),
            content,
        )
        .unwrap();

        let diff = diff_versions(&vault, &frag.id, Some(&v1_ts)).unwrap();
        assert!(diff.contains("-title: V1"));
        assert!(diff.contains("+title: V2"));
        assert!(diff.contains("-Line two"));
        assert!(diff.contains("+Line changed"));
    }
}
