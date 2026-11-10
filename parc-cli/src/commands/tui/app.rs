use std::io::Stdout;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use parc_core::config::Config;
use parc_core::index;
use parc_core::ops::Writer;
use parc_core::schema::load_schemas;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;
use rusqlite::Connection;

use super::cache::FragmentCache;
use super::data::VaultStamp;
use super::markdown::{ActionKind, Actionable};
use super::{
    actions, data, ui, CaptureField, CaptureForm, ConfirmAction, Focus, InputAction, Mode,
    OverlayKind, OverlayState, QuickField, Row, Tab,
};

mod launcher;

use launcher::{handle_launcher, open_launcher};

const PAGE_LINES: u16 = 10;
const HALF_PAGE_LINES: u16 = 5;
const FRAGMENT_CACHE_CAP: usize = 64;
/// How often to check whether the vault changed outside the TUI.
const WATCH_INTERVAL: Duration = Duration::from_secs(2);
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
    /// Saves fragments (hooks, history, index) the same way the CLI does.
    pub writer: Writer,
    /// Vault state as of the last row load; see `refresh_if_changed`.
    vault_stamp: VaultStamp,
}

impl App {
    fn new(rows: Vec<super::Row>, writer: Writer) -> Self {
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
            writer,
            vault_stamp: VaultStamp::default(),
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

pub(super) fn run_loop(
    terminal: &mut Term,
    vault: &Path,
    config: &Config,
    writer: Writer,
) -> Result<()> {
    let mut app = start_app(vault, config, writer)?;
    let mut next_watch = Instant::now() + WATCH_INTERVAL;

    loop {
        clamp_selection(&mut app);

        if app.dirty {
            terminal.draw(|frame| {
                ui::draw(frame, vault, config, &mut app);
            })?;
            app.dirty = false;
        }

        let now = Instant::now();
        let mut timeout = next_watch.saturating_duration_since(now);
        if let Some(deadline) = app.status.deadline() {
            timeout = timeout.min(deadline.saturating_duration_since(now));
        }

        let has_event = event::poll(timeout)?;
        if Instant::now() >= next_watch {
            refresh_if_changed(&mut app, vault, config);
            next_watch = Instant::now() + WATCH_INTERVAL;
        }
        if !has_event {
            if app.status.is_expired() {
                app.clear_status();
            }
            continue;
        }

        let event = event::read()?;
        let handled = match event {
            Event::Key(key) => match app.mode.clone() {
                Mode::Normal => handle_normal(&mut app, key, terminal, vault, config),
                Mode::Confirm { action, .. } => {
                    handle_confirm(&mut app, key, action, vault, config)
                }
                Mode::Input {
                    value,
                    action,
                    prompt,
                } => handle_input(&mut app, key, prompt, value, action, vault, config),
                Mode::Capture(form) => handle_capture(&mut app, key, form, vault, config),
                Mode::Launcher(popup) => {
                    handle_launcher(&mut app, key, popup, terminal, vault, config)
                }
                Mode::Overlay(state) => handle_overlay(&mut app, key, state, vault, config),
                Mode::Help => handle_help(&mut app, key),
            },
            Event::Resize(_, _) => {
                app.dirty = true;
                Ok(true)
            }
            _ => Ok(true),
        };
        // An error a handler didn't deal with itself is reported, not fatal:
        // one bad fragment or a locked index shouldn't end the session.
        let keep_going = handled.unwrap_or_else(|e| {
            app.mode = Mode::Normal;
            app.set_status(format!("error: {}", e));
            true
        });
        if !keep_going {
            break;
        }
    }

    Ok(())
}

/// Move the list selection or scroll the detail pane, whichever is focused.
fn move_focused(app: &mut App, delta: i32) {
    match app.focus {
        Focus::List => app.move_list(delta),
        Focus::Detail => app.scroll_detail(delta),
    }
}

/// Jump to the top or bottom of the focused pane.
fn jump_focused(app: &mut App, to_end: bool) {
    match app.focus {
        Focus::List => {
            let target = match (app.rows.len(), to_end) {
                (0, _) => None,
                (_, false) => Some(0),
                (len, true) => Some(len - 1),
            };
            app.select(target);
        }
        Focus::Detail => {
            app.detail_scroll = if to_end { app.detail_max_scroll } else { 0 };
            app.dirty = true;
        }
    }
}

/// Load the first screen and record the vault's on-disk state.
fn start_app(vault: &Path, config: &Config, writer: Writer) -> Result<App> {
    // Make sure the index exists before stamping; otherwise creating it
    // during the first load would look like an outside change.
    index::open_index(vault)?;
    let stamp = VaultStamp::read(vault);
    let rows = data::load_rows(vault, Tab::Today, config)?;
    let mut app = App::new(rows, writer);
    app.vault_stamp = stamp;
    Ok(app)
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
            app.clear_status();
            switch_tab(app, app.tab.next(), vault, config);
        }
        (KeyCode::BackTab, _) => {
            app.focus = app.focus.toggle();
            app.dirty = true;
        }
        (KeyCode::Char('1'), _) => switch_tab(app, Tab::Today, vault, config),
        (KeyCode::Char('2'), _) => switch_tab(app, Tab::List, vault, config),
        (KeyCode::Char('3'), _) => switch_tab(app, Tab::Stale, vault, config),
        (KeyCode::Char('4'), _) => switch_tab(app, Tab::Due, vault, config),
        (KeyCode::Char('5'), _) => switch_tab(app, Tab::Review, vault, config),
        (KeyCode::Char('6'), _) => switch_tab(app, Tab::Search, vault, config),
        (KeyCode::Char('/'), _) => open_launcher(app, vault, ""),
        (KeyCode::Char('p') | KeyCode::Char('P'), m) if m.contains(KeyModifiers::CONTROL) => {
            open_launcher(app, vault, ">")
        }

        (KeyCode::Char('r'), _) if plain => manual_reload(app, vault, config),

        (KeyCode::Char('?'), _) => {
            app.mode = Mode::Help;
            app.dirty = true;
        }
        (KeyCode::Char('c'), _) if plain => open_capture_form(app, vault),

        (KeyCode::Char('e'), _) if plain => {
            if let Some(id) = app.selected_id() {
                edit_id(app, terminal, vault, config, &id);
            }
        }
        (KeyCode::Char('t'), _) if plain => {
            if let Some(id) = app.selected_id() {
                toggle_status_id(app, vault, config, &id);
            }
        }
        (KeyCode::Char('a'), _) if plain => {
            if let Some(id) = app.selected_id() {
                archive_id(app, vault, config, &id);
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

        (KeyCode::Down, _) => move_focused(app, 1),
        (KeyCode::Char('j'), _) if plain => move_focused(app, 1),
        (KeyCode::Up, _) => move_focused(app, -1),
        (KeyCode::Char('k'), _) if plain => move_focused(app, -1),
        (KeyCode::PageDown, _) => move_focused(app, PAGE_LINES as i32),
        (KeyCode::PageUp, _) => move_focused(app, -(PAGE_LINES as i32)),
        (KeyCode::Char('d'), _) if ctrl => move_focused(app, HALF_PAGE_LINES as i32),
        (KeyCode::Char('u'), _) if ctrl => move_focused(app, -(HALF_PAGE_LINES as i32)),
        (KeyCode::Home, _) => jump_focused(app, false),
        (KeyCode::Char('g'), _) if plain => jump_focused(app, false),
        // Terminals differ on whether `G` arrives with SHIFT set.
        (KeyCode::End, _) | (KeyCode::Char('G'), _) => jump_focused(app, true),

        _ => {}
    }

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
                ConfirmAction::Delete { id } => {
                    let result = actions::delete(&mut app.writer, &id);
                    finish_write(app, vault, config, &id, "delete", result);
                }
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
                        let result = actions::promote(&mut app.writer, &id, &trimmed);
                        finish_write(app, vault, config, &id, "promote", result);
                    }
                }
                InputAction::SetField { id, field } => {
                    let result = actions::set_field(&mut app.writer, &id, field, &trimmed);
                    let what = format!("{} update", field.key());
                    finish_write(app, vault, config, &id, &what, result);
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
    // Only a failed save keeps the form open; once the fragment exists, a
    // later reload problem must not invite a duplicate submit.
    let (id, msg) = actions::capture(&mut app.writer, input)?;
    app.mode = Mode::Normal;
    finish_write(app, vault, config, &id, "capture", Ok(msg));
    Ok(())
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

fn edit_id(app: &mut App, terminal: &mut Term, vault: &Path, config: &Config, id: &str) {
    let result = actions::edit(terminal, &mut app.writer, id);
    finish_write(app, vault, config, id, "edit", result);
}

fn toggle_status_id(app: &mut App, vault: &Path, config: &Config, id: &str) {
    let result = actions::toggle_status(&mut app.writer, id);
    finish_write(app, vault, config, id, "toggle", result);
}

fn archive_id(app: &mut App, vault: &Path, config: &Config, id: &str) {
    let result = actions::archive(&mut app.writer, id);
    finish_write(app, vault, config, id, "archive", result);
}

/// Finish any action that saved a fragment. On success: drop cached copies,
/// reload the list, keep `id` selected if it's still listed, and show `msg`.
/// On failure: show "<what> failed: <error>". Never propagates an error.
fn finish_write(
    app: &mut App,
    vault: &Path,
    config: &Config,
    id: &str,
    what: &str,
    result: Result<String>,
) {
    let msg = match result {
        Ok(msg) => msg,
        Err(e) => {
            app.set_status(format!("{} failed: {}", what, e));
            return;
        }
    };
    app.cache.invalidate(id);
    app.search.mark_stale();
    match reload_rows(app, vault, config) {
        Ok(()) => {
            select_row_by_id(app, id);
            app.set_status(msg);
        }
        Err(e) => app.set_status(format!("{}, but reload failed: {}", msg, e)),
    }
}

/// Pick up changes made outside the TUI (CLI, agents, `git pull`, editors).
///
/// Only runs in Normal and Help mode. Under the link/checkbox overlay a
/// refresh could shift the labelled items under the user's cursor, and
/// popups have their own state; the change is picked up once they close.
fn refresh_if_changed(app: &mut App, vault: &Path, config: &Config) {
    if !matches!(app.mode, Mode::Normal | Mode::Help) {
        return;
    }
    // The new state is recorded even if the load below fails, so a broken
    // vault doesn't re-trigger (and re-report) every tick.
    if !note_disk_state(app, vault) {
        return;
    }
    let selected = app.selected_id();
    match load_current_rows(app, vault, config) {
        Ok(()) => {
            if let Some(id) = selected {
                select_row_by_id(app, &id);
            }
            if matches!(app.status, Status::Idle) {
                app.set_status("updated from disk");
            }
        }
        Err(e) => app.set_status(format!("reload failed: {}", e)),
    }
    app.dirty = true;
}

/// `r`: re-read everything from disk, including fragments edited elsewhere.
fn manual_reload(app: &mut App, vault: &Path, config: &Config) {
    app.cache.clear();
    app.search.mark_stale();
    match reload_rows(app, vault, config) {
        Ok(()) => app.set_status("reloaded"),
        Err(e) => app.set_status(format!("reload failed: {}", e)),
    }
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
            let result = actions::toggle_checkbox(&mut app.writer, &id, item.source_range.clone());
            finish_write(app, vault, config, &id, "toggle", result);
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
        switch_tab(app, Tab::List, vault, config);
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

fn switch_tab(app: &mut App, tab: Tab, vault: &Path, config: &Config) {
    if app.tab == tab {
        return;
    }

    app.save_tab_state();
    app.tab = tab;
    if let Err(e) = reload_rows(app, vault, config) {
        // Don't leave the previous tab's rows under the new tab's heading.
        app.rows.clear();
        app.set_status(format!("could not load tab: {}", e));
    }
    app.restore_tab_state();
    app.dirty = true;
}

fn reload_rows(app: &mut App, vault: &Path, config: &Config) -> Result<()> {
    note_disk_state(app, vault);
    load_current_rows(app, vault, config)
}

/// Record the vault's on-disk state. If it moved since the last load,
/// anything cached from disk may be stale, so drop it; returns whether it
/// moved. Called before loading, so a change that lands mid-load is caught
/// on the next watch tick rather than missed.
fn note_disk_state(app: &mut App, vault: &Path) -> bool {
    let stamp = VaultStamp::read(vault);
    if stamp == app.vault_stamp {
        return false;
    }
    app.vault_stamp = stamp;
    app.cache.clear();
    app.search.mark_stale();
    app.index_db = None;
    app.index_open_failed = false;
    true
}

fn load_current_rows(app: &mut App, vault: &Path, config: &Config) -> Result<()> {
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
    use parc_core::vault::init_vault;

    pub(super) fn test_vault() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        init_vault(&vault).unwrap();
        (tmp, vault)
    }

    pub(super) fn row(fragment_type: &str, title: &str, assignee: Option<&str>) -> Row {
        Row {
            id: format!("{}-id", title),
            title: title.to_string(),
            fragment_type: fragment_type.to_string(),
            status: None,
            priority: None,
            due: None,
            assignee: assignee.map(str::to_string),
            tags: Vec::new(),
            updated_at: "2026-05-03T00:00:00Z".to_string(),
            section: None,
            title_match_indices: Vec::new(),
            score: 0,
        }
    }

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

    fn test_app(vault: &Path) -> App {
        let writer = crate::hooks::writer(vault, parc_core::hook::HookStderr::Capture);
        App::new(Vec::new(), writer)
    }

    fn status_text(app: &App) -> &str {
        match &app.status {
            Status::Active { text, .. } => text,
            Status::Idle => "",
        }
    }

    /// Make every index read fail, as a corrupted or foreign index.db would.
    fn break_index(vault: &Path) {
        std::fs::write(vault.join("index.db"), b"definitely not sqlite").unwrap();
    }

    #[test]
    fn finish_write_reports_failed_action() {
        let (_tmp, vault) = test_vault();
        let config = parc_core::config::load_config(&vault).unwrap();
        let mut app = test_app(&vault);

        finish_write(
            &mut app,
            &vault,
            &config,
            "ID",
            "edit",
            Err(anyhow::anyhow!("boom")),
        );

        assert_eq!(status_text(&app), "edit failed: boom");
    }

    #[test]
    fn finish_write_survives_failed_reload() {
        let (_tmp, vault) = test_vault();
        let config = parc_core::config::load_config(&vault).unwrap();
        let mut app = test_app(&vault);
        break_index(&vault);

        finish_write(
            &mut app,
            &vault,
            &config,
            "ID",
            "edit",
            Ok("edited ID".into()),
        );

        assert!(
            status_text(&app).starts_with("edited ID, but reload failed:"),
            "{}",
            status_text(&app)
        );
    }

    #[test]
    fn switch_tab_survives_failed_load() {
        let (_tmp, vault) = test_vault();
        let config = parc_core::config::load_config(&vault).unwrap();
        let mut app = test_app(&vault);
        app.rows = vec![row("note", "From Today", None)];
        break_index(&vault);

        switch_tab(&mut app, Tab::List, &vault, &config);

        assert_eq!(app.tab, Tab::List);
        assert!(app.rows.is_empty(), "old tab's rows must not linger");
        assert!(status_text(&app).starts_with("could not load tab:"));
    }

    #[test]
    fn manual_reload_reports_failure() {
        let (_tmp, vault) = test_vault();
        let config = parc_core::config::load_config(&vault).unwrap();
        let mut app = test_app(&vault);
        break_index(&vault);

        manual_reload(&mut app, &vault, &config);

        assert!(status_text(&app).starts_with("reload failed:"));
    }

    fn create_note(vault: &Path, title: &str) -> String {
        let config = parc_core::config::load_config(vault).unwrap();
        let schemas = load_schemas(vault).unwrap();
        let frag = parc_core::fragment::new_fragment(
            "note",
            title,
            schemas.resolve("note").unwrap(),
            &config,
        );
        let mut writer = crate::hooks::writer(vault, parc_core::hook::HookStderr::Capture);
        writer.create(frag).unwrap().id
    }

    #[test]
    fn vault_stamp_tracks_creates_edits_and_deletes() {
        let (_tmp, vault) = test_vault();
        let empty = VaultStamp::read(&vault);
        assert_eq!(
            empty,
            VaultStamp::read(&vault),
            "stable when nothing changes"
        );

        let id = create_note(&vault, "One");
        let created = VaultStamp::read(&vault);
        assert_ne!(created, empty);

        // In-place edit: same file count, newer mtime. Set the mtime
        // explicitly so coarse filesystem timestamps can't hide it.
        let path = vault.join("fragments").join(format!("{}.md", id));
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() + Duration::from_secs(60))
            .unwrap();
        assert_ne!(VaultStamp::read(&vault), created);

        std::fs::remove_file(&path).unwrap();
        assert_eq!(VaultStamp::read(&vault).fragment_count_for_test(), 0);
    }

    #[test]
    fn refresh_picks_up_external_changes_once() {
        let (_tmp, vault) = test_vault();
        let config = parc_core::config::load_config(&vault).unwrap();
        let writer = crate::hooks::writer(&vault, parc_core::hook::HookStderr::Capture);
        let mut app = start_app(&vault, &config, writer).unwrap();
        switch_tab(&mut app, Tab::List, &vault, &config);
        assert!(app.rows.is_empty());

        // Nothing changed: no reload, no status noise.
        refresh_if_changed(&mut app, &vault, &config);
        assert_eq!(status_text(&app), "");

        // Another process (CLI, agent) adds a fragment.
        create_note(&vault, "From outside");
        refresh_if_changed(&mut app, &vault, &config);
        assert_eq!(app.rows.len(), 1);
        assert_eq!(app.rows[0].title, "From outside");
        assert_eq!(status_text(&app), "updated from disk");

        // Already seen: a second tick is a no-op.
        app.clear_status();
        refresh_if_changed(&mut app, &vault, &config);
        assert_eq!(status_text(&app), "");
    }

    #[test]
    fn own_writes_do_not_trigger_a_second_refresh() {
        let (_tmp, vault) = test_vault();
        let config = parc_core::config::load_config(&vault).unwrap();
        let writer = crate::hooks::writer(&vault, parc_core::hook::HookStderr::Capture);
        let mut app = start_app(&vault, &config, writer).unwrap();
        switch_tab(&mut app, Tab::List, &vault, &config);
        let id = create_note(&vault, "Mine");
        reload_rows(&mut app, &vault, &config).unwrap();

        let result = actions::archive(&mut app.writer, &id);
        finish_write(&mut app, &vault, &config, &id, "archive", result);
        let status = status_text(&app).to_string();

        refresh_if_changed(&mut app, &vault, &config);
        assert_eq!(status_text(&app), status, "the write's own message stays");
    }

    #[test]
    fn refresh_waits_while_a_popup_is_open() {
        let (_tmp, vault) = test_vault();
        let config = parc_core::config::load_config(&vault).unwrap();
        let writer = crate::hooks::writer(&vault, parc_core::hook::HookStderr::Capture);
        let mut app = start_app(&vault, &config, writer).unwrap();
        switch_tab(&mut app, Tab::List, &vault, &config);
        reload_rows(&mut app, &vault, &config).unwrap();
        app.mode = Mode::Help;
        refresh_if_changed(&mut app, &vault, &config);

        app.mode = Mode::Capture(new_capture_form(&vault).unwrap());
        create_note(&vault, "Later");
        refresh_if_changed(&mut app, &vault, &config);
        assert!(app.rows.is_empty(), "no refresh under the capture form");

        app.mode = Mode::Normal;
        refresh_if_changed(&mut app, &vault, &config);
        assert_eq!(app.rows.len(), 1);
    }

    #[test]
    fn jump_and_move_follow_the_focused_pane() {
        let (_tmp, vault) = test_vault();
        let mut app = test_app(&vault);
        app.rows = vec![
            row("note", "a", None),
            row("note", "b", None),
            row("note", "c", None),
        ];
        app.select(Some(0));

        jump_focused(&mut app, true);
        assert_eq!(app.list_state.selected(), Some(2));
        move_focused(&mut app, -1);
        assert_eq!(app.list_state.selected(), Some(1));
        jump_focused(&mut app, false);
        assert_eq!(app.list_state.selected(), Some(0));

        app.focus = Focus::Detail;
        app.detail_max_scroll = 40;
        jump_focused(&mut app, true);
        assert_eq!(app.detail_scroll, 40);
        assert_eq!(app.list_state.selected(), Some(0), "list untouched");
        jump_focused(&mut app, false);
        assert_eq!(app.detail_scroll, 0);

        app.focus = Focus::List;
        app.rows.clear();
        jump_focused(&mut app, true);
        assert_eq!(app.list_state.selected(), None);
    }
}
