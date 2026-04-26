use std::io;
use std::path::Path;

use anyhow::Result;
use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use parc_core::config::load_config;
use parc_core::search::SearchResult;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

mod actions;
mod app;
mod data;
mod markdown;
mod ui;

#[derive(Clone)]
pub(crate) enum Mode {
    Normal,
    Confirm {
        prompt: String,
        action: ConfirmAction,
    },
    Input {
        prompt: String,
        value: String,
        action: InputAction,
    },
    Help,
}

#[derive(Clone)]
pub(crate) enum ConfirmAction {
    Delete { id: String },
}

#[derive(Clone)]
pub(crate) enum InputAction {
    Promote { id: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Today,
    List,
    Stale,
    Search,
}

impl Tab {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Tab::Today => "Today",
            Tab::List => "List",
            Tab::Stale => "Stale",
            Tab::Search => "Search",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Tab::Today => Tab::List,
            Tab::List => Tab::Stale,
            Tab::Stale => Tab::Search,
            Tab::Search => Tab::Today,
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Tab::Today => 0,
            Tab::List => 1,
            Tab::Stale => 2,
            Tab::Search => 3,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    List,
    Detail,
}

impl Focus {
    pub(crate) fn toggle(self) -> Self {
        match self {
            Focus::List => Focus::Detail,
            Focus::Detail => Focus::List,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Row {
    pub id: String,
    pub title: String,
    pub fragment_type: String,
    pub status: Option<String>,
    pub section: Option<String>,
}

impl From<SearchResult> for Row {
    fn from(result: SearchResult) -> Self {
        Row {
            id: result.id,
            title: result.title,
            fragment_type: result.fragment_type,
            status: result.status,
            section: None,
        }
    }
}

pub fn run(vault: &Path) -> Result<()> {
    let config = load_config(vault)?;
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, Hide)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::run_loop(&mut terminal, vault, &config);

    execute!(terminal.backend_mut(), Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}
