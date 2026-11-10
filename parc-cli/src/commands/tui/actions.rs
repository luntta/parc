use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use parc_core::config::Config;
use parc_core::config::{get_editor, load_config};
use parc_core::fragment::{
    self, parse_fragment, read_fragment, serialize_fragment, validate_fragment,
};
use parc_core::ops::Writer;
use parc_core::schema::load_schemas;
use parc_core::secure_fs;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use serde_json::Value;

use super::QuickField;

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

/// Append any hook warnings the writer collected to a status message.
fn with_warnings(writer: &mut Writer, msg: String) -> String {
    let warnings = writer.take_warnings();
    match warnings.as_slice() {
        [] => msg,
        [one] => format!("{} (warning: {})", msg, one),
        [first, rest @ ..] => format!("{} (warning: {}; +{} more)", msg, first, rest.len()),
    }
}

pub(super) fn edit(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    writer: &mut Writer,
    id: &str,
) -> Result<String> {
    let mut stdout = io::stdout();
    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    let result = run_editor(writer, id);

    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, Hide)?;
    let _ = terminal.clear();

    result
}

fn run_editor(writer: &mut Writer, id: &str) -> Result<String> {
    let vault = writer.vault().to_path_buf();
    let vault = vault.as_path();
    let config = load_config(vault)?;
    let schemas = load_schemas(vault)?;
    let original = read_fragment(vault, id)?;
    let content = serialize_fragment(&original);
    let editor = get_editor(&config);
    let tmp_path = secure_fs::write_private_temp("parc-edit", ".md", content.as_bytes())?;

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

    let frag = parse_fragment(&edited)?;
    if let Some(s) = schemas.resolve(&frag.fragment_type) {
        validate_fragment(&frag, s)?;
    }
    let frag = writer.update(frag)?;
    Ok(with_warnings(writer, format!("edited {}", short(&frag.id))))
}

/// Flip the field the type's schema declares with `toggle: [off, on]`
/// (for todos, status open <-> done).
pub(super) fn toggle_status(writer: &mut Writer, id: &str) -> Result<String> {
    let mut frag = read_fragment(writer.vault(), id)?;
    let schemas = load_schemas(writer.vault())?;
    let toggle = schemas
        .resolve(&frag.fragment_type)
        .and_then(|schema| schema.toggle())
        .ok_or_else(|| {
            anyhow!(
                "{} has nothing to toggle (add `toggle: [a, b]` to an enum field in schemas/{}.yml)",
                frag.fragment_type,
                frag.fragment_type
            )
        })?;

    let current = frag
        .extra_fields
        .get(&toggle.field)
        .and_then(|v| v.as_str());
    let next = toggle.next(current).to_string();
    frag.extra_fields
        .insert(toggle.field.clone(), Value::String(next.clone()));

    let frag = writer.update(frag)?;
    Ok(with_warnings(
        writer,
        format!("{} {} {}", short(&frag.id), toggle.field, next),
    ))
}

/// Resolve a `[[target]]` wiki-link to a full fragment ID. The target may be
/// a ULID prefix; `read_fragment` already handles ambiguity and not-found
/// errors, which propagate to the caller as a status message.
pub(super) fn follow_link(vault: &Path, target: &str) -> Result<String> {
    let frag = read_fragment(vault, target)?;
    Ok(frag.id)
}

/// Toggle a `[ ]` ↔ `[x]` checkbox at a known byte range in the fragment body.
/// The range must point at exactly the 3-byte bracket literal — no offset
/// shift, so the rest of the body is untouched.
pub(super) fn toggle_checkbox(
    writer: &mut Writer,
    id: &str,
    source_range: std::ops::Range<usize>,
) -> Result<String> {
    let mut frag = read_fragment(writer.vault(), id)?;
    let bytes = frag.body.as_bytes();
    if source_range.end > bytes.len() || source_range.end - source_range.start != 3 {
        return Err(anyhow!("checkbox source range out of bounds"));
    }
    let next = match &bytes[source_range.start..source_range.end] {
        b"[ ]" => "[x]",
        b"[x]" | b"[X]" => "[ ]",
        other => {
            return Err(anyhow!(
                "checkbox source range did not match `[ ]`/`[x]` (found {:?})",
                String::from_utf8_lossy(other)
            ));
        }
    };
    frag.body.replace_range(source_range.clone(), next);

    let frag = writer.update(frag)?;
    let msg = format!(
        "{} checkbox {}",
        short(&frag.id),
        if next == "[x]" {
            "checked"
        } else {
            "unchecked"
        }
    );
    Ok(with_warnings(writer, msg))
}

