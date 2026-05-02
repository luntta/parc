use std::io;
use std::path::Path;

use anyhow::Result;
use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use parc_core::config::load_config;
use parc_core::fuzzy::FuzzyHit;
use parc_core::search::SearchResult;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::ListState;
use ratatui::Terminal;

mod actions;
mod app;
mod cache;
mod command;
mod data;
mod highlight;
mod markdown;
mod ui;

use command::{CommandEntry, LauncherKind};

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
    Capture(CaptureForm),
    Launcher(LauncherPopup),
    Help,
}

#[derive(Clone)]
pub(crate) enum ConfirmAction {
    Delete { id: String },
}

#[derive(Clone)]
pub(crate) enum InputAction {
    Promote { id: String },
    SetField { id: String, field: QuickField },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuickField {
    Status,
    Due,
    Priority,
    Assignee,
    Tags,
}

impl QuickField {
    pub(crate) fn label(self) -> &'static str {
        match self {
            QuickField::Status => "Status",
            QuickField::Due => "Due",
            QuickField::Priority => "Priority",
            QuickField::Assignee => "Assignee",
            QuickField::Tags => "Tags",
        }
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            QuickField::Status => "status",
            QuickField::Due => "due",
            QuickField::Priority => "priority",
            QuickField::Assignee => "assignee",
            QuickField::Tags => "tags",
        }
    }
}

#[derive(Clone)]
pub(crate) struct CaptureForm {
    pub text: String,
    pub type_choices: Vec<String>,
    pub type_index: usize,
    pub tags: String,
    pub status: String,
    pub due: String,
    pub priority: String,
    pub assignee: String,
    pub focus: CaptureField,
}

impl CaptureForm {
    pub(crate) fn new(type_choices: Vec<String>) -> Self {
        let type_choices = if type_choices.is_empty() {
            vec!["note".to_string()]
        } else {
            type_choices
        };
        let type_index = type_choices
            .iter()
            .position(|name| name == "note")
            .unwrap_or(0);

        Self {
            text: String::new(),
            type_choices,
            type_index,
            tags: String::new(),
            status: String::new(),
            due: String::new(),
            priority: String::new(),
            assignee: String::new(),
            focus: CaptureField::Text,
        }
    }

    pub(crate) fn current_type(&self) -> &str {
        self.type_choices
            .get(self.type_index)
            .map(String::as_str)
            .unwrap_or("note")
    }

    pub(crate) fn next_type(&mut self) {
        if !self.type_choices.is_empty() {
            self.type_index = (self.type_index + 1) % self.type_choices.len();
        }
    }

    pub(crate) fn previous_type(&mut self) {
        if !self.type_choices.is_empty() {
            self.type_index = if self.type_index == 0 {
                self.type_choices.len() - 1
            } else {
                self.type_index - 1
            };
        }
    }

    pub(crate) fn select_type_starting_with(&mut self, ch: char) {
        if let Some(idx) = self
            .type_choices
            .iter()
            .position(|name| name.starts_with(ch.to_ascii_lowercase()))
        {
            self.type_index = idx;
        }
    }

    pub(crate) fn next_field(&mut self) {
        self.focus = self.focus.next();
    }

    pub(crate) fn previous_field(&mut self) {
        self.focus = self.focus.previous();
    }

    pub(crate) fn push_char(&mut self, ch: char) {
        match self.focus {
            CaptureField::Text => self.text.push(ch),
            CaptureField::Type => self.select_type_starting_with(ch),
            CaptureField::Tags => self.tags.push(ch),
            CaptureField::Status => self.status.push(ch),
            CaptureField::Due => self.due.push(ch),
            CaptureField::Priority => self.priority.push(ch),
            CaptureField::Assignee => self.assignee.push(ch),
        }
    }

