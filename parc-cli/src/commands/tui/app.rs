use std::io;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use parc_core::config::Config;
use std::path::Path;

use super::{data, ui, Tab};

pub(super) fn run_loop(stdout: &mut io::Stdout, vault: &Path, config: &Config) -> Result<()> {
    let mut tab = Tab::Today;
    let mut selected = 0usize;
    let mut search_input = String::new();
    let mut rows = data::load_rows(vault, tab, &search_input, config)?;
    let mut status = String::new();
    // Repaint only when state changes. Without this, a 250ms poll-and-draw
    // loop redraws ~4×/sec while idle and the screen visibly flickers.
    let mut dirty = true;

    loop {
        if selected >= rows.len() {
            selected = rows.len().saturating_sub(1);
        }

        if dirty {
            ui::draw(
                stdout,
                vault,
                config,
                tab,
                &rows,
                selected,
                &search_input,
                &status,
            )?;
            dirty = false;
        }

        // Block until something happens. No timeout = no idle redraws.
        match event::read()? {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') if key.modifiers.is_empty() => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Tab => {
                    tab = tab.next();
                    selected = 0;
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    status.clear();
                    dirty = true;
                }
                KeyCode::Char('1') => {
                    tab = Tab::Today;
                    selected = 0;
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('2') => {
                    tab = Tab::List;
                    selected = 0;
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('3') => {
                    tab = Tab::Stale;
                    selected = 0;
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('4') | KeyCode::Char('/') => {
                    tab = Tab::Search;
                    selected = 0;
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char('r') if tab != Tab::Search => {
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    status = "reloaded".to_string();
                    dirty = true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected + 1 < rows.len() {
                        selected += 1;
                        dirty = true;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                        dirty = true;
                    }
                }
                KeyCode::Backspace if tab == Tab::Search => {
                    if search_input.pop().is_some() {
                        selected = 0;
                        rows = data::load_rows(vault, tab, &search_input, config)?;
                        dirty = true;
                    }
                }
                KeyCode::Esc if tab == Tab::Search && !search_input.is_empty() => {
                    search_input.clear();
                    selected = 0;
                    rows = data::load_rows(vault, tab, &search_input, config)?;
                    dirty = true;
                }
                KeyCode::Char(c)
                    if tab == Tab::Search && !key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    search_input.push(c);
                    selected = 0;
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
