use std::io::Stdout;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use parc_core::config::Config;
use parc_core::schema::load_schemas;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;

use super::cache::FragmentCache;
use super::command::{self, CommandAction, LauncherKind};
use super::markdown::{ActionKind, Actionable};
use super::{
    actions, data, ui, CaptureField, CaptureForm, ConfirmAction, Focus, InputAction, LauncherPopup,
    Mode, OverlayKind, OverlayState, QuickField, Row, Tab,
};

const PAGE_LINES: u16 = 10;
const HALF_PAGE_LINES: u16 = 5;
const FRAGMENT_CACHE_CAP: usize = 64;
const IDLE_POLL_SECS: u64 = 60;
const STATUS_LIFETIME_SECS: u64 = 4;
const TAB_COUNT: usize = 3;

pub(super) enum Status {
    Idle,
    Active { text: String, expires_at: Instant },
}

impl Status {
    pub(super) fn text(&self) -> &str {
        match self {
            Status::Active { text, expires_at } if *expires_at > Instant::now() => text,
            _ => "",
        }
    }

    fn deadline(&self) -> Option<Instant> {
        match self {
            Status::Active { expires_at, .. } => Some(*expires_at),
            _ => None,
        }
    }

    fn is_expired(&self) -> bool {
        matches!(self, Status::Active { expires_at, .. } if *expires_at <= Instant::now())
    }
}

type Term = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone, Default)]
struct TabViewState {
    selected_id: Option<String>,
    selected_index: Option<usize>,
    detail_scroll: u16,
}

pub(super) struct App {
    pub tab: Tab,
    pub focus: Focus,
    pub list_state: ListState,
    pub detail_scroll: u16,
    pub detail_max_scroll: u16,
    pub launcher_input: String,
    pub rows: Vec<super::Row>,
    pub status: Status,
    pub mode: Mode,
    pub dirty: bool,
    pub search: data::SearchState,
    pub cache: FragmentCache,
    /// Actionables (wiki-links, checkboxes) in the currently rendered detail
    /// body. Refreshed by `ui::draw_detail` each render. Logical-line indices
    /// are body-relative; add `detail_body_offset` to translate to
    /// `lines`-vector indices.
    pub detail_items: Vec<Actionable>,
    pub detail_body_offset: usize,
    /// Last-rendered viewport height of the detail pane's inner area.
    /// Used by overlay visibility filtering. `0` until the first render.
    pub detail_viewport: u16,
    tab_states: [TabViewState; TAB_COUNT],
}

impl App {
    fn new(rows: Vec<super::Row>) -> Self {
        let mut list_state = ListState::default();
        if !rows.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            tab: Tab::Today,
            focus: Focus::List,
            list_state,
            detail_scroll: 0,
            detail_max_scroll: 0,
            launcher_input: String::new(),
            rows,
            status: Status::Idle,
            mode: Mode::Normal,
            dirty: true,
            search: data::SearchState::new(),
            cache: FragmentCache::new(FRAGMENT_CACHE_CAP),
            detail_items: Vec::new(),
            detail_body_offset: 0,
            detail_viewport: 0,
            tab_states: std::array::from_fn(|_| TabViewState::default()),
        }
    }

    fn save_tab_state(&mut self) {
        let selected_index = self.list_state.selected();
        self.tab_states[self.tab.index()] = TabViewState {
            selected_id: selected_index.and_then(|idx| self.rows.get(idx).map(|r| r.id.clone())),
            selected_index,
            detail_scroll: self.detail_scroll,
        };
    }

    fn restore_tab_state(&mut self) {
        let state = &self.tab_states[self.tab.index()];
        let selected_by_id = state
            .selected_id
            .as_ref()
            .and_then(|id| self.rows.iter().position(|row| &row.id == id));
        let selected = selected_by_id
            .or_else(|| state.selected_index.filter(|idx| *idx < self.rows.len()))
            .or_else(|| if self.rows.is_empty() { None } else { Some(0) });
        self.list_state.select(selected);
        self.detail_scroll = state.detail_scroll;
    }

    fn select(&mut self, idx: Option<usize>) {
        if self.list_state.selected() != idx {
            self.list_state.select(idx);
            self.detail_scroll = 0;
            self.dirty = true;
        }
    }

    fn move_list(&mut self, delta: i32) {
        let len = self.rows.len();
        if len == 0 {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
        if Some(next) != self.list_state.selected() {
            self.list_state.select(Some(next));
            self.detail_scroll = 0;
            self.dirty = true;
        }
    }

    fn scroll_detail(&mut self, delta: i32) {
        let cur = self.detail_scroll as i32;
        let max = self.detail_max_scroll as i32;
        let next = (cur + delta).clamp(0, max) as u16;
        if next != self.detail_scroll {
            self.detail_scroll = next;
            self.dirty = true;
        }
    }

    fn selected_id(&self) -> Option<String> {
        let idx = self.list_state.selected()?;
        self.rows.get(idx).map(|r| r.id.clone())
    }

    fn selected_row(&self) -> Option<&Row> {
        let idx = self.list_state.selected()?;
        self.rows.get(idx)
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Status::Active {
            text: msg.into(),
            expires_at: Instant::now() + Duration::from_secs(STATUS_LIFETIME_SECS),
        };
        self.dirty = true;
    }

    fn clear_status(&mut self) {
        if !matches!(self.status, Status::Idle) {
            self.status = Status::Idle;
            self.dirty = true;
        }
    }

    pub(super) fn detail_viewport_height(&self) -> u16 {
        self.detail_viewport
    }
}

