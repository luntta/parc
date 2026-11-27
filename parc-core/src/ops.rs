//! Fragment write operations.
//!
//! Every frontend (CLI, TUI, server) changes fragments through [`Writer`], so
//! a save runs the same steps no matter where it came from:
//!
//! 1. validate against the type's schema (unknown custom types are accepted);
//!    a new fragment's type, or a converted one's, must not be turned off
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

use crate::attachment::{self, AttachMode};
use crate::error::ParcError;
use crate::fragment::{self, Fragment};
use crate::hook::{self, HookEvent, HookRunner};
use crate::index;
use crate::secure_fs;

/// Result of a link or unlink: the two full IDs, and whether either
/// fragment actually changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkChange {
    pub a: String,
    pub b: String,
    pub changed: bool,
}

/// Move the fragments holding `from` in a choice field to `to`, when a
/// schema renames or removes a value. See [`Writer::save_schema`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueRename {
    pub field: String,
    pub from: String,
    pub to: String,
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

    /// Save a new fragment. Fails if a fragment with the same ID exists, or
    /// if its type is turned off in this vault.
    pub fn create(&mut self, fragment: Fragment) -> Result<Fragment, ParcError> {
        crate::config::load_config(&self.vault)?
            .types
            .check_enabled(&fragment.fragment_type)?;
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
        self.rewrite(fragment)
    }

    /// [`Writer::update`] without touching `updated_at`: for changes parc
    /// makes on the user's behalf, such as renaming a status in every
    /// fragment of a type, which shouldn't make them look freshly edited.
    fn rewrite(&mut self, fragment: Fragment) -> Result<Fragment, ParcError> {
        let fragment = self.prepare_update(&fragment)?;
        fragment::write_fragment(&self.vault, &fragment)?;
        self.index(&fragment)?;
        self.post_hooks(HookEvent::PostUpdate, &fragment);
        Ok(fragment)
    }

    fn prepare_update(&mut self, fragment: &Fragment) -> Result<Fragment, ParcError> {
        self.check_type_change(fragment)?;
        fragment::validate_fragment_in_vault(&self.vault, fragment)?;
        self.pre_hooks(HookEvent::PreUpdate, fragment)
    }

    pub(crate) fn warn(&mut self, message: String) {
        self.warnings.push(message);
    }

    // An index error after a file operation commits must never trigger rollback of the original.
    fn finish_attachment_update(&mut self, fragment: &Fragment) {
        if let Err(e) = self.index(fragment) {
            self.warn(format!(
                "Saved {}, but could not update its index: {e}",
                fragment.id
            ));
        }
        self.post_hooks(HookEvent::PostUpdate, fragment);
    }

    /// A fragment may keep a type that is turned off, but may not change to
    /// one: whichever frontend converts it (promote, an edited `type:` line).
    fn check_type_change(&self, fragment: &Fragment) -> Result<(), ParcError> {
        let types = crate::config::load_config(&self.vault)?.types;
        if types.is_enabled(&fragment.fragment_type) {
            return Ok(());
        }
        let path = self
            .vault
            .join("fragments")
            .join(format!("{}.md", fragment.id));
        let saved_type = std::fs::read(&path)
            .ok()
            .and_then(|bytes| fragment::parse_fragment_file(&fragment.id, &bytes).ok())
            .map(|saved| saved.fragment_type);
        if saved_type.as_deref() == Some(fragment.fragment_type.as_str()) {
            return Ok(());
        }
        types.check_enabled(&fragment.fragment_type)
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

    /// Move a fragment back from the trash and index it. Runs no hooks: there is no
    /// restore event, matching history restore. `id_or_prefix` matches trashed IDs only.
    pub fn restore_from_trash(&mut self, id_or_prefix: &str) -> Result<Fragment, ParcError> {
        let id = self.resolve_trash_id(id_or_prefix)?;
        let trash = self.vault.join("trash");
        let dest = self.vault.join("fragments").join(format!("{}.md", id));
        if dest.exists() {
            return Err(ParcError::ValidationError(format!(
                "fragment {} already exists outside the trash",
                id
            )));
        }
        crate::secure_fs::rename_private_file(&trash.join(format!("{}.md", id)), &dest)?;
        let fragment = fragment::read_fragment(&self.vault, &id)?;
        self.index(&fragment)?;
        let vault = self.vault.clone();
        let _ = crate::extract::sync_attachments(self.conn()?, &vault);
        Ok(fragment)
    }

    /// Permanently remove one trashed fragment along with its `attachments/<id>/`
    /// (including `.text/` sidecars) and `history/<id>/` directories. Refuses symlinks.
    pub fn purge(&mut self, id_or_prefix: &str) -> Result<String, ParcError> {
        let id = self.resolve_trash_id(id_or_prefix)?;
        self.purge_trash_id(&id)?;
        Ok(id)
    }

    /// Permanently remove every valid trashed fragment, along with its
    /// `attachments/<id>/` and `history/<id>/` directories. Returns how many
    /// fragments were purged.
    pub fn empty_trash(&mut self) -> Result<usize, ParcError> {
        let trash_dir = self.vault.join("trash");
        if !trash_dir.is_dir() {
            return Ok(0);
        }
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&trash_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "md") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if fragment::validate_id(stem).is_err() {
                continue;
            }
            if let Ok(meta) = std::fs::symlink_metadata(&path) {
                if !meta.file_type().is_symlink() && meta.is_file() {
                    ids.push(stem.to_string());
                }
            }
        }
        ids.sort();
        let mut count = 0;
        for id in ids {
            self.purge_trash_id(&id)?;
            count += 1;
        }
        Ok(count)
    }

    fn resolve_trash_id(&self, id_or_prefix: &str) -> Result<String, ParcError> {
        let upper = id_or_prefix.to_uppercase();
        let trash = self.vault.join("trash");
        let mut matches = Vec::new();
        if trash.is_dir() {
            for entry in std::fs::read_dir(&trash)? {
                let path = entry?.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if path.extension().is_some_and(|e| e == "md")
                    && stem.starts_with(&upper)
                    && fragment::validate_id(stem).is_ok()
                {
                    matches.push(stem.to_string());
                }
            }
        }
        match matches.as_slice() {
            [id] => Ok(id.clone()),
            [] => Err(ParcError::FragmentNotFound(format!(
                "{} (not in the trash)",
                id_or_prefix
            ))),
            _ => Err(ParcError::AmbiguousId(
                id_or_prefix.to_string(),
                matches.len(),
            )),
        }
    }

    fn purge_trash_id(&mut self, id: &str) -> Result<(), ParcError> {
        fragment::validate_id(id)?;
        let trash_file = self.vault.join("trash").join(format!("{}.md", id));
        // Validate attachment and history trees before removing the trash file
        // so a symlink inside them aborts cleanly.
        let attach_dir = self.vault.join("attachments").join(id);
        let history_dir = self.vault.join("history").join(id);
        secure_fs::remove_private_dir_all(&attach_dir)?;
        secure_fs::remove_private_dir_all(&history_dir)?;
        secure_fs::remove_private_file(&trash_file)?;
        Ok(())
    }

    /// Attach a file to a fragment (copy or move), updating frontmatter through
    /// the writer so hooks, history, indexing, and attachment text extraction all run.
    pub fn attach(
        &mut self,
        id_or_prefix: &str,
        source_path: &Path,
        mode: AttachMode,
    ) -> Result<(Fragment, String), ParcError> {
        let (frag, filename, _) = self.attach_and_extract(id_or_prefix, source_path, mode)?;
        Ok((frag, filename))
    }

    /// Like [`Writer::attach`], also returning the extracted attachment text
    /// (including document title metadata) if extraction produced text.
    pub fn attach_and_extract(
        &mut self,
        id_or_prefix: &str,
        source_path: &Path,
        mode: AttachMode,
    ) -> Result<(Fragment, String, Option<crate::extract::Extracted>), ParcError> {
        let (frag, filename) = self.attach_file_only(id_or_prefix, source_path, mode)?;
        let extracted = self.extract_attached(&frag.id, &filename);
        Ok((frag, filename, extracted))
    }

    pub(crate) fn extract_attached(
        &mut self,
        id: &str,
        filename: &str,
    ) -> Option<crate::extract::Extracted> {
        let vault = self.vault.clone();
        match self
            .conn()
            .and_then(|conn| crate::extract::fill_one(&vault, conn, id, filename))
        {
            Ok(extracted) => extracted,
            Err(err) => {
                self.warn(format!(
                    "Kept {filename}; text indexing failed: {err}. Retry with parc reindex."
                ));
                None
            }
        }
    }

    /// Commit the original independently of derived text. Run vetoable hooks
    /// before moving bytes; never overwrite another incoming file in rollback.
    pub(crate) fn attach_file_only(
        &mut self,
        id: &str,
        source: &Path,
        mode: AttachMode,
    ) -> Result<(Fragment, String), ParcError> {
        let mut frag = fragment::read_fragment(&self.vault, id)?;
        let filename = source
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| ParcError::ValidationError("invalid filename".into()))?
            .to_owned();
        attachment::validate_attachment_filename(&filename)?;
        if !frag.attachments.contains(&filename) {
            frag.attachments.push(filename.clone());
        }
        frag.updated_at = Utc::now();
        let saved = self.prepare_update(&frag)?;
        if saved.id != frag.id || !saved.attachments.contains(&filename) {
            return Err(ParcError::ValidationError(
                "pre-update hook changed attachment ownership".into(),
            ));
        }
        let path = self
            .vault
            .join("fragments")
            .join(format!("{}.md", saved.id));
        let before = std::fs::read(&path)?;
        fragment::write_fragment(&self.vault, &saved)?;
        if let Err(err) = attachment::store_attachment_file(&self.vault, &saved.id, source, mode) {
            secure_fs::write_private_atomic(&path, before)?;
            return Err(err);
        }
        self.finish_attachment_update(&saved);
        Ok((saved, filename))
    }

    /// Detach an attachment from a fragment, updating frontmatter through the
    /// writer before deleting the file, its `.text/` sidecar, and its index rows.
    pub fn detach(&mut self, id_or_prefix: &str, filename: &str) -> Result<Fragment, ParcError> {
        attachment::validate_attachment_filename(filename)?;
        let mut frag = fragment::read_fragment(&self.vault, id_or_prefix)?;
        let full_id = frag.id.clone();
        frag.attachments.retain(|a| a != filename);
        let saved = self.update(frag)?;
        attachment::remove_attachment_file(&self.vault, &full_id, filename)?;
        crate::extract::store::remove_one_attachment(self.conn()?, &full_id, filename)?;
        Ok(saved)
    }

    /// Move an attachment (and its `.text/` sidecar) from one fragment to
    /// another, updating both fragments through the writer.
    pub fn move_attachment(
        &mut self,
        from_id_or_prefix: &str,
        to_id_or_prefix: &str,
        filename: &str,
    ) -> Result<(Fragment, Fragment), ParcError> {
        attachment::validate_attachment_filename(filename)?;
        let mut from = fragment::read_fragment(&self.vault, from_id_or_prefix)?;
        let mut to = fragment::read_fragment(&self.vault, to_id_or_prefix)?;
        if from.id == to.id {
            return Err(ParcError::ValidationError(
                "cannot move an attachment onto the same fragment".into(),
            ));
        }
        let from_id = from.id.clone();
        let to_id = to.id.clone();

        if !to.attachments.contains(&filename.to_string()) {
            to.attachments.push(filename.to_string());
        }
        from.attachments.retain(|a| a != filename);

        from.updated_at = Utc::now();
        to.updated_at = from.updated_at;
        let saved_to = self.prepare_update(&to)?;
        let saved_from = self.prepare_update(&from)?;
        if saved_to.id != to_id
            || saved_from.id != from_id
            || !saved_to.attachments.iter().any(|a| a == filename)
            || saved_from.attachments.iter().any(|a| a == filename)
        {
            return Err(ParcError::ValidationError(
                "pre-update hook changed attachment ownership".into(),
            ));
        }

        // Stage atomic frontmatter writes before moving the original. Roll back
        // exact pre-move bytes without invoking hooks a second time.
        let path = |id: &str| self.vault.join("fragments").join(format!("{id}.md"));
        let (to_path, from_path) = (path(&to_id), path(&from_id));
        let (to_before, from_before) = (std::fs::read(&to_path)?, std::fs::read(&from_path)?);
        fragment::write_fragment(&self.vault, &saved_to)?;
        if let Err(err) = fragment::write_fragment(&self.vault, &saved_from) {
            secure_fs::write_private_atomic(&to_path, to_before)?;
            return Err(err);
        }
        if let Err(err) = attachment::move_attachment_file(&self.vault, &from_id, &to_id, filename)
        {
            let restore_from = secure_fs::write_private_atomic(&from_path, from_before);
            let restore_to = secure_fs::write_private_atomic(&to_path, to_before);
            restore_from?;
            restore_to?;
            return Err(err);
        }

        if let Err(err) = attachment::move_sidecars(&self.vault, &from_id, &to_id, filename) {
            self.warn(format!(
                "Moved {filename}, but its derived text needs rebuilding: {err}"
            ));
        }
        self.finish_attachment_update(&saved_to);
        self.finish_attachment_update(&saved_from);
        if let Err(err) = self.conn().and_then(|c| {
            crate::extract::store::move_indexed_attachment(c, &from_id, &to_id, filename)
        }) {
            self.warn(format!(
                "Moved {filename}, but its old index needs rebuilding: {err}"
            ));
        }
        Ok((saved_from, saved_to))
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
        crate::config::load_config(&self.vault)?
            .types
            .check_enabled(&schema.name)?;
        let mut fragment = fragment::read_fragment(&self.vault, id_or_prefix)?;
        fragment::apply_promotion(&mut fragment, schema, overrides)?;
        self.update(fragment)
    }

    /// Write a type's schema and bring its fragments along: each
    /// [`ValueRename`] moves the fragments holding `from` in `field` to `to`.
    /// Nothing is written if the change would leave a fragment that is valid
    /// now invalid (a removed value nobody moved, a new required field
    /// without a default...); the error names them. Fragments that were
    /// already invalid are left as they are.
    ///
    /// Migrated fragments keep their `updated_at` and go through the usual
    /// save (hooks, history, index). Returns how many changed; a fragment that
    /// fails to save (a pre-hook veto) becomes a warning, since the schema is
    /// already written by then.
    pub fn save_schema(
        &mut self,
        schema: &crate::schema::Schema,
        create: bool,
        renames: &[ValueRename],
    ) -> Result<usize, ParcError> {
        use crate::schema::FieldType;

        crate::schema::validate_schema(schema)?;
        for rename in renames {
            let target = schema
                .field(&rename.field)
                .and_then(|f| match &f.field_type {
                    FieldType::Enum(values) => Some(values),
                    _ => None,
                })
                .ok_or_else(|| {
                    ParcError::ValidationError(format!(
                        "can't move values of '{}': it isn't a choice field of '{}'",
                        rename.field, schema.name
                    ))
                })?;
            if !target.contains(&rename.to) {
                return Err(ParcError::ValidationError(format!(
                    "can't move '{}' to '{}': it isn't a value of field '{}'",
                    rename.from, rename.to, rename.field
                )));
            }
        }

        let old = crate::schema::load_schemas(&self.vault)?
            .get_by_name(&schema.name)
            .cloned();
        let mut changed = Vec::new();
        let mut stranded = Vec::new();
        if !create {
            for mut fragment in fragment::read_all(&self.vault)? {
                if fragment.fragment_type != schema.name {
                    continue;
                }
                let valid_before = old
                    .as_ref()
                    .is_none_or(|o| fragment::validate_fragment(&fragment, o).is_ok());
                // One rename per field, so swapping two values (a→b, b→a) doesn't chain.
                let mut moved = std::collections::BTreeSet::new();
                for rename in renames {
                    if moved.contains(&rename.field) {
                        continue;
                    }
                    let field = fragment.extra_fields.get_mut(&rename.field);
                    if let Some(value) = field.filter(|v| v.as_str() == Some(rename.from.as_str()))
                    {
                        *value = Value::String(rename.to.clone());
                        moved.insert(rename.field.clone());
                    }
                }
                let touched = !moved.is_empty();
                match fragment::validate_fragment(&fragment, schema) {
                    Ok(()) if touched => changed.push(fragment),
                    Ok(()) => {}
                    Err(e) if valid_before => stranded.push((fragment, e)),
                    Err(_) => {}
                }
            }
        }
        if !stranded.is_empty() {
            let named: Vec<String> = stranded
                .iter()
                .take(3)
                .map(|(f, e)| {
                    let title = if f.title.is_empty() {
                        "Untitled"
                    } else {
                        &f.title
                    };
                    format!("“{}” ({})", title, e)
                })
                .collect();
            let more = stranded.len().saturating_sub(named.len());
            return Err(ParcError::ValidationError(format!(
                "this change would make {} {} invalid: {}{}",
                stranded.len(),
                if stranded.len() == 1 {
                    "fragment"
                } else {
                    "fragments"
                },
                named.join("; "),
                if more > 0 {
                    format!("; and {} more", more)
                } else {
                    String::new()
                }
            )));
        }

        crate::schema::write_schema(&self.vault, schema, create)?;
        let mut migrated = 0;
        for fragment in changed {
            let id = fragment.id.clone();
            match self.rewrite(fragment) {
                Ok(_) => migrated += 1,
                Err(e) => self
                    .warnings
                    .push(format!("{} keeps its old value: {}", id, e)),
            }
        }
        Ok(migrated)
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

    fn plain_vault() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        (tmp, vault)
    }

    fn turn_off(vault: &Path, name: &str) {
        let mut types = crate::config::load_config(vault).unwrap().types;
        types.disabled.push(name.to_string());
        crate::config::save_types(vault, &types).unwrap();
    }

    #[test]
    fn a_turned_off_type_takes_no_new_fragments_but_keeps_its_old_ones() {
        let (_tmp, vault) = plain_vault();
        let mut w = Writer::new(&vault, Recorder::default());
        let old = w.create(new_todo(&vault, "Before")).unwrap();
        let note = w.create(new_note(&vault, "A note")).unwrap();
        turn_off(&vault, "todo");

        let err = w.create(new_todo(&vault, "After")).unwrap_err().to_string();
        assert!(err.contains("turned off"), "{}", err);
        let err = w
            .promote(&note.id, "todo", BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("turned off"), "{}", err);

        // The existing todo still edits, and still validates against its schema.
        let mut edited = old.clone();
        edited
            .extra_fields
            .insert("status".into(), Value::String("done".into()));
        w.update(edited).unwrap();
        let mut bad = old.clone();
        bad.extra_fields
            .insert("status".into(), Value::String("nope".into()));
        assert!(w.update(bad).is_err());
    }

    fn todo_with(vault: &Path, w: &mut Writer, title: &str, status: &str) -> Fragment {
        let mut f = new_todo(vault, title);
        f.extra_fields
            .insert("status".into(), Value::String(status.into()));
        w.create(f).unwrap()
    }

    fn status_of(vault: &Path, id: &str) -> String {
        fragment::read_fragment(vault, id).unwrap().extra_fields["status"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn save_schema_renames_values_in_fragments_without_touching_updated_at() {
        let (_tmp, vault) = plain_vault();
        let mut w = Writer::new(&vault, Recorder::default());
        let doing = todo_with(&vault, &mut w, "Doing", "in-progress");
        let open = todo_with(&vault, &mut w, "Open", "open");
        let before = fragment::read_fragment(&vault, &doing.id)
            .unwrap()
            .updated_at;

        let mut schema = crate::schema::load_schemas(&vault)
            .unwrap()
            .get_by_name("todo")
            .unwrap()
            .clone();
        let status = schema
            .fields
            .iter_mut()
            .find(|f| f.name == "status")
            .unwrap();
        status.field_type = crate::schema::FieldType::Enum(vec![
            "open".into(),
            "doing".into(),
            "done".into(),
            "cancelled".into(),
        ]);
        status.categories.as_mut().unwrap().remove("in-progress");
        status
            .categories
            .as_mut()
            .unwrap()
            .insert("doing".into(), crate::schema::StatusCategory::Active);
        let rename = ValueRename {
            field: "status".into(),
            from: "in-progress".into(),
            to: "doing".into(),
        };

        assert_eq!(w.save_schema(&schema, false, &[rename]).unwrap(), 1);
        assert_eq!(status_of(&vault, &doing.id), "doing");
        assert_eq!(status_of(&vault, &open.id), "open");
        let after = fragment::read_fragment(&vault, &doing.id).unwrap();
        assert_eq!(after.updated_at, before);
        let saved = crate::schema::load_schemas(&vault).unwrap();
        let todo = saved.get_by_name("todo").unwrap();
        assert_eq!(
            todo.status_category("doing"),
            crate::schema::StatusCategory::Active
        );
        // The index follows: the renamed value is what a search finds.
        let conn = index::open_index(&vault).unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM fragments WHERE id = ?1",
                [&doing.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "doing");
    }

    #[test]
    fn save_schema_refuses_to_strand_fragments() {
        let (_tmp, vault) = plain_vault();
        let mut w = Writer::new(&vault, Recorder::default());
        let cancelled = todo_with(&vault, &mut w, "Dropped", "cancelled");
        let path = vault.join("schemas/todo.yml");
        let original = std::fs::read_to_string(&path).unwrap();

        let mut schema = crate::schema::load_schemas(&vault)
            .unwrap()
            .get_by_name("todo")
            .unwrap()
            .clone();
        let status = schema
            .fields
            .iter_mut()
            .find(|f| f.name == "status")
            .unwrap();
        status.field_type = crate::schema::FieldType::Enum(vec![
            "open".into(),
            "in-progress".into(),
            "done".into(),
        ]);
        status.categories.as_mut().unwrap().remove("cancelled");

        let err = w.save_schema(&schema, false, &[]).unwrap_err().to_string();
        assert!(err.contains("1 fragment invalid"), "{}", err);
        assert!(err.contains("Dropped"), "{}", err);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        // Moving them to a value that stays makes the same change go through.
        let to_done = ValueRename {
            field: "status".into(),
            from: "cancelled".into(),
            to: "done".into(),
        };
        assert_eq!(w.save_schema(&schema, false, &[to_done]).unwrap(), 1);
        assert_eq!(status_of(&vault, &cancelled.id), "done");

        // Swapping two values moves each fragment once.
        let open = todo_with(&vault, &mut w, "Open", "open");
        let swap = [
            ValueRename {
                field: "status".into(),
                from: "open".into(),
                to: "done".into(),
            },
            ValueRename {
                field: "status".into(),
                from: "done".into(),
                to: "open".into(),
            },
        ];
        assert_eq!(w.save_schema(&schema, false, &swap).unwrap(), 2);
        assert_eq!(status_of(&vault, &open.id), "done");
        assert_eq!(status_of(&vault, &cancelled.id), "open");

        // A rename into a value the schema doesn't have is refused up front.
        let stray = ValueRename {
            field: "status".into(),
            from: "open".into(),
            to: "later".into(),
        };
        assert!(w.save_schema(&schema, false, &[stray]).is_err());
    }

    #[test]
    fn save_schema_creates_a_new_type_that_takes_fragments() {
        let (_tmp, vault) = plain_vault();
        let mut w = Writer::new(&vault, Recorder::default());
        let schema = crate::schema::parse_schema(
            "name: bug\nalias: b\nfields:\n  - name: status\n    type: enum\n    values: [new, fixed]\n    default: new\n    categories:\n      done: [fixed]\n",
        )
        .unwrap();
        assert_eq!(w.save_schema(&schema, true, &[]).unwrap(), 0);
        assert!(vault.join("schemas/bug.yml").exists());
        assert!(vault.join("templates/bug.md").exists());
        assert!(w.save_schema(&schema, true, &[]).is_err(), "exists already");

        let schemas = crate::schema::load_schemas(&vault).unwrap();
        let bug = schemas.resolve("b").unwrap();
        let config = crate::config::load_config(&vault).unwrap();
        let frag = w
            .create(fragment::new_fragment("bug", "Crash", bug, &config))
            .unwrap();
        assert_eq!(frag.extra_fields["status"], Value::String("new".into()));
    }

    #[test]
    fn writer_attach_detach_and_move_run_hooks_history_and_index() {
        let (tmp, vault) = vault_with_hooks();
        let rec = Recorder::default();
        let mut w = Writer::new(&vault, rec.clone());
        let a = w.create(new_note(&vault, "First")).unwrap();
        let b = w.create(new_note(&vault, "Second")).unwrap();

        let src = tmp.path().join("report.txt");
        std::fs::write(&src, "hello report").unwrap();

        let (saved_a, name) = w.attach(&a.id, &src, AttachMode::Copy).unwrap();
        assert_eq!(name, "report.txt");
        assert_eq!(saved_a.attachments, vec!["report.txt"]);
        assert!(src.exists(), "copy keeps source");
        assert!(vault
            .join("attachments")
            .join(&a.id)
            .join("report.txt")
            .exists());
        // History snapshot taken before attach update
        assert!(!crate::history::list_versions(&vault, &a.id)
            .unwrap()
            .is_empty());
        // Pre- and post-update hooks ran
        assert!(rec.calls().contains(&"pre-update:First".to_string()));
        assert!(rec.calls().contains(&"post-update:First".to_string()));
        // Index attachment_count updated
        let conn = index::open_index(&vault).unwrap();
        let att_count: i64 = conn
            .query_row(
                "SELECT attachment_count FROM fragments WHERE id = ?1",
                [&a.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(att_count, 1);

        // Create a sidecar next to the attachment and verify move_attachment moves both
        let sidecar = attachment::sidecar_dir(&vault, &a.id).join("report.txt.txt");
        secure_fs::create_private_dir_all(sidecar.parent().unwrap()).unwrap();
        secure_fs::write_private(&sidecar, "extracted").unwrap();

        let (from_after, to_after) = w.move_attachment(&a.id, &b.id, "report.txt").unwrap();
        assert!(from_after.attachments.is_empty());
        assert_eq!(to_after.attachments, vec!["report.txt"]);
        assert!(!vault.join("attachments").join(&a.id).exists());
        assert!(vault
            .join("attachments")
            .join(&b.id)
            .join("report.txt")
            .exists());
        assert!(attachment::sidecar_dir(&vault, &b.id)
            .join("report.txt.txt")
            .exists());

        // Detach removes both file and sidecar
        let after_detach = w.detach(&b.id, "report.txt").unwrap();
        assert!(after_detach.attachments.is_empty());
        assert!(!vault.join("attachments").join(&b.id).exists());
    }

    #[cfg(unix)]
    #[test]
    fn writer_attach_move_rolls_back_on_pre_hook_veto() {
        use crate::hook::{HookStderr, ScriptRunner};
        use std::os::unix::fs::PermissionsExt;

        let (tmp, vault) = vault_with_hooks();
        let mut w = Writer::new(&vault, ScriptRunner::new(HookStderr::Capture));
        let note = w.create(new_note(&vault, "Keep")).unwrap();

        let pre = vault.join("hooks").join("pre-update");
        std::fs::write(&pre, "#!/bin/sh\necho 'vetoed' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&pre, std::fs::Permissions::from_mode(0o700)).unwrap();

        let src = tmp.path().join("incoming.txt");
        std::fs::write(&src, "data").unwrap();
        let err = w.attach(&note.id, &src, AttachMode::Move).unwrap_err();
        assert!(err.to_string().contains("vetoed"), "{err}");
        assert!(src.exists(), "source restored when pre-hook vetoes move");
        assert!(!vault
            .join("attachments")
            .join(&note.id)
            .join("incoming.txt")
            .exists());
    }

    #[test]
    fn source_veto_and_destination_collision_leave_move_retryable() {
        #[derive(Default)]
        struct VetoSource;
        impl HookRunner for VetoSource {
            fn run_pre_hook(
                &self,
                script: &HookScript,
                f: &Fragment,
            ) -> Result<Option<Fragment>, ParcError> {
                if script.event == HookEvent::PreUpdate
                    && f.title == "From"
                    && f.attachments.is_empty()
                {
                    return Err(ParcError::ValidationError("source veto".into()));
                }
                Ok(None)
            }
            fn run_post_hook(&self, _: &HookScript, _: &Fragment) -> Result<(), ParcError> {
                Ok(())
            }
        }
        let (tmp, vault) = vault_with_hooks();
        let mut w = Writer::new(&vault, VetoSource);
        let from = w.create(new_note(&vault, "From")).unwrap();
        let to = w.create(new_note(&vault, "To")).unwrap();
        let src = tmp.path().join("memo.txt");
        std::fs::write(&src, "original").unwrap();
        w.attach(&from.id, &src, AttachMode::Copy).unwrap();
        let revision = |id| fragment::revision(&vault, id).unwrap();
        let revs = (revision(&from.id), revision(&to.id));
        let original = vault.join("attachments").join(&from.id).join("memo.txt");
        let dest = vault.join("attachments").join(&to.id).join("memo.txt");
        assert!(w
            .move_attachment(&from.id, &to.id, "memo.txt")
            .unwrap_err()
            .to_string()
            .contains("source veto"));
        assert_eq!((revision(&from.id), revision(&to.id)), revs);
        assert!(!dest.exists());
        let mut w = Writer::new(&vault, Recorder::default());
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "existing").unwrap();
        assert!(w.move_attachment(&from.id, &to.id, "memo.txt").is_err());
        assert_eq!((revision(&from.id), revision(&to.id)), revs);
        assert_eq!(std::fs::read(&dest).unwrap(), b"existing");
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
        std::fs::remove_file(&dest).unwrap();
        w.move_attachment(&from.id, &to.id, "memo.txt").unwrap();
        assert!(!original.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), b"original");
    }

    #[test]
    fn writer_purge_and_empty_trash_remove_attachments_and_history() {
        let (tmp, vault) = plain_vault();
        let mut w = Writer::new(&vault, Recorder::default());
        let a = w.create(new_note(&vault, "One")).unwrap();
        let b = w.create(new_note(&vault, "Two")).unwrap();

        let f1 = tmp.path().join("a.txt");
        let f2 = tmp.path().join("b.txt");
        std::fs::write(&f1, "alpha").unwrap();
        std::fs::write(&f2, "beta").unwrap();
        w.attach(&a.id, &f1, AttachMode::Copy).unwrap();
        w.attach(&b.id, &f2, AttachMode::Copy).unwrap();
        // Add a .text sidecar too
        let sidecar = attachment::sidecar_dir(&vault, &a.id).join("a.txt.txt");
        secure_fs::create_private_dir_all(sidecar.parent().unwrap()).unwrap();
        secure_fs::write_private(&sidecar, "text").unwrap();

        assert!(vault.join("history").join(&a.id).is_dir());
        assert!(vault.join("attachments").join(&a.id).is_dir());

        // Soft-delete keeps attachments and history so restore works
        w.delete(&a.id).unwrap();
        assert!(vault.join("attachments").join(&a.id).is_dir());
        assert!(vault.join("history").join(&a.id).is_dir());

        // Restore brings it back intact
        w.restore_from_trash(&a.id[..8]).unwrap();
        assert!(vault
            .join("fragments")
            .join(format!("{}.md", a.id))
            .exists());

        // Delete again and purge one
        w.delete(&a.id).unwrap();
        let purged = w.purge(&a.id[..8]).unwrap();
        assert_eq!(purged, a.id);
        assert!(!vault.join("trash").join(format!("{}.md", a.id)).exists());
        assert!(!vault.join("attachments").join(&a.id).exists());
        assert!(!vault.join("history").join(&a.id).exists());

        // Delete second and empty trash
        w.delete(&b.id).unwrap();
        assert_eq!(w.empty_trash().unwrap(), 1);
        assert!(!vault.join("trash").join(format!("{}.md", b.id)).exists());
        assert!(!vault.join("attachments").join(&b.id).exists());
        assert!(!vault.join("history").join(&b.id).exists());
    }

    #[cfg(unix)]
    #[test]
    fn writer_purge_refuses_symlinks() {
        use std::os::unix::fs::symlink;

        let (tmp, vault) = plain_vault();
        let mut w = Writer::new(&vault, Recorder::default());
        let a = w.create(new_note(&vault, "Sym")).unwrap();
        w.delete(&a.id).unwrap();

        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("precious.txt"), "keep").unwrap();
        symlink(&outside, vault.join("attachments").join(&a.id)).unwrap();

        assert!(w.purge(&a.id).is_err());
        assert!(outside.join("precious.txt").exists());
        assert!(vault.join("trash").join(format!("{}.md", a.id)).exists());
    }
}
