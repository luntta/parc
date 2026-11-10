use std::path::{Path, PathBuf};

use crate::config;
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
    if is_safe_type_filter(fragment_type) {
        let typed_path = hooks_dir.join(format!("{}.{}", prefix, fragment_type));
        if typed_path.exists() {
            hooks.push(HookScript {
                path: typed_path,
                event,
                type_filter: Some(fragment_type.to_string()),
            });
        }
    }

    hooks
}

/// Validate a hook before execution. Hooks are arbitrary programs, so Parc only
/// runs owner-controlled regular files from an explicitly trusted vault.
pub fn validate_hook_script(path: &Path) -> Result<(), ParcError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(ParcError::ValidationError(format!(
            "refusing to run symlinked hook '{}'",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(ParcError::ValidationError(format!(
            "refusing to run non-file hook '{}'",
            path.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let owner = metadata.uid();
        // SAFETY: geteuid has no preconditions and does not mutate memory.
        let current_user = unsafe { libc::geteuid() };
        if owner != current_user {
            return Err(ParcError::ValidationError(format!(
                "refusing to run hook '{}' owned by uid {}",
                path.display(),
                owner
            )));
        }

        let mode = metadata.permissions().mode();
        if mode & 0o022 != 0 {
            return Err(ParcError::ValidationError(format!(
                "refusing to run group/world-writable hook '{}'",
                path.display()
            )));
        }
        if mode & 0o100 == 0 {
            return Err(ParcError::ValidationError(format!(
                "refusing to run hook '{}' without owner execute permission",
                path.display()
            )));
        }
    }

    Ok(())
}

fn trusted_hooks(
    vault: &Path,
    event: HookEvent,
    fragment_type: &str,
) -> Result<Vec<HookScript>, ParcError> {
    let config = config::load_config(vault)?;
    if !config.hooks.enabled {
        return Ok(Vec::new());
    }

    let hooks = discover_hooks(vault, event, fragment_type);
    for hook in &hooks {
        validate_hook_script(&hook.path)?;
    }
    Ok(hooks)
}

fn is_safe_type_filter(fragment_type: &str) -> bool {
    !fragment_type.is_empty()
        && fragment_type
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Trait for executing hook scripts. Core defines the interface and ships
/// [`ScriptRunner`], which runs hooks as child processes.
pub trait HookRunner {
    /// Run a pre-hook. Returns Ok(Some(fragment)) if the hook modified the fragment,
    /// Ok(None) if the hook ran successfully without modifications,
    /// or Err if the hook failed (non-zero exit = abort).
    fn run_pre_hook(
        &self,
        script: &HookScript,
        fragment: &Fragment,
    ) -> Result<Option<Fragment>, ParcError>;

    /// Run a post-hook. An Err is reported to the caller as a warning; it never
    /// undoes the write that already happened.
    fn run_post_hook(&self, script: &HookScript, fragment: &Fragment) -> Result<(), ParcError>;
}

/// Where a hook's stderr goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HookStderr {
    /// Pass through to this process's stderr (interactive CLI).
    #[default]
    Inherit,
    /// Capture it and include it in error and warning messages. Use this when
    /// stderr is not free for the hook to write to, e.g. under a full-screen
    /// TUI or in a server.
    Capture,
}

/// Runs hook scripts as child processes. The fragment is passed as JSON on
/// stdin; a pre-hook may print a modified fragment as JSON on stdout.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScriptRunner {
    stderr: HookStderr,
}

impl ScriptRunner {
    pub fn new(stderr: HookStderr) -> Self {
        Self { stderr }
    }

    fn run(
        &self,
        script: &HookScript,
        fragment: &Fragment,
        capture_stdout: bool,
    ) -> Result<std::process::Output, ParcError> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let json = serde_json::to_vec(fragment)?;
        let io_err = |e: std::io::Error| {
            ParcError::Io(std::io::Error::new(
                e.kind(),
                format!("hook '{}': {}", script.path.display(), e),
            ))
        };

        let mut child = Command::new(&script.path)
            .stdin(Stdio::piped())
            .stdout(if capture_stdout {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(match self.stderr {
                HookStderr::Inherit => Stdio::inherit(),
                HookStderr::Capture => Stdio::piped(),
            })
            .spawn()
            .map_err(io_err)?;

        // Feed stdin from a separate thread so a hook that writes a lot of
        // output before reading its input cannot deadlock on a full pipe.
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let writer = std::thread::spawn(move || {
            // A hook may exit without reading stdin; a broken pipe is fine.
            let _ = stdin.write_all(&json);
        });
        let output = child.wait_with_output().map_err(io_err)?;
        let _ = writer.join();
        Ok(output)
    }

    fn failure_message(
        &self,
        kind: &str,
        script: &HookScript,
        output: &std::process::Output,
    ) -> String {
        let mut msg = format!(
            "{} '{}' failed (exit {})",
            kind,
            script.path.display(),
            output.status.code().unwrap_or(-1)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if !stderr.is_empty() {
            msg.push_str(": ");
            msg.push_str(stderr);
        }
        msg
    }
}

impl HookRunner for ScriptRunner {
    fn run_pre_hook(
        &self,
        script: &HookScript,
        fragment: &Fragment,
    ) -> Result<Option<Fragment>, ParcError> {
        let output = self.run(script, fragment, true)?;
        if !output.status.success() {
            return Err(ParcError::ValidationError(
                self.failure_message("pre-hook", script, &output),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();
        if stdout.is_empty() {
            return Ok(None);
        }

        let modified: Fragment = serde_json::from_str(stdout).map_err(|e| {
            ParcError::ParseError(format!(
                "pre-hook '{}' produced invalid JSON: {}",
                script.path.display(),
                e
            ))
        })?;
        Ok(Some(modified))
    }

    fn run_post_hook(&self, script: &HookScript, fragment: &Fragment) -> Result<(), ParcError> {
        let output = self.run(script, fragment, false)?;
        if !output.status.success() {
            return Err(ParcError::ValidationError(self.failure_message(
                "post-hook",
                script,
                &output,
            )));
        }
        Ok(())
    }
}

/// Run all pre-hooks for an event. Returns the (possibly modified) fragment.
/// If any pre-hook fails, returns an error.
pub fn run_pre_hooks(
    runner: &dyn HookRunner,
    vault: &Path,
    event: HookEvent,
    fragment: &Fragment,
) -> Result<Fragment, ParcError> {
    let hooks = trusted_hooks(vault, event, &fragment.fragment_type)?;
    let mut current = fragment.clone();

    for hook in &hooks {
        if let Some(modified) = runner.run_pre_hook(hook, &current)? {
            current = modified;
        }
    }

    Ok(current)
}

/// Run all post-hooks for an event. Post-hooks cannot fail the operation;
/// problems are returned as warnings for the caller to surface.
pub fn run_post_hooks(
    runner: &dyn HookRunner,
    vault: &Path,
    event: HookEvent,
    fragment: &Fragment,
) -> Vec<String> {
    let hooks = match trusted_hooks(vault, event, &fragment.fragment_type) {
        Ok(hooks) => hooks,
        Err(e) => return vec![format!("skipping hooks for {}: {}", event.prefix(), e)],
    };
    hooks
        .iter()
        .filter_map(|hook| runner.run_post_hook(hook, fragment).err())
        .map(|e| e.to_string())
        .collect()
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

/// Run post-hooks (Tier 1 scripts + Tier 2 WASM plugins). Returns warnings.
#[cfg(feature = "wasm-plugins")]
pub fn run_post_hooks_with_plugins(
    runner: &dyn HookRunner,
    vault: &Path,
    event: HookEvent,
    fragment: &Fragment,
    plugins: &mut crate::plugin::manager::PluginManager,
) -> Vec<String> {
    // Tier 1: script hooks
    let mut warnings = run_post_hooks(runner, vault, event, fragment);
    // Tier 2: WASM plugins
    warnings.extend(plugins.dispatch_post_event(event, fragment));
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct CountingRunner {
        pre_calls: Cell<usize>,
    }

    impl CountingRunner {
        fn new() -> Self {
            Self {
                pre_calls: Cell::new(0),
            }
        }
    }

    impl HookRunner for CountingRunner {
        fn run_pre_hook(
            &self,
            _script: &HookScript,
            _fragment: &Fragment,
        ) -> Result<Option<Fragment>, ParcError> {
            self.pre_calls.set(self.pre_calls.get() + 1);
            Ok(None)
        }

        fn run_post_hook(
            &self,
            _script: &HookScript,
            _fragment: &Fragment,
        ) -> Result<(), ParcError> {
            Ok(())
        }
    }

    fn make_fragment(fragment_type: &str) -> Fragment {
        Fragment {
            id: "01JQ7V3XKP5GQZ2N8R6T1WBMVH".to_string(),
            fragment_type: fragment_type.to_string(),
            title: "Test".to_string(),
            tags: Vec::new(),
            links: Vec::new(),
            attachments: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            created_by: None,
            extra_fields: std::collections::BTreeMap::new(),
            body: String::new(),
        }
    }

    fn write_hook(path: &Path) {
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }

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
    fn test_run_hooks_disabled_by_default() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();

        let hook_path = vault.join("hooks").join("pre-create");
        write_hook(&hook_path);

        let runner = CountingRunner::new();
        let fragment = make_fragment("note");
        let result = run_pre_hooks(&runner, &vault, HookEvent::PreCreate, &fragment).unwrap();

        assert_eq!(result.id, fragment.id);
        assert_eq!(runner.pre_calls.get(), 0);
    }

    #[test]
    fn test_run_hooks_when_explicitly_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        std::fs::write(vault.join("config.yml"), "hooks:\n  enabled: true\n").unwrap();

        let hook_path = vault.join("hooks").join("pre-create");
        write_hook(&hook_path);

        let runner = CountingRunner::new();
        let fragment = make_fragment("note");
        run_pre_hooks(&runner, &vault, HookEvent::PreCreate, &fragment).unwrap();

        assert_eq!(runner.pre_calls.get(), 1);
    }

    #[test]
    #[cfg(unix)]
    fn test_validate_hook_rejects_group_writable_script() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let hook_path = tmp.path().join("pre-create");
        std::fs::write(&hook_path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o720)).unwrap();

        assert!(validate_hook_script(&hook_path).is_err());
    }

    #[test]
    #[cfg(unix)]
    fn test_validate_hook_rejects_symlink() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::TempDir::new().unwrap();
        let target = tmp.path().join("target");
        write_hook(&target);
        let link = tmp.path().join("pre-create");
        symlink(&target, &link).unwrap();

        assert!(validate_hook_script(&link).is_err());
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