pub(super) fn run_loop(terminal: &mut Term, vault: &Path, config: &Config) -> Result<()> {
    let initial_rows = data::load_rows(vault, Tab::Today, config)?;
    let mut app = App::new(initial_rows);

    loop {
        clamp_selection(&mut app);

        if app.dirty {
            terminal.draw(|frame| {
                ui::draw(frame, vault, config, &mut app);
            })?;
            app.dirty = false;
        }

        let now = Instant::now();
        let timeout = match app.status.deadline() {
            Some(deadline) => deadline.saturating_duration_since(now),
            None => Duration::from_secs(IDLE_POLL_SECS),
        };

        if !event::poll(timeout)? {
            if app.status.is_expired() {
                app.clear_status();
            }
            continue;
        }

        let event = event::read()?;
        let keep_going = match event {
            Event::Key(key) => match app.mode.clone() {
                Mode::Normal => handle_normal(&mut app, key, terminal, vault, config)?,
                Mode::Confirm { action, .. } => {
                    handle_confirm(&mut app, key, action, vault, config)?
                }
                Mode::Input {
                    value,
                    action,
                    prompt,
                } => handle_input(&mut app, key, prompt, value, action, vault, config)?,
                Mode::Capture(form) => handle_capture(&mut app, key, form, vault, config)?,
                Mode::Launcher(popup) => {
                    handle_launcher(&mut app, key, popup, terminal, vault, config)?
                }
                Mode::Overlay(state) => handle_overlay(&mut app, key, state)?,
                Mode::Help => handle_help(&mut app, key)?,
            },
            Event::Resize(_, _) => {
                app.dirty = true;
                true
            }
            _ => true,
        };
        if !keep_going {
            break;
        }
    }

    Ok(())
}