pub(super) fn archive(writer: &mut Writer, id: &str) -> Result<String> {
    let already = read_fragment(writer.vault(), id)?
        .extra_fields
        .get("archived")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let frag = writer.set_archived(id, !already)?;
    let msg = if already {
        format!("unarchived {}", short(&frag.id))
    } else {
        format!("archived {}", short(&frag.id))
    };
    Ok(with_warnings(writer, msg))
}

pub(super) fn delete(writer: &mut Writer, id: &str) -> Result<String> {
    let frag = writer.delete(id)?;
    Ok(with_warnings(
        writer,
        format!("deleted {} (trash)", short(&frag.id)),
    ))
}

pub(super) fn yank(id: &str) -> Result<String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| anyhow!("clipboard unavailable: {}", e))?;
    clipboard
        .set_text(id.to_string())
        .map_err(|e| anyhow!("clipboard write failed: {}", e))?;
    Ok(format!("copied {} to clipboard", short(id)))
}

pub(super) fn promote(writer: &mut Writer, id: &str, new_type: &str) -> Result<String> {
    let promoted = writer.promote(id, new_type, BTreeMap::new())?;
    let msg = format!(
        "promoted {} to {}",
        short(&promoted.id),
        promoted.fragment_type
    );
    Ok(with_warnings(writer, msg))
}

pub(super) fn set_field(
    writer: &mut Writer,
    id: &str,
    field: QuickField,
    value: &str,
) -> Result<String> {
    let mut frag = read_fragment(writer.vault(), id)?;
    let trimmed = value.trim();

    match field {
        QuickField::Tags => {
            frag.tags = parse_tag_list(trimmed);
        }
        QuickField::Due => {
            set_optional_string(&mut frag.extra_fields, field.key(), trimmed, |value| {
                Ok(parc_core::date::resolve_due_date(value)?)
            })?
        }
        QuickField::Status | QuickField::Priority | QuickField::Assignee => {
            set_optional_string(&mut frag.extra_fields, field.key(), trimmed, |value| {
                Ok(value.to_string())
            })?
        }
    }

    let frag = writer.update(frag)?;
    Ok(with_warnings(
        writer,
        format!("updated {} {}", short(&frag.id), field.key()),
    ))
}

pub(super) fn capture(writer: &mut Writer, input: CaptureInput) -> Result<(String, String)> {
    let vault = writer.vault().to_path_buf();
    let vault = vault.as_path();
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

    let frag = writer.create(frag)?;
    let msg = format!("captured {} {}", short(&frag.id), frag.fragment_type);
    Ok((frag.id, with_warnings(writer, msg)))
}

fn parse_tag_list(input: &str) -> Vec<String> {
    let mut tags = Vec::new();
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
    tags
}

