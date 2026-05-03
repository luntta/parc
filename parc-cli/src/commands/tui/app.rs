use std::io::Stdout;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use parc_core::config::Config;
use parc_core::index;
use parc_core::schema::load_schemas;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;
use rusqlite::Connection;

use super::cache::FragmentCache;
use super::command::{self, CommandAction, LauncherKind};
use super::markdown::{ActionKind, Actionable};
use super::{
    actions, data, ui, CaptureField, CaptureForm, ConfirmAction, Focus, InputAction, IntentAction,
    LauncherIntent, LauncherItem, LauncherPopup, Mode, OverlayKind, OverlayState, QuickField, Row,
    Tab,
};

const PAGE_LINES: u16 = 10;
const HALF_PAGE_LINES: u16 = 5;
const FRAGMENT_CACHE_CAP: usize = 64;
const IDLE_POLL_SECS: u64 = 60;
const STATUS_LIFETIME_SECS: u64 = 4;
const TAB_COUNT: usize = Tab::ALL.len();

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

/// Browser-style back/forward stack for link-follow navigation.
/// Only `follow_link_action` pushes onto it — arrow-key list movement does
/// NOT, so Ctrl-o feels like "go back to where you came from after following
/// a link", not "undo my browsing".
#[derive(Default, Clone)]
pub(super) struct NavHistory {
    back: Vec<String>,
    forward: Vec<String>,
}

impl NavHistory {
    /// Record `current` (where we were) before navigating elsewhere.
    /// Clears the forward stack — a new branch invalidates redo.
    pub(super) fn push(&mut self, current: String) {
        self.back.push(current);
        self.forward.clear();
    }

    /// Pop the previous fragment id; record `current` so a forward step
    /// can return here.
    pub(super) fn back(&mut self, current: String) -> Option<String> {
        let prev = self.back.pop()?;
        self.forward.push(current);
        Some(prev)
    }