/// Returns `false` if the loop should exit.
fn handle_normal(
    app: &mut App,
    key: KeyEvent,
    terminal: &mut Term,
    vault: &Path,
    config: &Config,
) -> Result<bool> {
    let plain = key.modifiers.is_empty();
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), m) if m.is_empty() => return Ok(false),
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => return Ok(false),

        (KeyCode::Tab, _) => {
            switch_tab(app, app.tab.next(), vault, config)?;
            app.clear_status();
        }
        (KeyCode::BackTab, _) => {
            app.focus = app.focus.toggle();
            app.dirty = true;
        }
        (KeyCode::Char('1'), _) => switch_tab(app, Tab::Today, vault, config)?,
        (KeyCode::Char('2'), _) => switch_tab(app, Tab::List, vault, config)?,
        (KeyCode::Char('3'), _) => switch_tab(app, Tab::Stale, vault, config)?,
        (KeyCode::Char('4'), _) | (KeyCode::Char('/'), _) => open_launcher(app, vault, ""),
        (KeyCode::Char('p') | KeyCode::Char('P'), m) if m.contains(KeyModifiers::CONTROL) => {
            open_launcher(app, vault, ">")
        }

        (KeyCode::Char('r'), _) if plain => {
            reload_rows(app, vault, config)?;
            app.set_status("reloaded");
        }

        (KeyCode::Char('?'), _) => {
            app.mode = Mode::Help;
            app.dirty = true;
        }
        (KeyCode::Char('c'), _) if plain => open_capture_form(app, vault),

        (KeyCode::Char('e'), _) if plain => {
            if let Some(id) = app.selected_id() {
                edit_id(app, terminal, vault, config, &id)?;
            }
        }
        (KeyCode::Char('t'), _) if plain => {
            if let Some(id) = app.selected_id() {
                toggle_status_id(app, vault, config, &id)?;
            }
        }
        (KeyCode::Char('a'), _) if plain => {
            if let Some(id) = app.selected_id() {
                archive_id(app, vault, config, &id)?;
            }
        }
        (KeyCode::Char('y'), _) if plain => {
            if let Some(id) = app.selected_id() {
                yank_id(app, &id);
            }
        }
        (KeyCode::Char('d'), _) if plain => {
            if let Some(id) = app.selected_id() {
                open_delete_confirm(app, id);
            }
        }
        (KeyCode::Char('p'), _) if plain => {
            if let Some(id) = app.selected_id() {
                open_promote_input(app, id);
            }
        }
        (KeyCode::Char('s'), _) if plain => start_field_input(app, QuickField::Status),
        (KeyCode::Char('D'), _) => start_field_input(app, QuickField::Due),
        (KeyCode::Char('P'), _) => start_field_input(app, QuickField::Priority),
        (KeyCode::Char('@'), _) => start_field_input(app, QuickField::Assignee),
        (KeyCode::Char('#'), _) => start_field_input(app, QuickField::Tags),

        (KeyCode::Char('f'), _) if plain && app.focus == Focus::Detail => {
            open_overlay(app, OverlayKind::FollowLink);
        }
        (KeyCode::Char('x'), _) if plain && app.focus == Focus::Detail => {
            open_overlay(app, OverlayKind::ToggleCheckbox);
        }

        (KeyCode::Down, _) => match app.focus {
            Focus::List => app.move_list(1),
            Focus::Detail => app.scroll_detail(1),
        },
        (KeyCode::Up, _) => match app.focus {
            Focus::List => app.move_list(-1),
            Focus::Detail => app.scroll_detail(-1),
        },
        (KeyCode::PageDown, _) => match app.focus {
            Focus::List => app.move_list(PAGE_LINES as i32),
            Focus::Detail => app.scroll_detail(PAGE_LINES as i32),
        },
        (KeyCode::PageUp, _) => match app.focus {
            Focus::List => app.move_list(-(PAGE_LINES as i32)),
            Focus::Detail => app.scroll_detail(-(PAGE_LINES as i32)),
        },
        (KeyCode::Char('d'), _) if ctrl => match app.focus {
            Focus::List => app.move_list(HALF_PAGE_LINES as i32),
            Focus::Detail => app.scroll_detail(HALF_PAGE_LINES as i32),
        },
        (KeyCode::Char('u'), _) if ctrl => match app.focus {
            Focus::List => app.move_list(-(HALF_PAGE_LINES as i32)),
            Focus::Detail => app.scroll_detail(-(HALF_PAGE_LINES as i32)),
        },
        (KeyCode::Home, _) => match app.focus {
            Focus::List => app.select(if app.rows.is_empty() { None } else { Some(0) }),
            Focus::Detail => {
                app.detail_scroll = 0;
                app.dirty = true;
            }
        },
        (KeyCode::End, _) => match app.focus {
            Focus::List => {
                let last = app.rows.len().saturating_sub(1);
                app.select(if app.rows.is_empty() {
                    None
                } else {
                    Some(last)
                });
            }
            Focus::Detail => {
                app.detail_scroll = app.detail_max_scroll;
                app.dirty = true;
            }
        },

        _ => {}
    }

    Ok(true)
}

