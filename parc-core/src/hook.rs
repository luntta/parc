use std::path::{Path, PathBuf};

use crate::error::ParcError;
use crate::fragment::Fragment;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    PreCreate,
    PostCreate,
    PreUpdate,
    PostUpdate,
    PreDelete,
    PostDelete,
}

impl HookEvent {
    pub fn prefix(&self) -> &'static str {
        match self {
            HookEvent::PreCreate => "pre-create",
            HookEvent::PostCreate => "post-create",
            HookEvent::PreUpdate => "pre-update",
            HookEvent::PostUpdate => "post-update",
            HookEvent::PreDelete => "pre-delete",
            HookEvent::PostDelete => "post-delete",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookScript {
    pub path: PathBuf,
    pub event: HookEvent,
    pub type_filter: Option<String>,
}

/// Discover hooks for a given event and fragment type.
/// Returns generic hooks first, then type-specific hooks.
/// Hook files are named: `<event>` (generic) or `<event>.<type>` (type-specific).
pub fn discover_hooks(vault: &Path, event: HookEvent, fragment_type: &str) -> Vec<HookScript> {
    let hooks_dir = vault.join("hooks");
    if !hooks_dir.is_dir() {
        return Vec::new();
    }

    let prefix = event.prefix();
    let mut hooks = Vec::new();

    // Generic hook: exact match on event prefix
    let generic_path = hooks_dir.join(prefix);
    if generic_path.exists() {
        hooks.push(HookScript {
            path: generic_path,
            event,
            type_filter: None,
        });
    }

    // Type-specific hook: event.type
    let typed_path = hooks_dir.join(format!("{}.{}", prefix, fragment_type));
    if typed_path.exists() {
        hooks.push(HookScript {
            path: typed_path,
            event,
            type_filter: Some(fragment_type.to_string()),
        });
    }

    hooks
}

/// Trait for executing hook scripts. Core defines the interface; CLI provides implementation.
pub trait HookRunner {
    /// Run a pre-hook. Returns Ok(Some(fragment)) if the hook modified the fragment,
    /// Ok(None) if the hook ran successfully without modifications,
    /// or Err if the hook failed (non-zero exit = abort).
    fn run_pre_hook(
        &self,
        script: &HookScript,
        fragment: &Fragment,
    ) -> Result<Option<Fragment>, ParcError>;

    /// Run a post-hook. Non-zero exit logs a warning but doesn't fail.
    fn run_post_hook(&self, script: &HookScript, fragment: &Fragment) -> Result<(), ParcError>;
}

/// Run all pre-hooks for an event. Returns the (possibly modified) fragment.
/// If any pre-hook fails, returns an error.
pub fn run_pre_hooks(
    runner: &dyn HookRunner,
    vault: &Path,
    event: HookEvent,
    fragment: &Fragment,
) -> Result<Fragment, ParcError> {
    let hooks = discover_hooks(vault, event, &fragment.fragment_type);
    let mut current = fragment.clone();

    for hook in &hooks {
        match runner.run_pre_hook(hook, &current)? {
            Some(modified) => current = modified,
            None => {}
        }
    }

    Ok(current)
}

/// Run all post-hooks for an event. Warnings are printed to stderr by the runner.
pub fn run_post_hooks(
    runner: &dyn HookRunner,
    vault: &Path,
    event: HookEvent,
    fragment: &Fragment,
) {
    let hooks = discover_hooks(vault, event, &fragment.fragment_type);
    for hook in &hooks {
        let _ = runner.run_post_hook(hook, fragment);
    }
}

/// Run pre-hooks (Tier 1 scripts + Tier 2 WASM plugins).
/// First runs Tier 1 hook scripts, then dispatches to WASM plugins.
#[cfg(feature = "wasm-plugins")]
pub fn run_pre_hooks_with_plugins(
    runner: &dyn HookRunner,
    vault: &Path,
    event: HookEvent,
    fragment: &Fragment,
    plugins: &mut crate::plugin::manager::PluginManager,
) -> Result<Fragment, ParcError> {
    // Tier 1: script hooks
    let current = run_pre_hooks(runner, vault, event, fragment)?;
    // Tier 2: WASM plugins
    let current = plugins.dispatch_pre_event(event, &current)?;
    Ok(current)
}

/// Run post-hooks (Tier 1 scripts + Tier 2 WASM plugins).
#[cfg(feature = "wasm-plugins")]
pub fn run_post_hooks_with_plugins(
    runner: &dyn HookRunner,
    vault: &Path,
    event: HookEvent,
    fragment: &Fragment,
    plugins: &mut crate::plugin::manager::PluginManager,
) {
    // Tier 1: script hooks
    run_post_hooks(runner, vault, event, fragment);
    // Tier 2: WASM plugins
    plugins.dispatch_post_event(event, fragment);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_hooks_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let hooks = discover_hooks(&vault, HookEvent::PreCreate, "note");
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_discover_hooks_generic() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let hooks_dir = vault.join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("post-create"), "#!/bin/sh\n").unwrap();

        let hooks = discover_hooks(&vault, HookEvent::PostCreate, "note");
        assert_eq!(hooks.len(), 1);
        assert!(hooks[0].type_filter.is_none());
    }

    #[test]
    fn test_discover_hooks_typed() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let hooks_dir = vault.join("hooks");
        std::fs::create_dir_all(&hooks_dir).unwrap();
        std::fs::write(hooks_dir.join("post-create"), "#!/bin/sh\n").unwrap();
        std::fs::write(hooks_dir.join("post-create.todo"), "#!/bin/sh\n").unwrap();

        // For "todo" type: generic + type-specific
        let hooks = discover_hooks(&vault, HookEvent::PostCreate, "todo");
        assert_eq!(hooks.len(), 2);

        // For "note" type: only generic
        let hooks = discover_hooks(&vault, HookEvent::PostCreate, "note");
        assert_eq!(hooks.len(), 1);
    }

    #[test]
    fn test_hook_event_prefix() {
        assert_eq!(HookEvent::PreCreate.prefix(), "pre-create");
        assert_eq!(HookEvent::PostCreate.prefix(), "post-create");
        assert_eq!(HookEvent::PreUpdate.prefix(), "pre-update");
        assert_eq!(HookEvent::PostUpdate.prefix(), "post-update");
        assert_eq!(HookEvent::PreDelete.prefix(), "pre-delete");
        assert_eq!(HookEvent::PostDelete.prefix(), "post-delete");
    }
}
