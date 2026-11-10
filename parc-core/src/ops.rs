//! Fragment write operations.
//!
//! Every frontend (CLI, TUI, server) changes fragments through [`Writer`], so
//! a save runs the same steps no matter where it came from:
//!
//! 1. validate against the type's schema (unknown custom types are accepted)
//! 2. run pre-hooks (scripts, then WASM plugins when enabled); a failing
//!    pre-hook aborts the save
//! 3. write the file (`write_fragment` takes the history snapshot)
//! 4. update the index
//! 5. run post-hooks; failures become warnings, never errors
//!
//! Warnings accumulate on the writer; drain them with
//! [`Writer::take_warnings`] and show them however suits the frontend.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::Connection;
use serde_json::Value;

use crate::error::ParcError;
use crate::fragment::{self, Fragment};
use crate::hook::{self, HookEvent, HookRunner};
use crate::index;

/// Result of a link or unlink: the two full IDs, and whether either
/// fragment actually changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkChange {
    pub a: String,
    pub b: String,
    pub changed: bool,
}

pub struct Writer {
    vault: PathBuf,
    hooks: Box<dyn HookRunner>,
    #[cfg(feature = "wasm-plugins")]
    plugins: Option<crate::plugin::manager::PluginManager>,
    conn: Option<Connection>,
    warnings: Vec<String>,
}

impl Writer {
    pub fn new(vault: impl Into<PathBuf>, hooks: impl HookRunner + 'static) -> Self {
        Self {
            vault: vault.into(),
            hooks: Box::new(hooks),
            #[cfg(feature = "wasm-plugins")]
            plugins: None,
            conn: None,
            warnings: Vec::new(),
        }
    }

    /// Also dispatch hook events to these WASM plugins.
    #[cfg(feature = "wasm-plugins")]
    pub fn with_plugins(mut self, plugins: crate::plugin::manager::PluginManager) -> Self {
        self.plugins = Some(plugins);
        self
    }

    pub fn vault(&self) -> &Path {
        &self.vault
    }

    /// Warnings collected since the last call (post-hook failures etc.).
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Save a new fragment. Fails if a fragment with the same ID exists.
    pub fn create(&mut self, fragment: Fragment) -> Result<Fragment, ParcError> {
        fragment::validate_fragment_in_vault(&self.vault, &fragment)?;
        let fragment = self.pre_hooks(HookEvent::PreCreate, &fragment)?;
        fragment::create_fragment(&self.vault, &fragment)?;
        self.index(&fragment)?;
        self.post_hooks(HookEvent::PostCreate, &fragment);
        Ok(fragment)
    }

    /// Save changes to an existing fragment. Sets `updated_at` to now.
    pub fn update(&mut self, mut fragment: Fragment) -> Result<Fragment, ParcError> {
        fragment.updated_at = Utc::now();
        fragment::validate_fragment_in_vault(&self.vault, &fragment)?;
        let fragment = self.pre_hooks(HookEvent::PreUpdate, &fragment)?;
        fragment::write_fragment(&self.vault, &fragment)?;
        self.index(&fragment)?;
        self.post_hooks(HookEvent::PostUpdate, &fragment);
        Ok(fragment)
    }

    /// Move a fragment to the trash. Returns the fragment as it was.
    pub fn delete(&mut self, id_or_prefix: &str) -> Result<Fragment, ParcError> {
        let fragment = fragment::read_fragment(&self.vault, id_or_prefix)?;
        // Pre-delete hooks can veto the delete; their output is ignored.
        self.pre_hooks(HookEvent::PreDelete, &fragment)?;
        fragment::delete_fragment(&self.vault, &fragment.id)?;
        index::remove_from_index(self.conn()?, &fragment.id)?;
        self.post_hooks(HookEvent::PostDelete, &fragment);
        Ok(fragment)
    }

    pub fn set_archived(
        &mut self,
        id_or_prefix: &str,
        archived: bool,
    ) -> Result<Fragment, ParcError> {
        let mut fragment = fragment::read_fragment(&self.vault, id_or_prefix)?;
        if archived {
            fragment
                .extra_fields
                .insert("archived".to_string(), Value::Bool(true));
        } else {
            fragment.extra_fields.remove("archived");
        }
        self.update(fragment)
    }