fn handle_launcher(
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
        KeyCode::Enter => match popup.kind() {
            LauncherKind::Fragments => {
                if let Some(id) = popup.selected_id() {
                    edit_id(app, terminal, vault, config, &id)?;
                    reload_launcher_popup(app, &mut popup, vault, false);
                }
            }
            LauncherKind::Commands => {
                if let Some(command) = popup.selected_command() {
                    app.launcher_input = popup.input;
                    app.mode = Mode::Normal;
                    execute_command_action(app, command.action, terminal, vault, config)?;
                    app.dirty = true;
                    return Ok(true);
                }
            }
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

fn handle_confirm(
    app: &mut App,
    key: KeyEvent,
    action: ConfirmAction,
    vault: &Path,
    config: &Config,
) -> Result<bool> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            match action {
                ConfirmAction::Delete { id } => match actions::delete(vault, &id) {
                    Ok(msg) => {
                        app.cache.invalidate(&id);
                        app.search.mark_stale();
                        reload_rows(app, vault, config)?;
                        app.set_status(msg);
                    }
                    Err(e) => app.set_status(format!("delete failed: {}", e)),
                },
            }
            app.mode = Mode::Normal;
            app.dirty = true;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.set_status("cancelled");
        }
        _ => {}
    }
    Ok(true)
}

fn handle_input(
    app: &mut App,
    key: KeyEvent,
    prompt: String,
    value: String,
    action: InputAction,
    vault: &Path,
    config: &Config,
) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.set_status("cancelled");
        }
        KeyCode::Enter => {
            let trimmed = value.trim().to_string();
            match action {
                InputAction::Promote { id } => {
                    if trimmed.is_empty() {
                        app.mode = Mode::Normal;
                        app.set_status("cancelled");
                    } else {
                        match actions::promote(vault, &id, &trimmed) {
                            Ok(msg) => {
                                app.cache.invalidate(&id);
                                app.search.mark_stale();
                                reload_rows(app, vault, config)?;
                                app.set_status(msg);
                            }
                            Err(e) => app.set_status(format!("promote failed: {}", e)),
                        }
                    }
                }
                InputAction::SetField { id, field } => {
                    match actions::set_field(vault, &id, field, &trimmed) {
                        Ok(msg) => {
                            app.cache.invalidate(&id);
                            app.search.mark_stale();
                            reload_rows(app, vault, config)?;
                            select_row_by_id(app, &id);
                            app.set_status(msg);
                        }
                        Err(e) => app.set_status(format!("{} update failed: {}", field.key(), e)),
                    }
                }
            }
            app.mode = Mode::Normal;
            app.dirty = true;
        }
        KeyCode::Backspace => {
            let mut new_value = value;
            new_value.pop();
            app.mode = Mode::Input {
                prompt,
                value: new_value,
                action,
            };
            app.dirty = true;
        }
        KeyCode::Char(c) if !ctrl => {
            let mut new_value = value;
            new_value.push(c);
            app.mode = Mode::Input {
                prompt,
                value: new_value,
                action,
            };
            app.dirty = true;
        }
        _ => {}
    }
    Ok(true)
}

fn handle_capture(
    app: &mut App,
    key: KeyEvent,
    mut form: CaptureForm,
    vault: &Path,
    config: &Config,
) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.set_status("cancelled");
        }
        KeyCode::Tab => {
            form.next_field();
            app.mode = Mode::Capture(form);
            app.dirty = true;
        }
        KeyCode::BackTab => {
            form.previous_field();
            app.mode = Mode::Capture(form);
            app.dirty = true;
        }
        KeyCode::Left | KeyCode::Up if form.focus == CaptureField::Type => {
            form.previous_type();
            app.mode = Mode::Capture(form);
            app.dirty = true;
        }
        KeyCode::Right | KeyCode::Down if form.focus == CaptureField::Type => {
            form.next_type();
            app.mode = Mode::Capture(form);
            app.dirty = true;
        }
        KeyCode::Backspace => {
            form.backspace();
            app.mode = Mode::Capture(form);
            app.dirty = true;
        }
        KeyCode::Enter => match submit_capture(app, &form, vault, config) {
            Ok(()) => {}
            Err(err) => {
                app.mode = Mode::Capture(form);
                app.set_status(format!("capture failed: {}", err));
            }
        },
        KeyCode::Char(ch) if !ctrl => {
            form.push_char(ch);
            app.mode = Mode::Capture(form);
            app.dirty = true;
        }
        _ => {
            app.mode = Mode::Capture(form);
        }
    }
    Ok(true)
}

