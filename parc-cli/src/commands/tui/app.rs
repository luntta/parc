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
use super::{
    actions, data, ui, CaptureField, CaptureForm, ConfirmAction, Focus, InputAction, Mode,
    SearchPopup, Tab,
};

const PAGE_LINES: u16 = 10;
const HALF_PAGE_LINES: u16 = 5;
const FRAGMENT_CACHE_CAP: usize = 64;
const IDLE_POLL_SECS: u64 = 60;
const STATUS_LIFETIME_SECS: u64 = 4;

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

pub(super) struct App {
    pub tab: Tab,
    pub focus: Focus,
    pub list_state: ListState,
    pub detail_scroll: u16,
    pub detail_max_scroll: u16,
    pub search_input: String,
    pub rows: Vec<super::Row>,
    pub status: Status,
    pub mode: Mode,
    pub dirty: bool,
    pub search: data::SearchState,
    pub cache: FragmentCache,
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
            search_input: String::new(),
            rows,
            status: Status::Idle,
            mode: Mode::Normal,
            dirty: true,
            search: data::SearchState::new(),
            cache: FragmentCache::new(FRAGMENT_CACHE_CAP),
        }
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
                Mode::Search(popup) => {
                    handle_search(&mut app, key, popup, terminal, vault, config)?
                }
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
            app.tab = app.tab.next();
            app.list_state.select(Some(0));
            app.detail_scroll = 0;
            reload_rows(app, vault, config)?;
            app.clear_status();
            app.dirty = true;
        }
        (KeyCode::BackTab, _) => {
            app.focus = app.focus.toggle();
            app.dirty = true;
        }
        (KeyCode::Char('1'), _) => switch_tab(app, Tab::Today, vault, config)?,
        (KeyCode::Char('2'), _) => switch_tab(app, Tab::List, vault, config)?,
        (KeyCode::Char('3'), _) => switch_tab(app, Tab::Stale, vault, config)?,
        (KeyCode::Char('4'), _) | (KeyCode::Char('/'), _) => open_search(app, vault),

        (KeyCode::Char('r'), _) if plain => {
            reload_rows(app, vault, config)?;
            app.set_status("reloaded");
        }

        (KeyCode::Char('?'), _) => {
            app.mode = Mode::Help;
            app.dirty = true;
        }
        (KeyCode::Char('c'), _) if plain => match new_capture_form(vault) {
            Ok(form) => {
                app.mode = Mode::Capture(form);
                app.dirty = true;
            }
            Err(err) => app.set_status(format!("capture unavailable: {}", err)),
        },

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
                let prompt = format!("Delete {}? (y/n)", &id[..8.min(id.len())]);
                app.mode = Mode::Confirm {
                    prompt,
                    action: ConfirmAction::Delete { id },
                };
                app.dirty = true;
            }
        }
        (KeyCode::Char('p'), _) if plain => {
            if let Some(id) = app.selected_id() {
                app.mode = Mode::Input {
                    prompt: "Promote to type:".to_string(),
                    value: String::new(),
                    action: InputAction::Promote { id },
                };
                app.dirty = true;
            }
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

fn handle_search(
    app: &mut App,
    key: KeyEvent,
    mut popup: SearchPopup,
    terminal: &mut Term,
    vault: &Path,
    config: &Config,
) -> Result<bool> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char('c') if ctrl => return Ok(false),
        KeyCode::Esc => {
            app.search_input = popup.input;
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
            Focus::List => {
                popup
                    .list_state
                    .select(if popup.rows.is_empty() { None } else { Some(0) })
            }
            Focus::Detail => popup.detail_scroll = 0,
        },
        KeyCode::End => match popup.focus {
            Focus::List => {
                let last = popup.rows.len().saturating_sub(1);
                popup.list_state.select(if popup.rows.is_empty() {
                    None
                } else {
                    Some(last)
                });
            }
            Focus::Detail => popup.detail_scroll = popup.detail_max_scroll,
        },
        KeyCode::Backspace => {
            if popup.input.pop().is_some() {
                reload_search_popup(app, &mut popup, vault, true);
            }
        }
        KeyCode::Enter => {
            if let Some(id) = popup.selected_id() {
                edit_id(app, terminal, vault, config, &id)?;
                reload_search_popup(app, &mut popup, vault, false);
            }
        }
        KeyCode::Char(c) if !ctrl => {
            popup.input.push(c);
            reload_search_popup(app, &mut popup, vault, true);
        }
        _ => {}
    }

    popup.clamp_selection();
    app.search_input = popup.input.clone();
    app.mode = Mode::Search(popup);
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
            if trimmed.is_empty() {
                app.mode = Mode::Normal;
                app.set_status("cancelled");
            } else {
                match action {
                    InputAction::Promote { id } => match actions::promote(vault, &id, &trimmed) {
                        Ok(msg) => {
                            app.cache.invalidate(&id);
                            app.search.mark_stale();
                            reload_rows(app, vault, config)?;
                            app.set_status(msg);
                        }
                        Err(e) => app.set_status(format!("promote failed: {}", e)),
                    },
                }
                app.mode = Mode::Normal;
                app.dirty = true;
            }
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

fn open_search(app: &mut App, vault: &Path) {
    let mut popup = SearchPopup::new(app.search_input.clone());
    reload_search_popup(app, &mut popup, vault, true);
    app.mode = Mode::Search(popup);
    app.clear_status();
    app.dirty = true;
}

fn reload_search_popup(
    app: &mut App,
    popup: &mut SearchPopup,
    vault: &Path,
    reset_selection: bool,
) {
    let selected_id = if reset_selection {
        None
    } else {
        popup.selected_id()
    };

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
    app.tab = tab;
    app.list_state.select(Some(0));
    app.detail_scroll = 0;
    reload_rows(app, vault, config)?;
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
    if cur >= app.rows.len() {
        app.list_state.select(Some(app.rows.len() - 1));
    }
}