    /// Convert a fragment to another type. See [`fragment::apply_promotion`].
    pub fn promote(
        &mut self,
        id_or_prefix: &str,
        new_type: &str,
        overrides: BTreeMap<String, Value>,
    ) -> Result<Fragment, ParcError> {
        let schemas = crate::schema::load_schemas(&self.vault)?;
        let schema = schemas
            .resolve(new_type)
            .ok_or_else(|| ParcError::SchemaNotFound(new_type.to_string()))?;
        let mut fragment = fragment::read_fragment(&self.vault, id_or_prefix)?;
        fragment::apply_promotion(&mut fragment, schema, overrides)?;
        self.update(fragment)
    }

    /// Link two fragments in both directions.
    pub fn link(&mut self, a: &str, b: &str) -> Result<LinkChange, ParcError> {
        let mut frag_a = fragment::read_fragment(&self.vault, a)?;
        let mut frag_b = fragment::read_fragment(&self.vault, b)?;
        if frag_a.id == frag_b.id {
            return Err(ParcError::ValidationError(
                "cannot link a fragment to itself".into(),
            ));
        }

        let a_changed = !frag_a.links.contains(&frag_b.id);
        let b_changed = !frag_b.links.contains(&frag_a.id);
        if a_changed {
            frag_a.links.push(frag_b.id.clone());
        }
        if b_changed {
            frag_b.links.push(frag_a.id.clone());
        }

        self.save_pair(frag_a, a_changed, frag_b, b_changed)
    }

    /// Remove the link between two fragments in both directions.
    pub fn unlink(&mut self, a: &str, b: &str) -> Result<LinkChange, ParcError> {
        let mut frag_a = fragment::read_fragment(&self.vault, a)?;
        let mut frag_b = fragment::read_fragment(&self.vault, b)?;

        let a_changed = frag_a.links.contains(&frag_b.id);
        let b_changed = frag_b.links.contains(&frag_a.id);
        frag_a.links.retain(|l| l != &frag_b.id);
        frag_b.links.retain(|l| l != &frag_a.id);

        self.save_pair(frag_a, a_changed, frag_b, b_changed)
    }

    /// Validate both sides before writing either, so a link never ends up
    /// half-applied because the second fragment was invalid.
    fn save_pair(
        &mut self,
        frag_a: Fragment,
        a_changed: bool,
        frag_b: Fragment,
        b_changed: bool,
    ) -> Result<LinkChange, ParcError> {
        if a_changed {
            fragment::validate_fragment_in_vault(&self.vault, &frag_a)?;
        }
        if b_changed {
            fragment::validate_fragment_in_vault(&self.vault, &frag_b)?;
        }
        let change = LinkChange {
            a: frag_a.id.clone(),
            b: frag_b.id.clone(),
            changed: a_changed || b_changed,
        };
        if a_changed {
            self.update(frag_a)?;
        }
        if b_changed {
            self.update(frag_b)?;
        }
        Ok(change)
    }

    fn conn(&mut self) -> Result<&Connection, ParcError> {
        if self.conn.is_none() {
            self.conn = Some(index::open_index(&self.vault)?);
        }
        Ok(self.conn.as_ref().expect("connection was just opened"))
    }

    fn index(&mut self, fragment: &Fragment) -> Result<(), ParcError> {
        let vault = self.vault.clone();
        index::index_fragment_auto(self.conn()?, fragment, &vault)
    }

    fn pre_hooks(&mut self, event: HookEvent, fragment: &Fragment) -> Result<Fragment, ParcError> {
        #[cfg(feature = "wasm-plugins")]
        if let Some(plugins) = self.plugins.as_mut() {
            return hook::run_pre_hooks_with_plugins(
                self.hooks.as_ref(),
                &self.vault,
                event,
                fragment,
                plugins,
            );
        }
        hook::run_pre_hooks(self.hooks.as_ref(), &self.vault, event, fragment)
    }

