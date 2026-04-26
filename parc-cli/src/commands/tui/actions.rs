use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Result};
use chrono::Utc;
use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use parc_core::config::{get_editor, load_config};
use parc_core::fragment::{
    self, delete_fragment as core_delete, parse_fragment, read_fragment, serialize_fragment,
    validate_fragment, write_fragment,
};
use parc_core::hook::{self, HookEvent};
use parc_core::index;
use parc_core::schema::load_schemas;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::Value;

use crate::hooks::CliHookRunner;

fn short(id: &str) -> &str {
    &id[..8.min(id.len())]
}

pub(super) fn edit(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    vault: &Path,
    id: &str,
) -> Result<String> {
    let mut stdout = io::stdout();
    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    let result = run_editor(vault, id);

    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, Hide)?;
    let _ = terminal.clear();

    result
}

fn run_editor(vault: &Path, id: &str) -> Result<String> {
    let config = load_config(vault)?;
    let schemas = load_schemas(vault)?;
    let runner = CliHookRunner;
    let original = read_fragment(vault, id)?;
    let content = serialize_fragment(&original);
    let editor = get_editor(&config);
    let tmp_path = std::env::temp_dir().join(format!("parc-edit-{}.md", short(&original.id)));
    std::fs::write(&tmp_path, &content)?;

    let status = Command::new(&editor).arg(&tmp_path).status()?;

    if !status.success() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow!("editor exited with non-zero status"));
    }

    let edited = std::fs::read_to_string(&tmp_path)?;
    let _ = std::fs::remove_file(&tmp_path);

    if edited == content {
        return Ok(format!("no changes to {}", short(&original.id)));
    }
    if edited.trim().is_empty() {
        return Err(anyhow!("aborted: empty content"));
    }

    let mut frag = parse_fragment(&edited)?;
    if let Some(s) = schemas.resolve(&frag.fragment_type) {
        validate_fragment(&frag, s)?;
    }
    frag.updated_at = Utc::now();

    let frag = hook::run_pre_hooks(&runner, vault, HookEvent::PreUpdate, &frag)?;
    write_fragment(vault, &frag)?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;
    hook::run_post_hooks(&runner, vault, HookEvent::PostUpdate, &frag);

    Ok(format!("edited {}", short(&frag.id)))
}

pub(super) fn toggle_status(vault: &Path, id: &str) -> Result<String> {
    let mut frag = read_fragment(vault, id)?;
    if frag.fragment_type != "todo" {
        return Ok(format!("{} not a todo", short(&frag.id)));
    }
    let cur = frag
        .extra_fields
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("open")
        .to_string();
    let next = if cur == "done" { "open" } else { "done" };
    frag.extra_fields
        .insert("status".to_string(), Value::String(next.to_string()));
    frag.updated_at = Utc::now();

    let runner = CliHookRunner;
    let frag = hook::run_pre_hooks(&runner, vault, HookEvent::PreUpdate, &frag)?;
    write_fragment(vault, &frag)?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;
    hook::run_post_hooks(&runner, vault, HookEvent::PostUpdate, &frag);

    Ok(format!("{} {}", short(&frag.id), next))
}

pub(super) fn archive(vault: &Path, id: &str) -> Result<String> {
    let mut frag = read_fragment(vault, id)?;
    let full_id = frag.id.clone();
    let already = frag
        .extra_fields
        .get("archived")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if already {
        frag.extra_fields.remove("archived");
    } else {
        frag.extra_fields
            .insert("archived".to_string(), Value::Bool(true));
    }
    frag.updated_at = Utc::now();
    write_fragment(vault, &frag)?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;

    Ok(if already {
        format!("unarchived {}", short(&full_id))
    } else {
        format!("archived {}", short(&full_id))
    })
}

pub(super) fn delete(vault: &Path, id: &str) -> Result<String> {
    let frag = read_fragment(vault, id)?;
    let runner = CliHookRunner;
    let _ = hook::run_pre_hooks(&runner, vault, HookEvent::PreDelete, &frag)?;
    let full_id = core_delete(vault, &frag.id)?;
    let conn = index::open_index(vault)?;
    index::remove_from_index(&conn, &full_id)?;
    hook::run_post_hooks(&runner, vault, HookEvent::PostDelete, &frag);
    Ok(format!("deleted {} (trash)", short(&full_id)))
}

pub(super) fn yank(id: &str) -> Result<String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| anyhow!("clipboard unavailable: {}", e))?;
    clipboard
        .set_text(id.to_string())
        .map_err(|e| anyhow!("clipboard write failed: {}", e))?;
    Ok(format!("copied {} to clipboard", short(id)))
}

pub(super) fn promote(vault: &Path, id: &str, new_type: &str) -> Result<String> {
    let promoted = fragment::promote_fragment(vault, id, new_type, BTreeMap::new())?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &promoted, vault)?;
    Ok(format!("promoted {} to {}", short(&promoted.id), promoted.fragment_type))
}
