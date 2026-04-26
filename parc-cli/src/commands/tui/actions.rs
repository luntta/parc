use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Result};
use chrono::Utc;
use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use parc_core::config::Config;
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

pub(super) struct CaptureInput {
    pub text: String,
    pub fragment_type: String,
    pub tags: String,
    pub status: String,
    pub due: String,
    pub priority: String,
    pub assignee: String,
}

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
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| anyhow!("clipboard unavailable: {}", e))?;
    clipboard
        .set_text(id.to_string())
        .map_err(|e| anyhow!("clipboard write failed: {}", e))?;
    Ok(format!("copied {} to clipboard", short(id)))
}

pub(super) fn promote(vault: &Path, id: &str, new_type: &str) -> Result<String> {
    let promoted = fragment::promote_fragment(vault, id, new_type, BTreeMap::new())?;
    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &promoted, vault)?;
    Ok(format!(
        "promoted {} to {}",
        short(&promoted.id),
        promoted.fragment_type
    ))
}

pub(super) fn capture(vault: &Path, input: CaptureInput) -> Result<(String, String)> {
    let raw = input.text.trim_end_matches(['\r', '\n']);
    if raw.trim().is_empty() {
        return Err(anyhow!("capture text is empty"));
    }

    let config = load_config(vault)?;
    let schemas = load_schemas(vault)?;
    let schema = schemas
        .resolve(&input.fragment_type)
        .ok_or_else(|| anyhow!("unknown type: {}", input.fragment_type))?;
    let (title, body) = crate::commands::capture::split_capture_text(raw);

    let mut frag = fragment::new_fragment(&schema.name, &title, schema, &config);
    frag.body = body;
    merge_tags(&mut frag.tags, &config, &input.tags);
    apply_capture_fields(&mut frag, input)?;
    validate_fragment(&frag, schema)?;

    let runner = CliHookRunner;
    let frag = hook::run_pre_hooks(&runner, vault, HookEvent::PreCreate, &frag)?;
    fragment::create_fragment(vault, &frag)?;

    let conn = index::open_index(vault)?;
    index::index_fragment_auto(&conn, &frag, vault)?;
    hook::run_post_hooks(&runner, vault, HookEvent::PostCreate, &frag);

    Ok((
        frag.id.clone(),
        format!("captured {} {}", short(&frag.id), frag.fragment_type),
    ))
}

fn merge_tags(tags: &mut Vec<String>, config: &Config, input: &str) {
    *tags = config.default_tags.clone();
    for tag in input
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .map(|tag| tag.trim().trim_start_matches('#'))
        .filter(|tag| !tag.is_empty())
    {
        let tag = tag.to_string();
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags.dedup();
}

fn apply_capture_fields(fragment: &mut fragment::Fragment, input: CaptureInput) -> Result<()> {
    insert_string_field(fragment, "status", input.status);
    insert_string_field(fragment, "priority", input.priority);
    insert_string_field(fragment, "assignee", input.assignee);

    let due = input.due.trim();
    if !due.is_empty() {
        fragment.extra_fields.insert(
            "due".to_string(),
            Value::String(parc_core::date::resolve_due_date(due)?),
        );
    }

    Ok(())
}

fn insert_string_field(fragment: &mut fragment::Fragment, key: &str, value: String) {
    let value = value.trim();
    if !value.is_empty() {
        fragment
            .extra_fields
            .insert(key.to_string(), Value::String(value.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parc_core::fragment::read_fragment;
    use parc_core::vault::init_vault;

    #[test]
    fn capture_creates_fragment_with_metadata() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();

        let (id, msg) = capture(
            &vault,
            CaptureInput {
                text: "Ship TUI capture".to_string(),
                fragment_type: "todo".to_string(),
                tags: "ui #quick".to_string(),
                status: "open".to_string(),
                due: "2026-03-01".to_string(),
                priority: "high".to_string(),
                assignee: "raine".to_string(),
            },
        )
        .unwrap();

        let fragment = read_fragment(&vault, &id).unwrap();
        assert!(msg.starts_with("captured "));
        assert_eq!(fragment.fragment_type, "todo");
        assert_eq!(fragment.title, "Ship TUI capture");
        assert!(fragment.tags.contains(&"ui".to_string()));
        assert!(fragment.tags.contains(&"quick".to_string()));
        assert_eq!(
            fragment
                .extra_fields
                .get("priority")
                .and_then(|v| v.as_str()),
            Some("high")
        );
        assert_eq!(
            fragment
                .extra_fields
                .get("assignee")
                .and_then(|v| v.as_str()),
            Some("raine")
        );
    }
}
