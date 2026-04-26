use std::io::Stdout;
use std::path::Path;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use parc_core::config::Config;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;

use super::{data, ui, Focus, Tab};

const PAGE_LINES: u16 = 10;
const HALF_PAGE_LINES: u16 = 5;

pub(super) struct App {
    pub tab: Tab,
    pub focus: Focus,
    pub list_state: ListState,
    pub detail_scroll: u16,
    pub detail_max_scroll: u16,
    pub search_input: String,
    pub rows: Vec<super::Row>,
    pub status: String,
    pub dirty: bool,
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
            dirty: true,
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
}

pub(super) fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    vault: &Path,
    config: &Config,
) -> Result<()> {
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

        let event = event::read()?;
        match event {
            Event::Key(key) => {
                if !handle_key(&mut app, key, vault, config)? {
                    break;
                }
            }
            Event::Resize(_, _) => {
                app.dirty = true;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Returns `false` if the loop should exit.
fn handle_key(app: &mut App, key: KeyEvent, vault: &Path, config: &Config) -> Result<bool> {
    let plain = key.modifiers.is_empty();
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), m) if m.is_empty() => return Ok(false),
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
            app.status = "reloaded".to_string();
            app.dirty = true;
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
                app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
                app.dirty = true;
            }
        }
        (KeyCode::Esc, _) if app.tab == Tab::Search && !app.search_input.is_empty() => {
            app.search_input.clear();
            app.list_state.select(Some(0));
            app.detail_scroll = 0;
            app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
            app.dirty = true;
        }
        (KeyCode::Char(c), _) if app.tab == Tab::Search && !ctrl => {
            app.search_input.push(c);
            app.list_state.select(Some(0));
            app.detail_scroll = 0;
            app.rows = data::load_rows(vault, app.tab, &app.search_input, config)?;
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