fn submit_capture(app: &mut App, form: &CaptureForm, vault: &Path, config: &Config) -> Result<()> {
    let input = actions::CaptureInput {
        text: form.text.clone(),
        fragment_type: form.current_type().to_string(),
        tags: form.tags.clone(),
        status: form.status.clone(),
        due: form.due.clone(),
        priority: form.priority.clone(),
        assignee: form.assignee.clone(),
    };
    let (id, msg) = actions::capture(vault, input)?;
    app.mode = Mode::Normal;
    app.search.mark_stale();
    reload_rows(app, vault, config)?;
    select_row_by_id(app, &id);
    app.set_status(msg);
    Ok(())
}

fn open_launcher(app: &mut App, vault: &Path, input: &str) {
    let mut popup = LauncherPopup::new(input.to_string());
    reload_launcher_popup(app, &mut popup, vault, true);
    app.mode = Mode::Launcher(popup);
    app.clear_status();
    app.dirty = true;
}

fn open_capture_form(app: &mut App, vault: &Path) {
    match new_capture_form(vault) {
        Ok(form) => {
            app.mode = Mode::Capture(form);
            app.dirty = true;
        }
        Err(err) => app.set_status(format!("capture unavailable: {}", err)),
    }
}

fn open_delete_confirm(app: &mut App, id: String) {
    let prompt = format!("Delete {}? (y/n)", &id[..8.min(id.len())]);
    app.mode = Mode::Confirm {
        prompt,
        action: ConfirmAction::Delete { id },
    };
    app.dirty = true;
}

fn open_promote_input(app: &mut App, id: String) {
    app.mode = Mode::Input {
        prompt: "Promote to type:".to_string(),
        value: String::new(),
        action: InputAction::Promote { id },
    };
    app.dirty = true;
}

fn start_field_input(app: &mut App, field: QuickField) {
    let Some(row) = app.selected_row() else {
        return;
    };
    app.mode = Mode::Input {
        prompt: format!("{}:", field.label()),
        value: quick_field_value(row, field),
        action: InputAction::SetField {
            id: row.id.clone(),
            field,
        },
    };
    app.dirty = true;
}

fn quick_field_value(row: &Row, field: QuickField) -> String {
    match field {
        QuickField::Status => row.status.clone().unwrap_or_default(),
        QuickField::Due => row.due.clone().unwrap_or_default(),
        QuickField::Priority => row.priority.clone().unwrap_or_default(),
        QuickField::Assignee => row.assignee.clone().unwrap_or_default(),
        QuickField::Tags => row.tags.join(" "),
    }
}

fn reload_launcher_popup(
    app: &mut App,
    popup: &mut LauncherPopup,
    vault: &Path,
    reset_selection: bool,
) {
    match popup.kind() {
        LauncherKind::Fragments => reload_fragment_results(app, popup, vault, reset_selection),
        LauncherKind::Commands => reload_command_results(app, popup, reset_selection),
    }
}

fn reload_fragment_results(
    app: &mut App,
    popup: &mut LauncherPopup,
    vault: &Path,
    reset_selection: bool,
) {
    let selected_id = if reset_selection {
        None
    } else {
        popup.selected_id()
    };
    popup.commands.clear();

    match data::load_search_rows(vault, &popup.input, &mut app.search) {
        Ok(rows) => {
            popup.rows = rows;
            popup.error = None;
            if let Some(id) = selected_id {
                if let Some(idx) = popup.rows.iter().position(|row| row.id == id) {
                    popup.list_state.select(Some(idx));
                } else {
                    popup.select_first();
                }
            } else {
                popup.select_first();
            }
        }
        Err(err) => {
            popup.rows.clear();
            popup.error = Some(err.to_string());
            popup.select_first();
        }
    }
}

fn reload_command_results(app: &App, popup: &mut LauncherPopup, reset_selection: bool) {
    let selected_action = if reset_selection {
        None
    } else {
        popup.selected_command().map(|command| command.action)
    };
    popup.rows.clear();
    popup.error = None;
    popup.commands = command::matching_commands(&popup.input, app.selected_id().is_some());

    if let Some(action) = selected_action {
        if let Some(idx) = popup
            .commands
            .iter()
            .position(|command| command.action == action)
        {
            popup.list_state.select(Some(idx));
            return;
        }
    }

    popup.select_first();
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
                edit_id(app, terminal, vault, config, &id)?;
            }
        }
        CommandAction::ToggleStatus => {
            if let Some(id) = app.selected_id() {
                toggle_status_id(app, vault, config, &id)?;
            }
        }
        CommandAction::Archive => {
            if let Some(id) = app.selected_id() {
                archive_id(app, vault, config, &id)?;
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
        CommandAction::Reload => {
            reload_rows(app, vault, config)?;
            app.set_status("reloaded");
        }
        CommandAction::Help => {
            app.mode = Mode::Help;
            app.dirty = true;
        }
        CommandAction::SwitchTab(tab) => {
            switch_tab(app, tab, vault, config)?;
            app.clear_status();
        }
    }

    Ok(())
}

