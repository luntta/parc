//! The universal launcher (`/`, `Ctrl-P`): fragment search, command and
//! view matching, and typed field/tag intents such as `status done` or
//! `add tag tui`.

use std::path::Path;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use parc_core::config::Config;
use parc_core::date;
use parc_core::schema::{load_schemas, FieldType};

use super::{
    archive_id, edit_id, finish_write, manual_reload, open_capture_form, open_delete_confirm,
    open_promote_input, select_row_by_id, short_id, start_field_input, switch_tab,
    toggle_status_id, yank_id, App, Term, HALF_PAGE_LINES, PAGE_LINES,
};
use crate::commands::tui::command::{self, CommandAction, LauncherKind};
use crate::commands::tui::{
    actions, data, Focus, IntentAction, LauncherIntent, LauncherItem, LauncherPopup, Mode,
    QuickField, Row, Tab,
};

pub(super) fn handle_launcher(
    app: &mut App,
    key: KeyEvent,
    mut popup: LauncherPopup,
    terminal: &mut Term,
    vault: &Path,
    config: &Config,
) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char('c') if ctrl => return Ok(false),
        KeyCode::Esc => {
            app.launcher_input = popup.input;
            app.mode = Mode::Normal;
            app.dirty = true;
            return Ok(true);
        }
        KeyCode::BackTab => {
            popup.focus = popup.focus.toggle();
        }
        KeyCode::Down => match popup.focus {
            Focus::List => popup.move_list(1),
            Focus::Detail => popup.scroll_detail(1),
        },
        KeyCode::Up => match popup.focus {
            Focus::List => popup.move_list(-1),
            Focus::Detail => popup.scroll_detail(-1),
        },
        KeyCode::PageDown => match popup.focus {
            Focus::List => popup.move_list(PAGE_LINES as i32),
            Focus::Detail => popup.scroll_detail(PAGE_LINES as i32),
        },
        KeyCode::PageUp => match popup.focus {
            Focus::List => popup.move_list(-(PAGE_LINES as i32)),
            Focus::Detail => popup.scroll_detail(-(PAGE_LINES as i32)),
        },
        KeyCode::Char('d') if ctrl => match popup.focus {
            Focus::List => popup.move_list(HALF_PAGE_LINES as i32),
            Focus::Detail => popup.scroll_detail(HALF_PAGE_LINES as i32),
        },
        KeyCode::Char('u') if ctrl => match popup.focus {
            Focus::List => popup.move_list(-(HALF_PAGE_LINES as i32)),
            Focus::Detail => popup.scroll_detail(-(HALF_PAGE_LINES as i32)),
        },
        KeyCode::Home => match popup.focus {
            Focus::List => popup.list_state.select(if popup.item_count() == 0 {
                None
            } else {
                Some(0)
            }),
            Focus::Detail => popup.detail_scroll = 0,
        },
        KeyCode::End => match popup.focus {
            Focus::List => {
                let last = popup.item_count().saturating_sub(1);
                popup.list_state.select(if popup.item_count() == 0 {
                    None
                } else {
                    Some(last)
                });
            }
            Focus::Detail => popup.detail_scroll = popup.detail_max_scroll,
        },
        KeyCode::Backspace => {
            if popup.input.pop().is_some() {
                reload_launcher_popup(app, &mut popup, vault, true);
            }
        }
        KeyCode::Enter => match popup.selected_item().cloned() {
            Some(LauncherItem::Fragment(row)) => {
                let rows = popup.rows.clone();
                let query = popup.input.clone();
                app.launcher_input = query.clone();
                app.mode = Mode::Normal;
                open_search_result(app, rows, query, &row.id);
                return Ok(true);
            }
            Some(LauncherItem::Command(command)) => {
                app.launcher_input = popup.input;
                app.mode = Mode::Normal;
                execute_command_action(app, command.action, terminal, vault, config)?;
                app.dirty = true;
                return Ok(true);
            }
            Some(LauncherItem::Intent(intent)) => {
                app.launcher_input = popup.input;
                app.mode = Mode::Normal;
                if intent.valid {
                    execute_intent_action(app, intent.action, vault, config)?;
                } else {
                    app.set_status(
                        intent
                            .detail
                            .unwrap_or_else(|| "invalid action".to_string()),
                    );
                }
                app.dirty = true;
                return Ok(true);
            }
            None => {}
        },
        KeyCode::Char(c) if !ctrl => {
            popup.input.push(c);
            reload_launcher_popup(app, &mut popup, vault, true);
        }
        _ => {}
    }

    popup.clamp_selection();
    app.launcher_input = popup.input.clone();
    app.mode = Mode::Launcher(popup);
    app.dirty = true;
    Ok(true)
}