    /// Pop the next fragment id; record `current` so a back step can
    /// return here.
    pub(super) fn forward(&mut self, current: String) -> Option<String> {
        let next = self.forward.pop()?;
        self.back.push(current);
        Some(next)
    }
}

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
    pub search_view_query: Option<String>,
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
    /// Back/forward stack populated only by link-follow; arrow-key list
    /// movement does not record a step.
    pub nav_history: NavHistory,
    /// Lazily opened SQLite index handle for backlink lookups. Cached for
    /// the lifetime of the App so backlinks queries don't reopen per render.
    index_db: Option<Connection>,
    /// Set once if opening the index fails, to avoid logging a status
    /// message on every redraw.
    index_open_failed: bool,
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
            search_view_query: None,
            cache: FragmentCache::new(FRAGMENT_CACHE_CAP),
            detail_items: Vec::new(),
            detail_body_offset: 0,
            detail_viewport: 0,
            nav_history: NavHistory::default(),
            index_db: None,
            index_open_failed: false,
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

    /// Lazily open the SQLite index. Returns `None` (silently) if the
    /// index can't be opened — backlinks are then omitted from the detail
    /// pane rather than crashing or spamming status messages.
    pub(super) fn ensure_index(&mut self, vault: &Path) -> Option<&Connection> {
        if self.index_db.is_some() {
            return self.index_db.as_ref();
        }
        if self.index_open_failed {
            return None;
        }
        match index::open_index(vault) {
            Ok(conn) => {
                self.index_db = Some(conn);
                self.index_db.as_ref()
            }
            Err(_) => {
                self.index_open_failed = true;
                None
            }
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
                Mode::Launcher(popup) => {
                    handle_launcher(&mut app, key, popup, terminal, vault, config)?
                }
                Mode::Overlay(state) => handle_overlay(&mut app, key, state, vault, config)?,
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
        (KeyCode::Char('4'), _) => switch_tab(app, Tab::Due, vault, config)?,
        (KeyCode::Char('5'), _) => switch_tab(app, Tab::Review, vault, config)?,
        (KeyCode::Char('6'), _) => switch_tab(app, Tab::Search, vault, config)?,
        (KeyCode::Char('/'), _) => open_launcher(app, vault, ""),
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
            open_overlay(app, OverlayKind::FollowLink, vault, config);
        }
        (KeyCode::Char('x'), _) if plain && app.focus == Focus::Detail => {
            open_overlay(app, OverlayKind::ToggleCheckbox, vault, config);
        }
        (KeyCode::Char('o'), _) if ctrl => nav_back(app, vault, config)?,
        (KeyCode::Char('i'), _) if ctrl => nav_forward(app, vault, config)?,

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
                execute_intent_action(app, intent.action, vault, config)?;
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
            let intents = launcher_intents(&popup.input, app.selected_id().is_some());
            popup.items =
                ranked_universal_items(&popup.input, &intents, &popup.commands, &popup.rows);
            popup.error = None;
            restore_launcher_selection(popup, selected_item);
        }
        Err(err) => {
            popup.rows.clear();
            popup.commands = command::matching_commands(&popup.input, app.selected_id().is_some());
            let intents = launcher_intents(&popup.input, app.selected_id().is_some());
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

fn launcher_intents(input: &str, has_selection: bool) -> Vec<LauncherIntent> {
    if !has_selection {
        return Vec::new();
    }

    let query = command::command_query(input).trim();
    let Some((field, value)) = parse_field_intent(query) else {
        return Vec::new();
    };

    vec![LauncherIntent {
        label: format!("Set {} to {}", field.label(), value),
        description: format!(
            "Update the selected fragment's {} field to `{}`.",
            field.key(),
            value
        ),
        action: IntentAction::SetField { field, value },
    }]
}

fn parse_field_intent(input: &str) -> Option<(QuickField, String)> {
    let input = input.trim();
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
            let value = value.trim();
            if !value.is_empty() {
                return Some((field, value.to_string()));
            }
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

            match actions::set_field(vault, &id, field, &value) {
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

fn open_overlay(app: &mut App, kind: OverlayKind, vault: &Path, config: &Config) {
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
        invoke_actionable(app, kind, visible[0], vault, config);
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

fn handle_overlay(
    app: &mut App,
    key: KeyEvent,
    state: OverlayState,
    vault: &Path,
    config: &Config,
) -> Result<bool> {
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
                    invoke_actionable(app, state.kind, idx, vault, config);
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

fn invoke_actionable(
    app: &mut App,
    kind: OverlayKind,
    item_idx: usize,
    vault: &Path,
    config: &Config,
) {
    let Some(item) = app.detail_items.get(item_idx).cloned() else {
        return;
    };
    let Some(id) = app.selected_id() else {
        return;
    };
    match (kind, &item.kind) {
        (OverlayKind::ToggleCheckbox, ActionKind::Checkbox { .. }) => {
            match actions::toggle_checkbox(vault, &id, item.source_range.clone()) {
                Ok(msg) => {
                    app.cache.invalidate(&id);
                    app.search.mark_stale();
                    if let Err(e) = reload_rows(app, vault, config) {
                        app.set_status(format!("toggle ok but reload failed: {}", e));
                        return;
                    }
                    app.set_status(msg);
                }
                Err(e) => app.set_status(format!("toggle failed: {}", e)),
            }
        }
        (OverlayKind::FollowLink, ActionKind::WikiLink { target, .. }) => {
            follow_link_action(app, vault, config, target);
        }
        _ => {}
    }
}

/// Resolve `target` to a fragment id, push the current selection onto the
/// nav stack, then jump. If the target isn't visible in the current tab,
/// switch to `Tab::List` (which shows recent fragments) and try again.
fn follow_link_action(app: &mut App, vault: &Path, config: &Config, target: &str) {
    let resolved = match actions::follow_link(vault, target) {
        Ok(id) => id,
        Err(e) => {
            app.set_status(format!("follow failed: {}", e));
            return;
        }
    };
    let from = app.selected_id();
    if jump_to_id(app, vault, config, &resolved).is_err() {
        app.set_status(format!(
            "follow failed: could not show {}",
            short_id(&resolved)
        ));
        return;
    }
    if let Some(from) = from {
        if from != resolved {
            app.nav_history.push(from);
        }
    }
    app.set_status(format!("→ {}", short_id(&resolved)));
}

fn nav_back(app: &mut App, vault: &Path, config: &Config) -> Result<()> {
    let Some(current) = app.selected_id() else {
        return Ok(());
    };
    let Some(prev) = app.nav_history.back(current.clone()) else {
        app.set_status("no history");
        return Ok(());
    };
    if jump_to_id(app, vault, config, &prev).is_err() {
        // Restore the stack — nothing happened.
        let _ = app.nav_history.forward(current);
        app.set_status(format!("could not show {}", short_id(&prev)));
        return Ok(());
    }
    app.set_status(format!("← {}", short_id(&prev)));
    Ok(())
}

fn nav_forward(app: &mut App, vault: &Path, config: &Config) -> Result<()> {
    let Some(current) = app.selected_id() else {
        return Ok(());
    };
    let Some(next) = app.nav_history.forward(current.clone()) else {
        app.set_status("no forward history");
        return Ok(());
    };
    if jump_to_id(app, vault, config, &next).is_err() {
        let _ = app.nav_history.back(current);
        app.set_status(format!("could not show {}", short_id(&next)));
        return Ok(());
    }
    app.set_status(format!("→ {}", short_id(&next)));
    Ok(())
}

/// Try to select `id` in the current tab. If absent, switch to `Tab::List`
/// (recent-fragments view) and try again. Returns Err if still not found.
fn jump_to_id(app: &mut App, vault: &Path, config: &Config, id: &str) -> Result<()> {
    if app.rows.iter().any(|row| row.id == id) {
        select_row_by_id(app, id);
        app.detail_scroll = 0;
        app.dirty = true;
        return Ok(());
    }
    if app.tab != Tab::List {
        switch_tab(app, Tab::List, vault, config)?;
    }
    if app.rows.iter().any(|row| row.id == id) {
        select_row_by_id(app, id);
        app.detail_scroll = 0;
        app.dirty = true;
        Ok(())
    } else {
        Err(anyhow::anyhow!("not in current view"))
    }
}

fn short_id(id: &str) -> &str {
    &id[..8.min(id.len())]
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
    app.rows = if app.tab == Tab::Search {
        match app.search_view_query.clone() {
            Some(query) if !query.trim().is_empty() => {
                data::load_search_rows(vault, &query, &mut app.search)?
            }
            _ => Vec::new(),
        }
    } else {
        data::load_rows(vault, app.tab, config)?
    };
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
        let visible = visible_actionables(
            &items,
            /*body_offset=*/ 5,
            /*scroll=*/ 5,
            /*viewport=*/ 10,
            OverlayKind::FollowLink,
        );
        // line indices: 0+5=5 (in), 2+5=7 (in but checkbox), 4+5=9 (in), 20+5=25 (out).
        assert_eq!(visible, vec![0, 2]);
    }

    #[test]
    fn visible_filter_caps_at_alphabet_size() {
        let items: Vec<Actionable> = (0..40).map(link).collect();
        let visible = visible_actionables(&items, 0, 0, 100, OverlayKind::FollowLink);
        assert_eq!(visible.len(), LABEL_ALPHABET.len());
    }

    #[test]
    fn visible_filter_returns_empty_when_scrolled_past() {
        let items = vec![link(0), link(1)];
        let visible = visible_actionables(&items, 0, 50, 10, OverlayKind::FollowLink);
        assert!(visible.is_empty());
    }

    #[test]
    fn nav_history_tracks_back_and_forward() {
        let mut nav = NavHistory::default();
        // Start at A, follow → B, follow → C.
        nav.push("A".into());
        nav.push("B".into());
        // Currently at C.
        assert_eq!(nav.back("C".into()), Some("B".into()));
        // Currently at B.
        assert_eq!(nav.back("B".into()), Some("A".into()));
        // Currently at A; nothing further back.
        assert_eq!(nav.back("A".into()), None);
        // Forward to B.
        assert_eq!(nav.forward("A".into()), Some("B".into()));
        // Forward to C.
        assert_eq!(nav.forward("B".into()), Some("C".into()));
        assert_eq!(nav.forward("C".into()), None);
    }

    #[test]
    fn nav_history_push_clears_forward() {
        let mut nav = NavHistory::default();
        nav.push("A".into());
        nav.push("B".into());
        // Back twice, then a fresh push should drop forward stack.
        assert_eq!(nav.back("C".into()), Some("B".into()));
        assert_eq!(nav.back("B".into()), Some("A".into()));
        nav.push("A".into()); // user followed a link from A
        assert_eq!(nav.forward("X".into()), None); // forward stack gone
    }

    #[test]
    fn launcher_intents_parse_selected_field_updates() {
        let intents = launcher_intents("status done", true);
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].label, "Set Status to done");
        assert_eq!(
            intents[0].action,
            IntentAction::SetField {
                field: QuickField::Status,
                value: "done".into()
            }
        );

        let intents = launcher_intents("due next friday", true);
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
        assert!(launcher_intents("status done", false).is_empty());
        assert!(launcher_intents("status", true).is_empty());
        assert!(launcher_intents("tui", true).is_empty());
    }
}