fn edit_id(
    app: &mut App,
    terminal: &mut Term,
    vault: &Path,
    config: &Config,
    id: &str,
) -> Result<()> {
    match actions::edit(terminal, vault, id) {
        Ok(msg) => {
            app.cache.invalidate(id);
            app.search.mark_stale();
            reload_rows(app, vault, config)?;
            app.set_status(msg);
        }
        Err(e) => app.set_status(format!("edit failed: {}", e)),
    }
    Ok(())
}

fn toggle_status_id(app: &mut App, vault: &Path, config: &Config, id: &str) -> Result<()> {
    match actions::toggle_status(vault, id) {
        Ok(msg) => {
            app.cache.invalidate(id);
            app.search.mark_stale();
            reload_rows(app, vault, config)?;
            app.set_status(msg);
        }
        Err(e) => app.set_status(format!("toggle failed: {}", e)),
    }
    Ok(())
}

fn archive_id(app: &mut App, vault: &Path, config: &Config, id: &str) -> Result<()> {
    match actions::archive(vault, id) {
        Ok(msg) => {
            app.cache.invalidate(id);
            app.search.mark_stale();
            reload_rows(app, vault, config)?;
            app.set_status(msg);
        }
        Err(e) => app.set_status(format!("archive failed: {}", e)),
    }
    Ok(())
}

fn yank_id(app: &mut App, id: &str) {
    match actions::yank(id) {
        Ok(msg) => app.set_status(msg),
        Err(e) => app.set_status(format!("{} (id: {})", e, id)),
    }
}

fn open_overlay(app: &mut App, kind: OverlayKind) {
    let viewport = app.detail_viewport_height();
    let visible = visible_actionables(
        &app.detail_items,
        app.detail_body_offset,
        app.detail_scroll,
        viewport,
        kind,
    );

    if visible.is_empty() {
        let what = match kind {
            OverlayKind::FollowLink => "no links in view",
            OverlayKind::ToggleCheckbox => "no checkboxes in view",
        };
        app.set_status(what);
        return;
    }

    if visible.len() == 1 {
        invoke_actionable(app, kind, visible[0]);
        return;
    }

    let labels: Vec<(char, usize)> = visible
        .into_iter()
        .zip(LABEL_ALPHABET.chars())
        .map(|(idx, ch)| (ch, idx))
        .collect();

    app.mode = Mode::Overlay(OverlayState { kind, labels });
    app.dirty = true;
}

fn handle_overlay(app: &mut App, key: KeyEvent, state: OverlayState) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => return Ok(false),
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.set_status("cancelled");
        }
        KeyCode::Char(ch) => {
            let target = state
                .labels
                .iter()
                .find(|(label, _)| *label == ch.to_ascii_lowercase())
                .map(|(_, idx)| *idx);
            match target {
                Some(idx) => {
                    app.mode = Mode::Normal;
                    invoke_actionable(app, state.kind, idx);
                }
                None => {
                    app.mode = Mode::Overlay(state);
                }
            }
        }
        _ => {
            app.mode = Mode::Overlay(state);
        }
    }
    Ok(true)
}

/// Step 3 stub: just set a status message describing what would happen.
/// Steps 4/5 wire actual checkbox toggle and link follow.
fn invoke_actionable(app: &mut App, kind: OverlayKind, item_idx: usize) {
    let Some(item) = app.detail_items.get(item_idx).cloned() else {
        return;
    };
    let msg = match (kind, &item.kind) {
        (OverlayKind::FollowLink, ActionKind::WikiLink { target, .. }) => {
            format!("would follow [[{}]]", target)
        }
        (OverlayKind::ToggleCheckbox, ActionKind::Checkbox { checked }) => {
            format!("would toggle checkbox ({})", if *checked { "✓→·" } else { "·→✓" })
        }
        _ => "nothing to do".to_string(),
    };
    app.set_status(msg);
}

