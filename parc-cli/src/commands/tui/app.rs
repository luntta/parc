use std::io::Stdout;
use std::path::Path;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use parc_core::config::Config;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;

use super::{data, ui, Tab};

pub(super) fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    vault: &Path,
    config: &Config,
) -> Result<()> {
    let mut tab = Tab::Today;
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut search_input = String::new();
    let mut rows = data::load_rows(vault, tab, &search_input, config)?;
    let mut status = String::new();
    let mut dirty = true;

    loop {
        clamp_selection(&mut list_state, rows.len());

        if dirty {
            terminal.draw(|frame| {
                ui::draw(
                    frame,
                    vault,
                    config,
                    tab,
                    &rows,
                    &mut list_state,
                    &search_input,
                    &status,
                );
            })?;
            dirty = false;
        }

        match event::read()? {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') if key.modifiers.is_empty() => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Tab => {
                    tab = tab.next();
                    list_state.select(Some(0));
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    status.clear();
                    dirty = true;
                }
                KeyCode::Char('1') => {
                    tab = Tab::Today;
                    list_state.select(Some(0));
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('2') => {
                    tab = Tab::List;
                    list_state.select(Some(0));
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('3') => {
                    tab = Tab::Stale;
                    list_state.select(Some(0));
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('4') | KeyCode::Char('/') => {
                    tab = Tab::Search;
                    list_state.select(Some(0));
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('r') if tab != Tab::Search => {
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    status = "reloaded".to_string();
                    dirty = true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = rows.len();
                    let cur = list_state.selected().unwrap_or(0);
                    if cur + 1 < len {
                        list_state.select(Some(cur + 1));
                        dirty = true;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let cur = list_state.selected().unwrap_or(0);
                    if cur > 0 {
                        list_state.select(Some(cur - 1));
                        dirty = true;
                    }
                }
                KeyCode::Backspace if tab == Tab::Search => {
                    if search_input.pop().is_some() {
                        list_state.select(Some(0));
                        rows = data::load_rows(vault, tab, &search_input, config)?;
                        dirty = true;
                    }
                }
                KeyCode::Esc if tab == Tab::Search && !search_input.is_empty() => {
                    search_input.clear();
                    list_state.select(Some(0));
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char(c)
                    if tab == Tab::Search && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    search_input.push(c);
                    list_state.select(Some(0));
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                _ => {}
            },
            Event::Resize(_, _) => {
                dirty = true;
            }
            _ => {}
        }
    }

    Ok(())
}

fn clamp_selection(list_state: &mut ListState, len: usize) {
    if len == 0 {
        list_state.select(None);
        return;
    }
    let cur = list_state.selected().unwrap_or(0);
    if cur >= len {
        list_state.select(Some(len - 1));
    }
}