    pub(crate) fn backspace(&mut self) {
        match self.focus {
            CaptureField::Text => {
                self.text.pop();
            }
            CaptureField::Type => {}
            CaptureField::Tags => {
                self.tags.pop();
            }
            CaptureField::Status => {
                self.status.pop();
            }
            CaptureField::Due => {
                self.due.pop();
            }
            CaptureField::Priority => {
                self.priority.pop();
            }
            CaptureField::Assignee => {
                self.assignee.pop();
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureField {
    Text,
    Type,
    Tags,
    Status,
    Due,
    Priority,
    Assignee,
}

impl CaptureField {
    pub(crate) fn label(self) -> &'static str {
        match self {
            CaptureField::Text => "Text",
            CaptureField::Type => "Type",
            CaptureField::Tags => "Tags",
            CaptureField::Status => "Status",
            CaptureField::Due => "Due",
            CaptureField::Priority => "Priority",
            CaptureField::Assignee => "Assignee",
        }
    }

    fn next(self) -> Self {
        match self {
            CaptureField::Text => CaptureField::Type,
            CaptureField::Type => CaptureField::Tags,
            CaptureField::Tags => CaptureField::Status,
            CaptureField::Status => CaptureField::Due,
            CaptureField::Due => CaptureField::Priority,
            CaptureField::Priority => CaptureField::Assignee,
            CaptureField::Assignee => CaptureField::Text,
        }
    }

    fn previous(self) -> Self {
        match self {
            CaptureField::Text => CaptureField::Assignee,
            CaptureField::Type => CaptureField::Text,
            CaptureField::Tags => CaptureField::Type,
            CaptureField::Status => CaptureField::Tags,
            CaptureField::Due => CaptureField::Status,
            CaptureField::Priority => CaptureField::Due,
            CaptureField::Assignee => CaptureField::Priority,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Today,
    List,
    Stale,
}

impl Tab {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Tab::Today => "Today",
            Tab::List => "List",
            Tab::Stale => "Stale",
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Tab::Today => Tab::List,
            Tab::List => Tab::Stale,
            Tab::Stale => Tab::Today,
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Tab::Today => 0,
            Tab::List => 1,
            Tab::Stale => 2,
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
    pub priority: Option<String>,
    pub due: Option<String>,
    pub assignee: Option<String>,
    pub tags: Vec<String>,
    pub updated_at: String,
    pub section: Option<String>,
    pub title_match_indices: Vec<u32>,
}

impl From<SearchResult> for Row {
    fn from(result: SearchResult) -> Self {
        Row {
            id: result.id,
            title: result.title,
            fragment_type: result.fragment_type,
            status: result.status,
            priority: None,
            due: None,
            assignee: None,
            tags: result.tags,
            updated_at: result.updated_at,
            section: None,
            title_match_indices: Vec::new(),
        }
    }
}

impl From<FuzzyHit> for Row {
    fn from(hit: FuzzyHit) -> Self {
        Row {
            id: hit.item.id,
            title: hit.item.title,
            fragment_type: hit.item.fragment_type,
            status: hit.item.status,
            priority: hit.item.priority,
            due: hit.item.due,
            assignee: hit.item.assignee,
            tags: hit.item.tags,
            updated_at: hit.item.updated_at,
            section: None,
            title_match_indices: hit.title_match_indices,
        }
    }
}

#[derive(Clone)]
pub(crate) struct LauncherPopup {
    pub input: String,
    pub rows: Vec<Row>,
    pub commands: Vec<CommandEntry>,
    pub list_state: ListState,
    pub focus: Focus,
    pub detail_scroll: u16,
    pub detail_max_scroll: u16,
    pub error: Option<String>,
}

impl LauncherPopup {
    pub(crate) fn new(input: String) -> Self {
        Self {
            input,
            rows: Vec::new(),
            commands: Vec::new(),
            list_state: ListState::default(),
            focus: Focus::List,
            detail_scroll: 0,
            detail_max_scroll: 0,
            error: None,
        }
    }

    pub(crate) fn kind(&self) -> LauncherKind {
        command::launcher_kind(&self.input)
    }

    pub(crate) fn selected_id(&self) -> Option<String> {
        if self.kind() != LauncherKind::Fragments {
            return None;
        }
        let idx = self.list_state.selected()?;
        self.rows.get(idx).map(|row| row.id.clone())
    }

    pub(crate) fn selected_command(&self) -> Option<CommandEntry> {
        if self.kind() != LauncherKind::Commands {
            return None;
        }
        let idx = self.list_state.selected()?;
        self.commands.get(idx).copied()
    }

    pub(crate) fn select_first(&mut self) {
        if self.item_count() == 0 {
            self.list_state.select(None);
        } else {
            self.list_state.select(Some(0));
        }
        self.detail_scroll = 0;
    }

    pub(crate) fn move_list(&mut self, delta: i32) {
        let len = self.item_count();
        if len == 0 {
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
        if Some(next) != self.list_state.selected() {
            self.list_state.select(Some(next));
            self.detail_scroll = 0;
        }
    }

    pub(crate) fn scroll_detail(&mut self, delta: i32) {
        let cur = self.detail_scroll as i32;
        let max = self.detail_max_scroll as i32;
        self.detail_scroll = (cur + delta).clamp(0, max) as u16;
    }

    pub(crate) fn clamp_selection(&mut self) {
        let len = self.item_count();
        if len == 0 {
            self.list_state.select(None);
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0);
        if cur >= len {
            self.list_state.select(Some(len - 1));
        }
    }

    pub(crate) fn item_count(&self) -> usize {
        match self.kind() {
            LauncherKind::Fragments => self.rows.len(),
            LauncherKind::Commands => self.commands.len(),
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