const LABEL_ALPHABET: &str = "abcdefghijklmnopqrstuvwxyz";

/// Returns indices into `items` that (a) match the overlay kind and
/// (b) sit in the currently visible viewport.
fn visible_actionables(
    items: &[Actionable],
    body_offset: usize,
    detail_scroll: u16,
    viewport: u16,
    kind: OverlayKind,
) -> Vec<usize> {
    let top = detail_scroll as usize;
    let bottom = top.saturating_add(viewport as usize);
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| match (kind, &item.kind) {
            (OverlayKind::FollowLink, ActionKind::WikiLink { .. }) => true,
            (OverlayKind::ToggleCheckbox, ActionKind::Checkbox { .. }) => true,
            _ => false,
        })
        .filter(|(_, item)| {
            let line = item.logical_line + body_offset;
            line >= top && line < bottom
        })
        .map(|(idx, _)| idx)
        .take(LABEL_ALPHABET.len())
        .collect()
}

fn handle_help(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
            app.mode = Mode::Normal;
            app.dirty = true;
        }
        _ => {}
    }
    Ok(true)
}

fn switch_tab(app: &mut App, tab: Tab, vault: &Path, config: &Config) -> Result<()> {
    if app.tab == tab {
        return Ok(());
    }

    app.save_tab_state();
    app.tab = tab;
    reload_rows(app, vault, config)?;
    app.restore_tab_state();
    app.dirty = true;
    Ok(())
}

fn reload_rows(app: &mut App, vault: &Path, config: &Config) -> Result<()> {
    app.rows = data::load_rows(vault, app.tab, config)?;
    Ok(())
}

fn new_capture_form(vault: &Path) -> Result<CaptureForm> {
    let schemas = load_schemas(vault)?;
    let types = schemas
        .list()
        .into_iter()
        .map(|schema| schema.name.clone())
        .collect();
    Ok(CaptureForm::new(types))
}

fn select_row_by_id(app: &mut App, id: &str) {
    if let Some(idx) = app.rows.iter().position(|row| row.id == id) {
        app.select(Some(idx));
    }
}

fn clamp_selection(app: &mut App) {
    if app.rows.is_empty() {
        app.list_state.select(None);
        return;
    }
    let cur = app.list_state.selected().unwrap_or(0);
    if app.list_state.selected().is_none() {
        app.list_state.select(Some(0));
    } else if cur >= app.rows.len() {
        app.list_state.select(Some(app.rows.len() - 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tui::markdown::{ActionKind, Actionable};

    fn link(line: usize) -> Actionable {
        Actionable {
            kind: ActionKind::WikiLink {
                target: format!("ID{}", line),
                display_text: None,
            },
            logical_line: line,
            source_range: 0..0,
        }
    }

    fn checkbox(line: usize) -> Actionable {
        Actionable {
            kind: ActionKind::Checkbox { checked: false },
            logical_line: line,
            source_range: 0..0,
        }
    }

    #[test]
    fn visible_filter_picks_in_range_links_only() {
        // viewport rows [scroll..scroll+height); body_offset adds to each item.
        let items = vec![link(0), checkbox(2), link(4), link(20)];
        let visible =
            visible_actionables(&items, /*body_offset=*/ 5, /*scroll=*/ 5, /*viewport=*/ 10, OverlayKind::FollowLink);
        // line indices: 0+5=5 (in), 2+5=7 (in but checkbox), 4+5=9 (in), 20+5=25 (out).
        assert_eq!(visible, vec![0, 2]);
    }

    #[test]
    fn visible_filter_caps_at_alphabet_size() {
        let items: Vec<Actionable> = (0..40).map(link).collect();
        let visible =
            visible_actionables(&items, 0, 0, 100, OverlayKind::FollowLink);
        assert_eq!(visible.len(), LABEL_ALPHABET.len());
    }

    #[test]
    fn visible_filter_returns_empty_when_scrolled_past() {
        let items = vec![link(0), link(1)];
        let visible = visible_actionables(&items, 0, 50, 10, OverlayKind::FollowLink);
        assert!(visible.is_empty());
    }
}
