use std::path::Path;
use std::process::{Command, ExitStatus};

use anyhow::{Context, Result};
use parc_core::config::{editor_command, get_editor, Config};

use crate::render::sanitize_terminal_text;

/// Open `file` in the user's editor and wait for it to close, after warning
/// on stderr if the vault's own `editor:` setting is being ignored.
pub fn open(config: &Config, file: &Path) -> Result<ExitStatus> {
    if let Some(note) = ignored_note(config) {
        eprintln!("warning: {}", note);
    }
    launch(config, file)
}

/// Open `file` in the editor without printing anything (the TUI reports
/// [`ignored_note`] in its status line instead). The editor setting may
/// carry arguments (`EDITOR="code --wait"`); it is split into words, never
/// handed to a shell.
pub fn launch(config: &Config, file: &Path) -> Result<ExitStatus> {
    let argv = editor_command(config)?;
    let (program, args) = argv.split_first().context("the editor command is empty")?;
    Command::new(program)
        .args(args)
        .arg(file)
        .status()
        .with_context(|| format!("failed to launch editor: {}", get_editor(config)))
}

/// Why a project vault's `editor:` didn't choose the editor, if it set one.
pub fn ignored_note(config: &Config) -> Option<String> {
    config.ignored_editor.as_ref().map(|editor| {
        format!(
            "ignoring `editor: {}` in this vault's config.yml: a vault can't choose the program parc runs. Using {}; set $EDITOR, or editor: in ~/.parc/config.yml",
            sanitize_terminal_text(editor),
            sanitize_terminal_text(&get_editor(config))
        )
    })
}

/// Only discard the private draft after the caller has successfully saved it.
/// An explicit empty-file cancellation may remove the draft itself.
pub fn with_draft<T>(initial: &str, edit: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    let path = parc_core::secure_fs::write_private_temp("parc-edit", ".md", initial.as_bytes())?;
    match edit(&path) {
        Ok(result) => {
            let _ = std::fs::remove_file(&path);
            Ok(result)
        }
        Err(e) if path.exists() => Err(anyhow::anyhow!(
            "{e:#}; your edits are kept in {}",
            path.display()
        )),
        Err(e) => Err(e),
    }
}

pub fn check_edit_id(original: &str, edited: &str) -> Result<()> {
    anyhow::ensure!(
        original == edited,
        "a fragment's id cannot be changed while editing"
    );
    Ok(())
}