pub(super) fn open_launcher(app: &mut App, vault: &Path, input: &str) {
    let mut popup = LauncherPopup::new(input.to_string());
    reload_launcher_popup(app, &mut popup, vault, true);
    app.mode = Mode::Launcher(popup);
    app.clear_status();
    app.dirty = true;
}

fn open_search_result(app: &mut App, rows: Vec<Row>, query: String, id: &str) {
    app.save_tab_state();
    app.tab = Tab::Search;
    app.search_view_query = Some(query.trim().to_string());
    app.rows = rows;
    select_row_by_id(app, id);
    app.focus = Focus::List;
    app.detail_scroll = 0;
    app.set_status(format!("opened {}", short_id(id)));
    app.dirty = true;
}

fn reload_launcher_popup(
    app: &mut App,
    popup: &mut LauncherPopup,
    vault: &Path,
    reset_selection: bool,
) {
    match popup.kind() {
        LauncherKind::Universal => reload_universal_results(app, popup, vault, reset_selection),
        LauncherKind::Commands => reload_command_results(app, popup, reset_selection),
    }
}

fn reload_universal_results(
    app: &mut App,
    popup: &mut LauncherPopup,
    vault: &Path,
    reset_selection: bool,
) {
    let selected_item = if reset_selection {
        None
    } else {
        popup.selected_item().cloned()
    };

    match data::load_search_rows(vault, &popup.input, &mut app.search) {
        Ok(rows) => {
            popup.rows = rows;
            popup.commands = command::matching_commands(&popup.input, app.selected_id().is_some());
            let selected_row = app.selected_row().cloned();
            let intents = launcher_intents(
                &popup.input,
                selected_row.as_ref(),
                &app.rows,
                &popup.rows,
                vault,
            );
            popup.items =
                ranked_universal_items(&popup.input, &intents, &popup.commands, &popup.rows);
            popup.error = None;
            restore_launcher_selection(popup, selected_item);
        }
        Err(err) => {
            popup.rows.clear();
            popup.commands = command::matching_commands(&popup.input, app.selected_id().is_some());
            let selected_row = app.selected_row().cloned();
            let intents = launcher_intents(
                &popup.input,
                selected_row.as_ref(),
                &app.rows,
                &popup.rows,
                vault,
            );
            popup.items =
                ranked_universal_items(&popup.input, &intents, &popup.commands, &popup.rows);
            popup.error = Some(err.to_string());
            restore_launcher_selection(popup, selected_item);
        }
    }
}

fn reload_command_results(app: &App, popup: &mut LauncherPopup, reset_selection: bool) {
    let selected_item = if reset_selection {
        None
    } else {
        popup.selected_item().cloned()
    };
    popup.rows.clear();
    popup.error = None;
    popup.commands = command::matching_commands(&popup.input, app.selected_id().is_some());
    popup.items = popup
        .commands
        .iter()
        .copied()
        .map(LauncherItem::Command)
        .collect();

    restore_launcher_selection(popup, selected_item);
}

fn restore_launcher_selection(popup: &mut LauncherPopup, selected_item: Option<LauncherItem>) {
    let idx = selected_item.and_then(|selected| {
        popup
            .items
            .iter()
            .position(|candidate| same_launcher_item(candidate, &selected))
    });
    if let Some(idx) = idx {
        popup.list_state.select(Some(idx));
    } else {
        popup.select_first();
    }
}

fn same_launcher_item(a: &LauncherItem, b: &LauncherItem) -> bool {
    match (a, b) {
        (LauncherItem::Fragment(a), LauncherItem::Fragment(b)) => a.id == b.id,
        (LauncherItem::Command(a), LauncherItem::Command(b)) => a.action == b.action,
        (LauncherItem::Intent(a), LauncherItem::Intent(b)) => a.action == b.action,
        _ => false,
    }
}