    fn post_hooks(&mut self, event: HookEvent, fragment: &Fragment) {
        #[cfg(feature = "wasm-plugins")]
        if let Some(plugins) = self.plugins.as_mut() {
            let warnings = hook::run_post_hooks_with_plugins(
                self.hooks.as_ref(),
                &self.vault,
                event,
                fragment,
                plugins,
            );
            self.warnings.extend(warnings);
            return;
        }
        let warnings = hook::run_post_hooks(self.hooks.as_ref(), &self.vault, event, fragment);
        self.warnings.extend(warnings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::HookScript;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Records every hook invocation as "<event>:<fragment title>".
    #[derive(Clone, Default)]
    struct Recorder {
        calls: Rc<RefCell<Vec<String>>>,
        fail_post: bool,
    }

    impl Recorder {
        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl HookRunner for Recorder {
        fn run_pre_hook(
            &self,
            script: &HookScript,
            fragment: &Fragment,
        ) -> Result<Option<Fragment>, ParcError> {
            self.calls
                .borrow_mut()
                .push(format!("{}:{}", script.event.prefix(), fragment.title));
            Ok(None)
        }

        fn run_post_hook(&self, script: &HookScript, fragment: &Fragment) -> Result<(), ParcError> {
            self.calls
                .borrow_mut()
                .push(format!("{}:{}", script.event.prefix(), fragment.title));
            if self.fail_post {
                return Err(ParcError::ValidationError("post-hook boom".into()));
            }
            Ok(())
        }
    }

    /// A vault with hooks enabled and a generic script for every event.
    fn vault_with_hooks() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        std::fs::write(vault.join("config.yml"), "hooks:\n  enabled: true\n").unwrap();
        for event in [
            HookEvent::PreCreate,
            HookEvent::PostCreate,
            HookEvent::PreUpdate,
            HookEvent::PostUpdate,
            HookEvent::PreDelete,
            HookEvent::PostDelete,
        ] {
            let path = vault.join("hooks").join(event.prefix());
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
        (tmp, vault)
    }

    fn new_note(vault: &Path, title: &str) -> Fragment {
        let config = crate::config::load_config(vault).unwrap();
        let schemas = crate::schema::load_schemas(vault).unwrap();
        fragment::new_fragment("note", title, schemas.resolve("note").unwrap(), &config)
    }

    fn new_todo(vault: &Path, title: &str) -> Fragment {
        let config = crate::config::load_config(vault).unwrap();
        let schemas = crate::schema::load_schemas(vault).unwrap();
        fragment::new_fragment("todo", title, schemas.resolve("todo").unwrap(), &config)
    }

    #[test]
    fn create_runs_hooks_writes_and_indexes() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());

        let frag = w.create(new_note(&vault, "Hello")).unwrap();

        assert_eq!(rec.calls(), vec!["pre-create:Hello", "post-create:Hello"]);
        assert_eq!(
            fragment::read_fragment(&vault, &frag.id).unwrap().title,
            "Hello"
        );
        let conn = index::open_index(&vault).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fragments WHERE id = ?1",
                [&frag.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn update_bumps_updated_at_and_runs_update_hooks() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());
        let created = w.create(new_note(&vault, "Before")).unwrap();

        let mut edited = created.clone();
        edited.title = "After".into();
        let saved = w.update(edited).unwrap();

        assert!(saved.updated_at >= created.updated_at);
        assert_eq!(
            rec.calls()[2..],
            [
                "pre-update:After".to_string(),
                "post-update:After".to_string()
            ]
        );
        assert_eq!(
            fragment::read_fragment(&vault, &created.id).unwrap().title,
            "After"
        );
    }

    #[test]
    fn update_rejects_invalid_fragment_before_hooks() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());
        let mut todo = w.create(new_todo(&vault, "Task")).unwrap();

