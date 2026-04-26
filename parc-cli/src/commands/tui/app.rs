use std::io::Stdout;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use parc_core::config::Config;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;

use super::cache::FragmentCache;
use super::{actions, data, ui, ConfirmAction, Focus, InputAction, Mode, Tab};

const PAGE_LINES: u16 = 10;
const HALF_PAGE_LINES: u16 = 5;
const SEARCH_DEBOUNCE_MS: u64 = 120;
const FRAGMENT_CACHE_CAP: usize = 64;
const IDLE_POLL_SECS: u64 = 60;

type Term = Terminal<CrosstermBackend<Stdout>>;

pub(super) struct App {
    pub tab: Tab,
    pub focus: Focus,
    pub list_state: ListState,
    pub detail_scroll: u16,
    pub detail_max_scroll: u16,
    pub search_input: String,
    pub rows: Vec<super::Row>,
    pub status: String,
    pub mode: Mode,
    pub dirty: bool,
    pub pending_search_load: Option<Instant>,
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
            status: String::new(),
            mode: Mode::Normal,
            dirty: true,
            pending_search_load: None,
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
        self.status = msg.into();
        self.dirty = true;
    }
}

pub(super) fn run_loop(terminal: &mut Term, vault: &Path, config: &Config) -> Result<()> {
    let initial_rows = data::load_rows(vault, Tab::Today, "", config)?;
    let mut app = App::new(initial_rows);

    loop {
        clamp_selection(&mut app);

        if app.dirty {
            terminal.draw(|frame| {
                ui::draw(frame, vault, config, &mut app);
            })?;
            app.dirty = false;
        }

        let timeout = match app.pending_search_load {
            Some(deadline) => deadline.saturating_duration_since(Instant::now()),
            None => Duration::from_secs(IDLE_POLL_SECS),
        };

        if !event::poll(timeout)? {
            // Timeout fired with no event — flush pending debounced search.
            if app.pending_search_load.take().is_some() {
                app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
                app.dirty = true;
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
        (KeyCode::Char('q'), m) if m.is_empty() && app.tab != Tab::Search => return Ok(false),
        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => return Ok(false),

        (KeyCode::Tab, _) => {
            app.tab = app.tab.next();
            app.list_state.select(Some(0));
            app.detail_scroll = 0;
            app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
            app.status.clear();
            app.dirty = true;
        }
        (KeyCode::BackTab, _) => {
            app.focus = app.focus.toggle();
            app.dirty = true;
        }
        (KeyCode::Char('1'), _) => switch_tab(app, Tab::Today, vault, config)?,
        (KeyCode::Char('2'), _) => switch_tab(app, Tab::List, vault, config)?,
        (KeyCode::Char('3'), _) => switch_tab(app, Tab::Stale, vault, config)?,
        (KeyCode::Char('4'), _) | (KeyCode::Char('/'), _) => {
            switch_tab(app, Tab::Search, vault, config)?
        }

        (KeyCode::Char('r'), _) if app.tab != Tab::Search && plain => {
            app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
            app.set_status("reloaded");
        }

        (KeyCode::Char('?'), _) if app.tab != Tab::Search => {
            app.mode = Mode::Help;
            app.dirty = true;
        }

        (KeyCode::Char('e'), _) if plain && app.tab != Tab::Search => {
            if let Some(id) = app.selected_id() {
                match actions::edit(terminal, vault, &id) {
                    Ok(msg) => {
                        app.cache.invalidate(&id);
                        app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
                        app.set_status(msg);
                    }
                    Err(e) => app.set_status(format!("edit failed: {}", e)),
                }
            }
        }
        (KeyCode::Char('t'), _) if plain && app.tab != Tab::Search => {
            if let Some(id) = app.selected_id() {
                match actions::toggle_status(vault, &id) {
                    Ok(msg) => {
                        app.cache.invalidate(&id);
                        app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
                        app.set_status(msg);
                    }
                    Err(e) => app.set_status(format!("toggle failed: {}", e)),
                }
            }
        }
        (KeyCode::Char('a'), _) if plain && app.tab != Tab::Search => {
            if let Some(id) = app.selected_id() {
                match actions::archive(vault, &id) {
                    Ok(msg) => {
                        app.cache.invalidate(&id);
                        app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
                        app.set_status(msg);
                    }
                    Err(e) => app.set_status(format!("archive failed: {}", e)),
                }
            }
        }
        (KeyCode::Char('y'), _) if plain && app.tab != Tab::Search => {
            if let Some(id) = app.selected_id() {
                app.set_status(format!("yanked id: {}", id));
            }
        }
        (KeyCode::Char('d'), _) if plain && app.tab != Tab::Search => {
            if let Some(id) = app.selected_id() {
                let prompt = format!("Delete {}? (y/n)", &id[..8.min(id.len())]);
                app.mode = Mode::Confirm {
                    prompt,
                    action: ConfirmAction::Delete { id },
                };
                app.dirty = true;
            }
        }
        (KeyCode::Char('p'), _) if plain && app.tab != Tab::Search => {
            if let Some(id) = app.selected_id() {
                app.mode = Mode::Input {
                    prompt: "Promote to type:".to_string(),
                    value: String::new(),
                    action: InputAction::Promote { id },
                };
                app.dirty = true;
            }
        }

        (KeyCode::Down, _) | (KeyCode::Char('j'), _) if !ctrl => match app.focus {
            Focus::List => app.move_list(1),
            Focus::Detail => app.scroll_detail(1),
        },
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) if !ctrl => match app.focus {
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
        (KeyCode::Char('g'), _) if plain && app.tab != Tab::Search => match app.focus {
            Focus::List => app.select(if app.rows.is_empty() { None } else { Some(0) }),
            Focus::Detail => {
                app.detail_scroll = 0;
                app.dirty = true;
            }
        },
        (KeyCode::Char('G'), _) if app.tab != Tab::Search => match app.focus {
            Focus::List => {
                let last = app.rows.len().saturating_sub(1);
                app.select(if app.rows.is_empty() { None } else { Some(last) });
            }
            Focus::Detail => {
                app.detail_scroll = app.detail_max_scroll;
                app.dirty = true;
            }
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
                app.select(if app.rows.is_empty() { None } else { Some(last) });
            }
            Focus::Detail => {
                app.detail_scroll = app.detail_max_scroll;
                app.dirty = true;
            }
        },

        (KeyCode::Backspace, _) if app.tab == Tab::Search => {
            if app.search_input.pop().is_some() {
                app.list_state.select(Some(0));
                app.detail_scroll = 0;
                app.pending_search_load =
                    Some(Instant::now() + Duration::from_millis(SEARCH_DEBOUNCE_MS));
                app.dirty = true;
            }
        }
        (KeyCode::Esc, _) if app.tab == Tab::Search && !app.search_input.is_empty() => {
            app.search_input.clear();
            app.list_state.select(Some(0));
            app.detail_scroll = 0;
            app.pending_search_load = None;
            app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
            app.dirty = true;
        }
        (KeyCode::Char(c), _) if app.tab == Tab::Search && !ctrl => {
            app.search_input.push(c);
            app.list_state.select(Some(0));
            app.detail_scroll = 0;
            app.pending_search_load =
                Some(Instant::now() + Duration::from_millis(SEARCH_DEBOUNCE_MS));
            app.dirty = true;
        }
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
                ConfirmAction::Delete { id } => match actions::delete(vault, &id) {
                    Ok(msg) => {
                        app.cache.invalidate(&id);
                        app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
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
                            app.rows =
                                data::load_rows(vault, app.tab, &app.search_input, config)?;
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
    app.pending_search_load = None;
    app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
    app.dirty = true;
    Ok(())
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
