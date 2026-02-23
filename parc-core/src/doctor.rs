use std::collections::HashSet;
use std::path::Path;

use crate::attachment;
use crate::error::ParcError;
use crate::fragment::{self, Fragment};
use crate::link;
use crate::schema::{load_schemas, SchemaRegistry};

#[derive(Debug)]
pub enum DoctorFinding {
    BrokenLink {
        source_id: String,
        source_title: String,
        target_ref: String,
    },
    OrphanFragment {
        id: String,
        title: String,
    },
    SchemaViolation {
        id: String,
        title: String,
        message: String,
    },
    AttachmentMismatch {
        fragment_id: String,
        detail: String,
    },
    VaultSizeWarning {
        total_bytes: u64,
    },
    PluginIssue {
        plugin_name: String,
        detail: String,
    },
}

#[derive(Debug)]
pub struct DoctorReport {
    pub findings: Vec<DoctorFinding>,
    pub fragments_checked: usize,
}

impl DoctorReport {
    pub fn is_healthy(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Check for broken links: frontmatter links and body wiki-links
/// that reference non-existent fragment IDs.
pub fn check_broken_links(
    fragments: &[Fragment],
    all_ids: &[String],
) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();

    for frag in fragments {
        // Check frontmatter links
        for link_id in &frag.links {
            let upper = link_id.to_uppercase();
            let found = all_ids.iter().any(|id| id.starts_with(&upper));
            if !found {
                findings.push(DoctorFinding::BrokenLink {
                    source_id: frag.id.clone(),
                    source_title: frag.title.clone(),
                    target_ref: link_id.clone(),
                });
            }
        }

        // Check body wiki-links
        let wiki_links = link::parse_wiki_links(&frag.body);
        for wl in &wiki_links {
            let upper = wl.target.to_uppercase();
            let found = all_ids.iter().any(|id| id.starts_with(&upper));
            if !found {
                findings.push(DoctorFinding::BrokenLink {
                    source_id: frag.id.clone(),
                    source_title: frag.title.clone(),
                    target_ref: format!("[[{}]]", wl.target),
                });
            }
        }
    }

    findings
}

/// Check for orphan fragments: fragments with no inbound or outbound links.
pub fn check_orphans(
    fragments: &[Fragment],
    all_ids: &[String],
) -> Vec<DoctorFinding> {
    use std::collections::HashSet;

    // Build set of all IDs that participate in any link relationship
    let mut linked_ids = HashSet::new();

    for frag in fragments {
        // Outbound frontmatter links
        for link_id in &frag.links {
            let upper = link_id.to_uppercase();
            // Resolve prefix
            let matches: Vec<&String> = all_ids.iter().filter(|id| id.starts_with(&upper)).collect();
            if matches.len() == 1 {
                linked_ids.insert(frag.id.clone());
                linked_ids.insert(matches[0].clone());
            }
        }

        // Outbound body wiki-links
        let wiki_links = link::parse_wiki_links(&frag.body);
        for wl in &wiki_links {
            let upper = wl.target.to_uppercase();
            let matches: Vec<&String> = all_ids.iter().filter(|id| id.starts_with(&upper)).collect();
            if matches.len() == 1 {
                linked_ids.insert(frag.id.clone());
                linked_ids.insert(matches[0].clone());
            }
        }
    }

    fragments
        .iter()
        .filter(|f| !linked_ids.contains(&f.id))
        .map(|f| DoctorFinding::OrphanFragment {
            id: f.id.clone(),
            title: f.title.clone(),
        })
        .collect()
}

/// Check for schema violations.
pub fn check_schema_violations(
    fragments: &[Fragment],
    schemas: &SchemaRegistry,
) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();

    for frag in fragments {
        if let Some(schema) = schemas.resolve(&frag.fragment_type) {
            if let Err(e) = fragment::validate_fragment(frag, schema) {
                findings.push(DoctorFinding::SchemaViolation {
                    id: frag.id.clone(),
                    title: frag.title.clone(),
                    message: e.to_string(),
                });
            }
        }
    }