        todo.extra_fields
            .insert("status".into(), Value::String("not-a-status".into()));
        assert!(w.update(todo).is_err());
        assert_eq!(
            rec.calls().len(),
            2,
            "no update hooks after failed validation"
        );
    }

    #[test]
    fn archive_runs_update_hooks() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());
        let created = w.create(new_note(&vault, "Old")).unwrap();

        let archived = w.set_archived(&created.id[..10], true).unwrap();
        assert_eq!(
            archived.extra_fields.get("archived"),
            Some(&Value::Bool(true))
        );
        let restored = w.set_archived(&created.id, false).unwrap();
        assert!(!restored.extra_fields.contains_key("archived"));

        let updates = rec
            .calls()
            .iter()
            .filter(|c| c.contains("-update:"))
            .count();
        assert_eq!(updates, 4);
    }

    #[test]
    fn delete_runs_delete_hooks_and_unindexes() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());
        let created = w.create(new_note(&vault, "Gone")).unwrap();

        let deleted = w.delete(&created.id[..10]).unwrap();

        assert_eq!(deleted.id, created.id);
        assert_eq!(
            rec.calls()[2..],
            [
                "pre-delete:Gone".to_string(),
                "post-delete:Gone".to_string()
            ]
        );
        assert!(fragment::read_fragment(&vault, &created.id).is_err());
        assert!(vault
            .join("trash")
            .join(format!("{}.md", created.id))
            .exists());
        let conn = index::open_index(&vault).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fragments WHERE id = ?1",
                [&created.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn link_and_unlink_update_both_sides_once() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());
        let a = w.create(new_note(&vault, "A")).unwrap();
        let b = w.create(new_note(&vault, "B")).unwrap();

        let change = w.link(&a.id, &b.id).unwrap();
        assert!(change.changed);
        assert_eq!(
            fragment::read_fragment(&vault, &a.id).unwrap().links,
            vec![b.id.clone()]
        );
        assert_eq!(
            fragment::read_fragment(&vault, &b.id).unwrap().links,
            vec![a.id.clone()]
        );

        assert!(
            !w.link(&a.id, &b.id).unwrap().changed,
            "linking twice is a no-op"
        );

        let change = w.unlink(&b.id, &a.id).unwrap();
        assert!(change.changed);
        assert!(fragment::read_fragment(&vault, &a.id)
            .unwrap()
            .links
            .is_empty());
        assert!(!w.unlink(&a.id, &b.id).unwrap().changed);

        // create A, create B, link (A, B), unlink (B, A) — each save has pre+post.
        let updates = rec
            .calls()
            .iter()
            .filter(|c| c.contains("-update:"))
            .count();
        assert_eq!(updates, 8);
    }

    #[test]
    fn link_to_self_is_rejected() {
        let (_tmp, vault) = vault_with_hooks();
        let mut w = Writer::new(&vault, Recorder::default());
        let a = w.create(new_note(&vault, "A")).unwrap();
        assert!(w.link(&a.id, &a.id).is_err());
    }

    #[test]
    fn promote_saves_as_new_type() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());
        let note = w.create(new_note(&vault, "Idea")).unwrap();

        let promoted = w.promote(&note.id, "todo", BTreeMap::new()).unwrap();

        assert_eq!(promoted.fragment_type, "todo");
        assert_eq!(
            fragment::read_fragment(&vault, &note.id)
                .unwrap()
                .fragment_type,
            "todo"
        );
        assert!(rec.calls().contains(&"post-update:Idea".to_string()));
    }

    #[test]
    fn post_hook_failures_become_warnings() {
        let (_tmp, vault) = vault_with_hooks();
        let rec = Recorder {
            fail_post: true,
            ..Recorder::default()
        };
        let mut w = Writer::new(&vault, rec);

        let frag = w.create(new_note(&vault, "Still saved")).unwrap();

        assert!(fragment::read_fragment(&vault, &frag.id).is_ok());
        let warnings = w.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("post-hook boom"));
        assert!(w.take_warnings().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn script_runner_applies_pre_hook_output_and_captures_stderr() {
        use crate::hook::{HookStderr, ScriptRunner};
        use std::os::unix::fs::PermissionsExt;

        let (_tmp, vault) = vault_with_hooks();
        // Pre-create hook rewrites the title via sed on the JSON.
        let pre = vault.join("hooks").join("pre-create");
        std::fs::write(
            &pre,
            "#!/bin/sh\nsed 's/\"title\":\"draft\"/\"title\":\"final\"/'\n",
        )
        .unwrap();
        std::fs::set_permissions(&pre, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Post-create hook fails loudly.
        let post = vault.join("hooks").join("post-create");
        std::fs::write(&post, "#!/bin/sh\necho 'nope' >&2\nexit 3\n").unwrap();
        std::fs::set_permissions(&post, std::fs::Permissions::from_mode(0o700)).unwrap();

        let mut w = Writer::new(&vault, ScriptRunner::new(HookStderr::Capture));
        let frag = w.create(new_note(&vault, "draft")).unwrap();

        assert_eq!(frag.title, "final");
        assert_eq!(
            fragment::read_fragment(&vault, &frag.id).unwrap().title,
            "final"
        );
        let warnings = w.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("exit 3"), "{}", warnings[0]);
        assert!(warnings[0].contains("nope"), "{}", warnings[0]);
    }

    #[cfg(unix)]
    #[test]
    fn script_runner_failing_pre_hook_aborts_save() {
        use crate::hook::{HookStderr, ScriptRunner};
        use std::os::unix::fs::PermissionsExt;

        let (_tmp, vault) = vault_with_hooks();
        let pre = vault.join("hooks").join("pre-create");
        std::fs::write(&pre, "#!/bin/sh\necho 'rejected' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&pre, std::fs::Permissions::from_mode(0o700)).unwrap();

        let mut w = Writer::new(&vault, ScriptRunner::new(HookStderr::Capture));
        let note = new_note(&vault, "Nope");
        let id = note.id.clone();
        let err = w.create(note).unwrap_err().to_string();

        assert!(err.contains("rejected"), "{}", err);
        assert!(fragment::read_fragment(&vault, &id).is_err());
    }
}