fn ranked_universal_items(
    input: &str,
    intents: &[LauncherIntent],
    commands: &[command::CommandEntry],
    rows: &[Row],
) -> Vec<LauncherItem> {
    let query = command::command_query(input).trim();
    let mut scored = intents
        .iter()
        .cloned()
        .map(LauncherItem::Intent)
        .chain(commands.iter().copied().map(LauncherItem::Command))
        .chain(rows.iter().cloned().map(LauncherItem::Fragment))
        .map(|item| (launcher_item_score(&item, query), item))
        .collect::<Vec<_>>();
    scored.sort_by(|(score_a, _), (score_b, _)| score_b.cmp(score_a));
    scored.into_iter().map(|(_, item)| item).collect()
}

fn launcher_intents(
    input: &str,
    selected_row: Option<&Row>,
    current_rows: &[Row],
    search_rows: &[Row],
    vault: &Path,
) -> Vec<LauncherIntent> {
    let Some(selected_row) = selected_row else {
        return Vec::new();
    };
    let query = input.strip_prefix('>').unwrap_or(input).trim_start();
    let tag_intents = launcher_tag_intents(query, selected_row, current_rows, search_rows);
    if !tag_intents.is_empty() {
        return tag_intents;
    }

    let Some((field, value_prefix)) = parse_field_intent(query) else {
        return Vec::new();
    };

    let value_prefix = value_prefix.trim();
    let mut values = suggested_field_values(vault, selected_row, current_rows, search_rows, field)
        .into_iter()
        .filter(|value| {
            value_prefix.is_empty()
                || value.starts_with(value_prefix)
                || value
                    .get(..value_prefix.len().min(value.len()))
                    .is_some_and(|head| head.eq_ignore_ascii_case(value_prefix))
        })
        .collect::<Vec<_>>();

    if values.is_empty() && !value_prefix.is_empty() {
        values.push(value_prefix.to_string());
    }

    values
        .into_iter()
        .take(8)
        .map(|value| build_set_field_intent(vault, selected_row, field, value))
        .collect()
}

fn launcher_tag_intents(
    query: &str,
    selected_row: &Row,
    current_rows: &[Row],
    search_rows: &[Row],
) -> Vec<LauncherIntent> {
    for (prefix, kind) in [
        ("add tag ", TagIntentKind::Add),
        ("remove tag ", TagIntentKind::Remove),
        ("set tags ", TagIntentKind::Set),
    ] {
        let Some(raw_value) = strip_ascii_prefix(query, prefix) else {
            continue;
        };
        return tag_intents_for_kind(kind, raw_value, selected_row, current_rows, search_rows);
    }
    Vec::new()
}

#[derive(Clone, Copy)]
enum TagIntentKind {
    Add,
    Remove,
    Set,
}

fn tag_intents_for_kind(
    kind: TagIntentKind,
    raw_value: &str,
    selected_row: &Row,
    current_rows: &[Row],
    search_rows: &[Row],
) -> Vec<LauncherIntent> {
    let value_prefix = raw_value.trim();
    let candidates = match kind {
        TagIntentKind::Add => known_tags(current_rows, search_rows)
            .into_iter()
            .filter(|tag| !selected_row.tags.iter().any(|existing| existing == tag))
            .collect(),
        TagIntentKind::Remove => selected_row.tags.clone(),
        TagIntentKind::Set => Vec::new(),
    };
    let suggestions = candidates
        .into_iter()
        .filter(|tag| {
            value_prefix.is_empty()
                || tag
                    .get(..value_prefix.len().min(tag.len()))
                    .is_some_and(|head| head.eq_ignore_ascii_case(value_prefix))
        })
        .collect::<Vec<_>>();

    if !suggestions.is_empty() {
        return suggestions
            .into_iter()
            .take(8)
            .map(|tag| build_tag_intent(kind, vec![tag]))
            .collect();
    }

    let tags = parse_tag_value(value_prefix);
    if tags.is_empty() {
        return Vec::new();
    }
    vec![build_tag_intent(kind, tags)]
}

fn known_tags(current_rows: &[Row], search_rows: &[Row]) -> Vec<String> {
    let mut values = Vec::new();
    for row in current_rows.iter().chain(search_rows) {
        for tag in &row.tags {
            if !values.iter().any(|value: &String| value == tag) {
                values.push(tag.clone());
            }
        }
    }
    values
}