fn merge_tags(tags: &mut Vec<String>, config: &Config, input: &str) {
    *tags = config.default_tags.clone();
    for tag in parse_tag_list(input) {
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

fn set_optional_string(
    fields: &mut BTreeMap<String, Value>,
    key: &str,
    value: &str,
    normalize: impl FnOnce(&str) -> Result<String>,
) -> Result<()> {
    if value.is_empty() {
        fields.remove(key);
    } else {
        fields.insert(key.to_string(), Value::String(normalize(value)?));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parc_core::fragment::read_fragment;
    use parc_core::hook::HookStderr;
    use parc_core::vault::init_vault;

    fn test_writer(vault: &Path) -> Writer {
        crate::hooks::writer(vault, HookStderr::Capture)
    }

    #[test]
    fn capture_creates_fragment_with_metadata() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();

        let (id, msg) = capture(
            &mut test_writer(&vault),
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

    #[test]
    fn set_field_updates_common_metadata() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();

        let (id, _) = capture(
            &mut test_writer(&vault),
            CaptureInput {
                text: "Review quick edits".to_string(),
                fragment_type: "todo".to_string(),
                tags: String::new(),
                status: "open".to_string(),
                due: String::new(),
                priority: String::new(),
                assignee: String::new(),
            },
        )
        .unwrap();

        set_field(&mut test_writer(&vault), &id, QuickField::Due, "2026-03-01").unwrap();
        set_field(
            &mut test_writer(&vault),
            &id,
            QuickField::Priority,
            "medium",
        )
        .unwrap();
        set_field(&mut test_writer(&vault), &id, QuickField::Assignee, "raine").unwrap();
        set_field(
            &mut test_writer(&vault),
            &id,
            QuickField::Tags,
            "ui #quick ui",
        )
        .unwrap();

        let fragment = read_fragment(&vault, &id).unwrap();
        assert_eq!(
            fragment.extra_fields.get("due").and_then(|v| v.as_str()),
            Some("2026-03-01")
        );
        assert_eq!(
            fragment
                .extra_fields
                .get("priority")
                .and_then(|v| v.as_str()),
            Some("medium")
        );
        assert_eq!(
            fragment
                .extra_fields
                .get("assignee")
                .and_then(|v| v.as_str()),
            Some("raine")
        );
        assert_eq!(fragment.tags, vec!["ui".to_string(), "quick".to_string()]);

        set_field(&mut test_writer(&vault), &id, QuickField::Due, "").unwrap();
        let fragment = read_fragment(&vault, &id).unwrap();
        assert!(!fragment.extra_fields.contains_key("due"));
    }

    #[test]
    fn toggle_checkbox_round_trips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();

        let (id, _) = capture(
            &mut test_writer(&vault),
            CaptureInput {
                text: "task list\n\n- [ ] one\n- [ ] two\n".to_string(),
                fragment_type: "note".to_string(),
                tags: String::new(),
                status: String::new(),
                due: String::new(),
                priority: String::new(),
                assignee: String::new(),
            },
        )
        .unwrap();

        // Locate the second `[ ]` byte range.
        let frag = read_fragment(&vault, &id).unwrap();
        let mut occurrences = frag.body.match_indices("[ ]");
        let _first = occurrences.next().unwrap();
        let (second_start, _) = occurrences.next().unwrap();
        let range = second_start..second_start + 3;

        let msg = toggle_checkbox(&mut test_writer(&vault), &id, range.clone()).unwrap();
        assert!(msg.contains("checked"));
        let frag = read_fragment(&vault, &id).unwrap();
        assert_eq!(&frag.body[range.clone()], "[x]");

        let msg = toggle_checkbox(&mut test_writer(&vault), &id, range.clone()).unwrap();
        assert!(msg.contains("unchecked"));
        let frag = read_fragment(&vault, &id).unwrap();
        assert_eq!(&frag.body[range], "[ ]");
    }

    #[test]
    fn toggle_checkbox_rejects_bad_range() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();

        let (id, _) = capture(
            &mut test_writer(&vault),
            CaptureInput {
                text: "no checkboxes here".to_string(),
                fragment_type: "note".to_string(),
                tags: String::new(),
                status: String::new(),
                due: String::new(),
                priority: String::new(),
                assignee: String::new(),
            },
        )
        .unwrap();

        // Range that doesn't point at `[ ]`.
        assert!(toggle_checkbox(&mut test_writer(&vault), &id, 0..3).is_err());
        // Out-of-bounds.
        assert!(toggle_checkbox(&mut test_writer(&vault), &id, 1_000..1_003).is_err());
    }

    fn capture_type(writer: &mut Writer, fragment_type: &str, text: &str) -> String {
        capture(
            writer,
            CaptureInput {
                text: text.to_string(),
                fragment_type: fragment_type.to_string(),
                tags: String::new(),
                status: String::new(),
                due: String::new(),
                priority: String::new(),
                assignee: String::new(),
            },
        )
        .unwrap()
        .0
    }

    fn status_of(vault: &Path, id: &str) -> Option<String> {
        read_fragment(vault, id)
            .unwrap()
            .extra_fields
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    #[test]
    fn toggle_follows_each_schema() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();
        let mut w = test_writer(&vault);

        let todo = capture_type(&mut w, "todo", "Ship it");
        toggle_status(&mut w, &todo).unwrap();
        assert_eq!(status_of(&vault, &todo).as_deref(), Some("done"));
        toggle_status(&mut w, &todo).unwrap();
        assert_eq!(status_of(&vault, &todo).as_deref(), Some("open"));

        let risk = capture_type(&mut w, "risk", "Disk fills up");
        let msg = toggle_status(&mut w, &risk).unwrap();
        assert!(msg.ends_with("status resolved"), "{}", msg);
        assert_eq!(status_of(&vault, &risk).as_deref(), Some("resolved"));

        let idea = capture_type(&mut w, "idea", "Maybe later");
        let err = toggle_status(&mut w, &idea).unwrap_err().to_string();
        assert!(err.contains("idea has nothing to toggle"), "{}", err);
        assert_eq!(
            status_of(&vault, &idea).as_deref(),
            Some("raw"),
            "unchanged"
        );
    }

    #[test]
    fn toggle_finishes_an_in_progress_todo() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();
        let mut w = test_writer(&vault);
        let todo = capture_type(&mut w, "todo", "Halfway");
        set_field(&mut w, &todo, QuickField::Status, "in-progress").unwrap();

        toggle_status(&mut w, &todo).unwrap();

        assert_eq!(status_of(&vault, &todo).as_deref(), Some("done"));
    }
}