    findings
}

/// Check for attachment mismatches.
pub fn check_attachments(
    vault: &Path,
    fragments: &[Fragment],
) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    let mut referenced_dirs = HashSet::new();

    for frag in fragments {
        let attach_dir = vault.join("attachments").join(&frag.id);

        // Check frontmatter attachments have corresponding files
        for filename in &frag.attachments {
            let file_path = attach_dir.join(filename);
            if !file_path.exists() {
                findings.push(DoctorFinding::AttachmentMismatch {
                    fragment_id: frag.id.clone(),
                    detail: format!("frontmatter lists '{}' but file not found on disk", filename),
                });
            }
        }

        // Check body ![[attach:...]] refs have corresponding files
        let body_refs = attachment::parse_attachment_refs(&frag.body);
        for aref in &body_refs {
            let file_path = attach_dir.join(&aref.filename);
            if !file_path.exists() && !frag.attachments.contains(&aref.filename) {
                findings.push(DoctorFinding::AttachmentMismatch {
                    fragment_id: frag.id.clone(),
                    detail: format!(
                        "body references ![[attach:{}]] but file not found",
                        aref.filename
                    ),
                });
            }
        }

        // Check for files on disk not referenced in frontmatter
        if attach_dir.is_dir() {
            referenced_dirs.insert(frag.id.clone());
            if let Ok(entries) = std::fs::read_dir(&attach_dir) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.path().file_name().and_then(|n| n.to_str()) {
                        if !frag.attachments.contains(&name.to_string()) {
                            findings.push(DoctorFinding::AttachmentMismatch {
                                fragment_id: frag.id.clone(),
                                detail: format!(
                                    "file '{}' on disk but not listed in frontmatter",
                                    name
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    // Check for attachment directories belonging to non-existent fragments
    let fragment_ids: HashSet<&str> = fragments.iter().map(|f| f.id.as_str()).collect();
    let attachments_dir = vault.join("attachments");
    if attachments_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&attachments_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(dir_name) = entry.file_name().to_str() {
                        if !fragment_ids.contains(dir_name) {
                            findings.push(DoctorFinding::AttachmentMismatch {
                                fragment_id: dir_name.to_string(),
                                detail: "attachment directory exists but fragment not found".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    findings
}

/// Check vault total size.
pub fn check_vault_size(vault: &Path) -> Vec<DoctorFinding> {
    const WARN_THRESHOLD: u64 = 500 * 1024 * 1024; // 500 MB

    let total = dir_size(vault);
    if total > WARN_THRESHOLD {
        vec![DoctorFinding::VaultSizeWarning { total_bytes: total }]
    } else {
        Vec::new()
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// Check plugin manifests for issues.
pub fn check_plugins(vault: &Path) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();

    let discovered = match crate::plugin::discover_plugins(vault) {
        Ok(d) => d,
        Err(_) => return findings,
    };

    for disc in &discovered {
        // Check manifest validity
        if let Err(e) = crate::plugin::validate_manifest(&disc.manifest, vault) {
            findings.push(DoctorFinding::PluginIssue {
                plugin_name: disc.manifest.plugin.name.clone(),
                detail: e.to_string(),
            });
            continue;
        }

        // Check wasm file exists
        if !disc.wasm_path.exists() {
            findings.push(DoctorFinding::PluginIssue {
                plugin_name: disc.manifest.plugin.name.clone(),
                detail: format!("wasm file not found: {}", disc.wasm_path.display()),
            });
        }
    }

    findings
}

/// Run all checks and return a combined report.
pub fn run_doctor(vault: &Path) -> Result<DoctorReport, ParcError> {
    let all_ids = fragment::list_fragment_ids(vault)?;
    let schemas = load_schemas(vault)?;

    // Load all fragments
    let mut fragments = Vec::new();
    for id in &all_ids {
        let path = vault.join("fragments").join(format!("{}.md", id));
        let content = std::fs::read_to_string(&path)?;
        match fragment::parse_fragment(&content) {
            Ok(frag) => fragments.push(frag),
            Err(_) => continue,
        }
    }

    let fragments_checked = fragments.len();
    let mut findings = Vec::new();

    findings.extend(check_broken_links(&fragments, &all_ids));
    findings.extend(check_schema_violations(&fragments, &schemas));
    findings.extend(check_orphans(&fragments, &all_ids));
    findings.extend(check_attachments(vault, &fragments));
    findings.extend(check_vault_size(vault));
    findings.extend(check_plugins(vault));

    Ok(DoctorReport {
        findings,
        fragments_checked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fragment::new_id;
    use chrono::Utc;
    use std::collections::BTreeMap;

    fn make_fragment(title: &str, body: &str) -> Fragment {
        Fragment {
            id: new_id(),
            fragment_type: "note".to_string(),
            title: title.to_string(),
            tags: Vec::new(),
            links: Vec::new(),
            attachments: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            extra_fields: BTreeMap::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn test_broken_frontmatter_link() {
        let mut frag = make_fragment("Test", "Body");
        frag.links = vec!["NONEXISTENT".to_string()];
        let all_ids = vec![frag.id.clone()];

        let findings = check_broken_links(&[frag], &all_ids);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], DoctorFinding::BrokenLink { target_ref, .. } if target_ref == "NONEXISTENT"));
    }

    #[test]
    fn test_broken_body_link() {
        let frag = make_fragment("Test", "See [[DEADBEEF]] for details.");
        let all_ids = vec![frag.id.clone()];

        let findings = check_broken_links(&[frag], &all_ids);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], DoctorFinding::BrokenLink { target_ref, .. } if target_ref == "[[DEADBEEF]]"));
    }

    #[test]
    fn test_no_broken_links() {
        let frag_a = make_fragment("A", "Body");
        let mut frag_b = make_fragment("B", "Body");
        frag_b.links = vec![frag_a.id.clone()];
        let all_ids = vec![frag_a.id.clone(), frag_b.id.clone()];

        let findings = check_broken_links(&[frag_a, frag_b], &all_ids);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_orphan_detection() {
        let frag_a = make_fragment("Orphan", "No links");
        let frag_b = make_fragment("B", "Body");
        let mut frag_c = make_fragment("C", "Body");
        frag_c.links = vec![frag_b.id.clone()];
        let all_ids = vec![frag_a.id.clone(), frag_b.id.clone(), frag_c.id.clone()];

        let findings = check_orphans(&[frag_a.clone(), frag_b, frag_c], &all_ids);
        assert_eq!(findings.len(), 1);
        assert!(matches!(&findings[0], DoctorFinding::OrphanFragment { id, .. } if id == &frag_a.id));
    }

    #[test]
    fn test_run_doctor_healthy() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let mut frag_a = make_fragment("A", "See [[placeholder]].");
        let mut frag_b = make_fragment("B", "Body");
        // Make them link to each other via frontmatter
        frag_a.links = vec![frag_b.id.clone()];
        frag_b.links = vec![frag_a.id.clone()];
        // Fix body link to point to B
        frag_a.body = format!("See [[{}]].", &frag_b.id[..8]);

        fragment::create_fragment(&vault, &frag_a).unwrap();
        fragment::create_fragment(&vault, &frag_b).unwrap();

        let report = run_doctor(&vault).unwrap();
        assert_eq!(report.fragments_checked, 2);
        // No broken links or schema violations; both linked so no orphans
        let non_orphan_findings: Vec<_> = report.findings.iter()
            .filter(|f| !matches!(f, DoctorFinding::OrphanFragment { .. }))
            .collect();
        assert!(non_orphan_findings.is_empty());
    }
}