fn parse_tag_value(value: &str) -> Vec<String> {
    let mut tags = Vec::new();
    for tag in value
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

fn build_tag_intent(kind: TagIntentKind, tags: Vec<String>) -> LauncherIntent {
    let joined = tags.join(", ");
    let (label, description, action) = match kind {
        TagIntentKind::Add => (
            format!("Add tag {}", joined),
            format!("Add {} to the selected fragment.", tag_phrase(&tags)),
            IntentAction::AddTags { tags },
        ),
        TagIntentKind::Remove => (
            format!("Remove tag {}", joined),
            format!("Remove {} from the selected fragment.", tag_phrase(&tags)),
            IntentAction::RemoveTags { tags },
        ),
        TagIntentKind::Set => (
            format!("Set tags to {}", joined),
            format!("Replace the selected fragment's tags with `{}`.", joined),
            IntentAction::SetTags { tags },
        ),
    };
    LauncherIntent {
        label,
        description,
        detail: None,
        valid: true,
        action,
    }
}

fn tag_phrase(tags: &[String]) -> String {
    if tags.len() == 1 {
        format!("tag `{}`", tags[0])
    } else {
        format!("tags `{}`", tags.join(", "))
    }
}

fn build_set_field_intent(
    vault: &Path,
    selected_row: &Row,
    field: QuickField,
    value: String,
) -> LauncherIntent {
    let validation = validate_set_field_intent(vault, selected_row, field, &value);
    let valid = validation.is_ok();
    let detail = match validation {
        Ok(Some(detail)) => Some(detail),
        Ok(None) => None,
        Err(reason) => Some(reason),
    };
    LauncherIntent {
        label: format!("Set {} to {}", field.label(), value),
        description: format!(
            "Update the selected fragment's {} field to `{}`.",
            field.key(),
            value
        ),
        detail,
        valid,
        action: IntentAction::SetField { field, value },
    }
}

fn validate_set_field_intent(
    vault: &Path,
    selected_row: &Row,
    field: QuickField,
    value: &str,
) -> std::result::Result<Option<String>, String> {
    match field {
        QuickField::Due => date::resolve_due_date(value)
            .map(|normalized| Some(format!("Will store due date `{}`.", normalized)))
            .map_err(|err| err.to_string()),
        QuickField::Status | QuickField::Priority => {
            let allowed = schema_enum_values(vault, &selected_row.fragment_type, field.key());
            if allowed.is_empty() || allowed.iter().any(|allowed| allowed == value) {
                Ok(None)
            } else {
                Err(format!(
                    "Invalid {} `{}`. Allowed values: {}.",
                    field.key(),
                    value,
                    allowed.join(", ")
                ))
            }
        }
        QuickField::Assignee => Ok(None),
        QuickField::Tags => Ok(None),
    }
}

fn suggested_field_values(
    vault: &Path,
    selected_row: &Row,
    current_rows: &[Row],
    search_rows: &[Row],
    field: QuickField,
) -> Vec<String> {
    match field {
        QuickField::Status | QuickField::Priority => {
            schema_enum_values(vault, &selected_row.fragment_type, field.key())
        }
        QuickField::Due => vec![
            "today".to_string(),
            "tomorrow".to_string(),
            "this-week".to_string(),
            "next-week".to_string(),
            "in-3-days".to_string(),
        ],
        QuickField::Assignee => known_assignees(current_rows, search_rows),
        QuickField::Tags => Vec::new(),
    }
}

fn schema_enum_values(vault: &Path, fragment_type: &str, field_key: &str) -> Vec<String> {
    let Ok(schemas) = load_schemas(vault) else {
        return Vec::new();
    };
    let Some(schema) = schemas.resolve(fragment_type) else {
        return Vec::new();
    };
    schema
        .fields
        .iter()
        .find(|field| field.name == field_key)
        .and_then(|field| match &field.field_type {
            FieldType::Enum(values) => Some(values.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn known_assignees(current_rows: &[Row], search_rows: &[Row]) -> Vec<String> {
    let mut values = Vec::new();
    for row in current_rows.iter().chain(search_rows) {
        let Some(assignee) = row.assignee.as_deref().filter(|value| !value.is_empty()) else {
            continue;
        };
        if !values.iter().any(|value: &String| value == assignee) {
            values.push(assignee.to_string());
        }
    }
    values
}

fn parse_field_intent(input: &str) -> Option<(QuickField, &str)> {
    let input = input.trim_start();
    for (prefix, field) in [
        ("set status ", QuickField::Status),
        ("status ", QuickField::Status),
        ("set due ", QuickField::Due),
        ("due ", QuickField::Due),
        ("deadline ", QuickField::Due),
        ("set priority ", QuickField::Priority),
        ("priority ", QuickField::Priority),
        ("pri ", QuickField::Priority),
        ("set assignee ", QuickField::Assignee),
        ("assignee ", QuickField::Assignee),
        ("assign ", QuickField::Assignee),
    ] {
        if let Some(value) = strip_ascii_prefix(input, prefix) {
            return Some((field, value));
        }
    }
    None
}

fn strip_ascii_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    input
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        .then(|| &input[prefix.len()..])
}

fn execute_intent_action(
    app: &mut App,
    action: IntentAction,
    vault: &Path,
    config: &Config,
) -> Result<()> {
    match action {
        IntentAction::SetField { field, value } => {
            let Some(id) = app.selected_id() else {
                app.set_status("no selected fragment");
                return Ok(());
            };

            let result = actions::set_field(&mut app.writer, &id, field, &value);
            let what = format!("{} update", field.key());
            finish_write(app, vault, config, &id, &what, result);
        }
        IntentAction::AddTags { tags } => {
            apply_tag_intent(app, vault, config, |existing| {
                for tag in tags {
                    if !existing.contains(&tag) {
                        existing.push(tag);
                    }
                }
            })?;
        }
        IntentAction::RemoveTags { tags } => {
            apply_tag_intent(app, vault, config, |existing| {
                existing.retain(|tag| !tags.contains(tag));
            })?;
        }
        IntentAction::SetTags { tags } => {
            apply_tag_intent(app, vault, config, |existing| {
                *existing = tags;
            })?;
        }
    }
    Ok(())
}

fn apply_tag_intent(
    app: &mut App,
    vault: &Path,
    config: &Config,
    update: impl FnOnce(&mut Vec<String>),
) -> Result<()> {
    let Some(row) = app.selected_row() else {
        app.set_status("no selected fragment");
        return Ok(());
    };
    let id = row.id.clone();
    let mut tags = row.tags.clone();
    update(&mut tags);
    let value = tags.join(" ");

    let result = actions::set_field(&mut app.writer, &id, QuickField::Tags, &value);
    finish_write(app, vault, config, &id, "tags update", result);
    Ok(())
}

fn launcher_item_score(item: &LauncherItem, query: &str) -> i64 {
    match item {
        LauncherItem::Intent(_) => 980_000,
        LauncherItem::Command(command) => command.match_score(query).unwrap_or(0),
        LauncherItem::Fragment(row) => fragment_result_score(row, query),
    }
}

fn fragment_result_score(row: &Row, query: &str) -> i64 {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return row.score as i64;
    }

    let title = row.title.to_lowercase();
    if title == query {
        940_000
    } else if title.starts_with(&query) {
        900_000
    } else if title.split_whitespace().any(|word| word == query) {
        850_000
    } else if title.contains(&query) {
        800_000
    } else {
        100_000 + i64::from(row.score.min(100_000))
    }
}

fn execute_command_action(
    app: &mut App,
    action: CommandAction,
    terminal: &mut Term,
    vault: &Path,
    config: &Config,
) -> Result<()> {
    match action {
        CommandAction::Edit => {
            if let Some(id) = app.selected_id() {
                edit_id(app, terminal, vault, config, &id);
            }
        }
        CommandAction::ToggleStatus => {
            if let Some(id) = app.selected_id() {
                toggle_status_id(app, vault, config, &id);
            }
        }
        CommandAction::Archive => {
            if let Some(id) = app.selected_id() {
                archive_id(app, vault, config, &id);
            }
        }
        CommandAction::Delete => {
            if let Some(id) = app.selected_id() {
                open_delete_confirm(app, id);
            }
        }
        CommandAction::Promote => {
            if let Some(id) = app.selected_id() {
                open_promote_input(app, id);
            }
        }
        CommandAction::YankId => {
            if let Some(id) = app.selected_id() {
                yank_id(app, &id);
            }
        }
        CommandAction::SetField(field) => start_field_input(app, field),
        CommandAction::Capture => open_capture_form(app, vault),
        CommandAction::Reload => manual_reload(app, vault, config),
        CommandAction::Help => {
            app.mode = Mode::Help;
            app.dirty = true;
        }
        CommandAction::SwitchTab(tab) => {
            app.clear_status();
            switch_tab(app, tab, vault, config);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tui::app::tests::{row, test_vault};

    #[test]
    fn launcher_intents_parse_selected_field_updates() {
        let (_tmp, vault) = test_vault();
        let selected = row("todo", "selected", None);
        let intents = launcher_intents("status done", Some(&selected), &[], &[], &vault);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].label, "Set Status to done");
        assert_eq!(
            intents[0].action,
            IntentAction::SetField {
                field: QuickField::Status,
                value: "done".into()
            }
        );

        let intents = launcher_intents("due next friday", Some(&selected), &[], &[], &vault);
        assert_eq!(
            intents[0].action,
            IntentAction::SetField {
                field: QuickField::Due,
                value: "next friday".into()
            }
        );
    }

    #[test]
    fn launcher_intents_require_selection_and_value() {
        let (_tmp, vault) = test_vault();
        let selected = row("todo", "selected", None);
        assert!(launcher_intents("status done", None, &[], &[], &vault).is_empty());
        assert!(launcher_intents("status", Some(&selected), &[], &[], &vault).is_empty());
        assert!(launcher_intents("tui", Some(&selected), &[], &[], &vault).is_empty());
    }

    #[test]
    fn launcher_intents_suggest_schema_enum_values() {
        let (_tmp, vault) = test_vault();
        let selected = row("todo", "selected", None);

        let labels = launcher_intents("status ", Some(&selected), &[], &[], &vault)
            .into_iter()
            .map(|intent| intent.label)
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            vec![
                "Set Status to open",
                "Set Status to in-progress",
                "Set Status to done",
                "Set Status to cancelled"
            ]
        );

        let labels = launcher_intents("priority h", Some(&selected), &[], &[], &vault)
            .into_iter()
            .map(|intent| intent.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["Set Priority to high"]);
    }

    #[test]
    fn launcher_intents_suggest_due_and_assignee_values() {
        let (_tmp, vault) = test_vault();
        let selected = row("todo", "selected", None);

        let labels = launcher_intents("due tom", Some(&selected), &[], &[], &vault)
            .into_iter()
            .map(|intent| intent.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["Set Due to tomorrow"]);

        let current = vec![
            row("todo", "a", Some("alice")),
            row("todo", "b", Some("bob")),
        ];
        let labels = launcher_intents("assignee a", Some(&selected), &current, &[], &vault)
            .into_iter()
            .map(|intent| intent.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["Set Assignee to alice"]);
    }

    #[test]
    fn launcher_intents_handle_explicit_tag_actions() {
        let (_tmp, vault) = test_vault();
        let mut selected = row("todo", "selected", None);
        selected.tags = vec!["tui".to_string()];
        let mut tagged = row("todo", "tagged", None);
        tagged.tags = vec!["design".to_string(), "search".to_string()];
        let current = vec![tagged];

        let intent = launcher_intents("add tag des", Some(&selected), &current, &[], &vault)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            intent.action,
            IntentAction::AddTags {
                tags: vec!["design".into()]
            }
        );

        let intent = launcher_intents("remove tag ", Some(&selected), &current, &[], &vault)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(
            intent.action,
            IntentAction::RemoveTags {
                tags: vec!["tui".into()]
            }
        );

        let intent = launcher_intents(
            "set tags ui backend",
            Some(&selected),
            &current,
            &[],
            &vault,
        )
        .into_iter()
        .next()
        .unwrap();
        assert_eq!(
            intent.action,
            IntentAction::SetTags {
                tags: vec!["ui".into(), "backend".into()]
            }
        );
    }

    #[test]
    fn launcher_intents_validate_and_preview_values() {
        let (_tmp, vault) = test_vault();
        let selected = row("todo", "selected", None);

        let intent = launcher_intents("due tomorrow", Some(&selected), &[], &[], &vault)
            .into_iter()
            .next()
            .unwrap();
        assert!(intent.valid);
        assert_eq!(
            intent.detail,
            Some(format!(
                "Will store due date `{}`.",
                date::resolve_due_date("tomorrow").unwrap()
            ))
        );

        let intent = launcher_intents("due someday", Some(&selected), &[], &[], &vault)
            .into_iter()
            .next()
            .unwrap();
        assert!(!intent.valid);
        assert!(intent
            .detail
            .as_deref()
            .unwrap()
            .contains("invalid due date"));

        let intent = launcher_intents("status nope", Some(&selected), &[], &[], &vault)
            .into_iter()
            .next()
            .unwrap();
        assert!(!intent.valid);
        assert!(intent.detail.as_deref().unwrap().contains("Allowed values"));
    }
}
