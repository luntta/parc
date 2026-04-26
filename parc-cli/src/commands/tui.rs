use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::queue;
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{
    self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};
use parc_core::config::{load_config, Config};
use parc_core::fragment;
use parc_core::search::{CompareOp, DateFilter, Filter, SearchQuery, SearchResult, SortOrder};

use crate::commands::resurfacing;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Today,
    List,
    Stale,
    Search,
}

impl Tab {
    fn title(self) -> &'static str {
        match self {
            Tab::Today => "Today",
            Tab::List => "List",
            Tab::Stale => "Stale",
            Tab::Search => "Search",
        }
    }

    fn next(self) -> Self {
        match self {
            Tab::Today => Tab::List,
            Tab::List => Tab::Stale,
            Tab::Stale => Tab::Search,
            Tab::Search => Tab::Today,
        }
    }
}

#[derive(Clone)]
struct Row {
    id: String,
    title: String,
    fragment_type: String,
    status: Option<String>,
    section: Option<String>,
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
    let result = run_loop(&mut stdout, vault, &config);
    execute!(stdout, Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    result
}

fn run_loop(stdout: &mut io::Stdout, vault: &Path, config: &Config) -> Result<()> {
    let mut tab = Tab::Today;
    let mut selected = 0usize;
    let mut search_input = String::new();
    let mut rows = load_rows(vault, tab, &search_input, config)?;
    let mut status = String::new();

    loop {
        if selected >= rows.len() {
            selected = rows.len().saturating_sub(1);
        }

        draw(stdout, vault, config, tab, &rows, selected, &search_input, &status)?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') if key.modifiers.is_empty() => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Tab => {
                    tab = tab.next();
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                    status.clear();
                }
                KeyCode::Char('1') => {
                    tab = Tab::Today;
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                }
                KeyCode::Char('2') => {
                    tab = Tab::List;
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                }
                KeyCode::Char('3') => {
                    tab = Tab::Stale;
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                }
                KeyCode::Char('4') | KeyCode::Char('/') => {
                    tab = Tab::Search;
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                }
                KeyCode::Char('r') if tab != Tab::Search => {
                    rows = load_rows(vault, tab, &search_input, config)?;
                    status = "reloaded".to_string();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if selected + 1 < rows.len() {
                        selected += 1;
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Backspace if tab == Tab::Search => {
                    search_input.pop();
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                }
                KeyCode::Esc if tab == Tab::Search && !search_input.is_empty() => {
                    search_input.clear();
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                }
                KeyCode::Char(c) if tab == Tab::Search && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    search_input.push(c);
                    selected = 0;
                    rows = load_rows(vault, tab, &search_input, config)?;
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }

    Ok(())
}

fn load_rows(vault: &Path, tab: Tab, search_input: &str, config: &Config) -> Result<Vec<Row>> {
    match tab {
        Tab::Today => load_today_rows(vault, config),
        Tab::List => query_rows(
            vault,
            SearchQuery {
                text_terms: Vec::new(),
                filters: Vec::new(),
                sort: SortOrder::UpdatedDesc,
                limit: Some(200),
            },
        ),
        Tab::Stale => load_stale_rows(vault, config),
        Tab::Search => {
            if search_input.trim().is_empty() {
                return Ok(Vec::new());
            }
            let mut query = parc_core::search::parse_query(search_input)?;
            query.limit = Some(200);
            query_rows(vault, query)
        }
    }
}

fn query_rows(vault: &Path, query: SearchQuery) -> Result<Vec<Row>> {
    Ok(resurfacing::run_search(vault, &query)?
        .into_iter()
        .map(Row::from)
        .collect())
}

fn load_today_rows(vault: &Path, config: &Config) -> Result<Vec<Row>> {
    let today = resurfacing::today_string();
    let limit = config.resurfacing.today_section_limit;
    let mut rows = Vec::new();

    let touched = resurfacing::merge_unique(vec![
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![Filter::Created(DateFilter::Absolute {
                    op: CompareOp::Eq,
                    date: today.clone(),
                })],
                sort: SortOrder::UpdatedDesc,
                limit: Some(limit),
            },
        )?,
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![Filter::Updated(DateFilter::Absolute {
                    op: CompareOp::Eq,
                    date: today.clone(),
                })],
                sort: SortOrder::UpdatedDesc,
                limit: Some(limit),
            },
        )?,
    ]);
    push_section(&mut rows, "Touched today", touched.into_iter().take(limit));

    let mut due_filters = vec![
        Filter::Type {
            value: "todo".to_string(),
            negated: false,
        },
        Filter::Due(DateFilter::Absolute {
            op: CompareOp::Lte,
            date: today,
        }),
    ];
    due_filters.extend(resurfacing::unfinished_status_filters());
    push_section(
        &mut rows,
        "Due today / overdue",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: due_filters,
                sort: SortOrder::CreatedAsc,
                limit: Some(limit),
            },
        )?
        .into_iter(),
    );

    push_section(
        &mut rows,
        "Open & high priority",
        resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters: vec![
                    Filter::Type {
                        value: "todo".to_string(),
                        negated: false,
                    },
                    Filter::Status {
                        value: "open".to_string(),
                        negated: false,
                    },
                    Filter::Priority {
                        op: CompareOp::Gte,
                        value: "high".to_string(),
                        negated: false,
                    },
                ],
                sort: SortOrder::UpdatedDesc,
                limit: Some(limit),
            },
        )?
        .into_iter(),
    );

    Ok(rows)
}

fn load_stale_rows(vault: &Path, config: &Config) -> Result<Vec<Row>> {
    let cutoff = resurfacing::days_ago_string(config.resurfacing.stale_days);
    let mut groups = Vec::new();

    for fragment_type in ["todo", "decision", "risk"] {
        let mut filters = vec![
            Filter::Type {
                value: fragment_type.to_string(),
                negated: false,
            },
            Filter::Updated(DateFilter::Absolute {
                op: CompareOp::Lt,
                date: cutoff.clone(),
            }),
        ];
        filters.extend(resurfacing::unfinished_status_filters());
        groups.push(resurfacing::run_search(
            vault,
            &SearchQuery {
                text_terms: Vec::new(),
                filters,
                sort: SortOrder::UpdatedAsc,
                limit: Some(200),
            },
        )?);
    }

    let mut results = resurfacing::merge_unique(groups);
    results.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
    Ok(results.into_iter().map(Row::from).collect())
}

fn push_section(rows: &mut Vec<Row>, section: &str, results: impl Iterator<Item = SearchResult>) {
    for mut row in results.map(Row::from) {
        row.section = Some(section.to_string());
        rows.push(row);
    }
}

fn draw(
    stdout: &mut io::Stdout,
    vault: &Path,
    config: &Config,
    tab: Tab,
    rows: &[Row],
    selected: usize,
    search_input: &str,
    status: &str,
) -> Result<()> {
    let (width, height) = terminal::size()?;
    let left_width = (width / 2).max(32);
    let detail_x = left_width.saturating_add(2);
    let content_height = height.saturating_sub(3) as usize;

    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
    draw_tabs(stdout, tab)?;

    if tab == Tab::Search {
        queue!(
            stdout,
            MoveTo(0, 1),
            Print(format!("/{}", truncate(search_input, width as usize - 1)))
        )?;
    }

    let start_y = if tab == Tab::Search { 3 } else { 2 };
    let visible_rows = content_height.saturating_sub(start_y as usize);
    for (idx, row) in rows.iter().take(visible_rows).enumerate() {
        let y = start_y + idx as u16;
        queue!(stdout, MoveTo(0, y))?;
        if idx == selected {
            queue!(stdout, SetAttribute(Attribute::Reverse))?;
        }
        let section = row.section.as_deref().unwrap_or("");
        let short = short_id(&row.id, config.id_display_length);
        let status = row.status.as_deref().unwrap_or("-");
        let line = if section.is_empty() {
            format!("{}  {:<8} {:<10} {}", short, row.fragment_type, status, row.title)
        } else {
            format!("{}  {:<8} {:<10} {} · {}", short, row.fragment_type, status, section, row.title)
        };
        queue!(stdout, Print(truncate(&line, left_width as usize)))?;
        if idx == selected {
            queue!(stdout, SetAttribute(Attribute::Reset))?;
        }
    }

    draw_detail(stdout, vault, rows.get(selected), detail_x, 2, width, height)?;

    let footer = if status.is_empty() {
        "tab/1-4 tabs  j/k move  / search  r reload  q quit".to_string()
    } else {
        format!("{}  ·  tab/1-4 tabs  j/k move  / search  q quit", status)
    };
    queue!(
        stdout,
        MoveTo(0, height.saturating_sub(1)),
        SetAttribute(Attribute::Reverse),
        Print(truncate(&footer, width as usize)),
        SetAttribute(Attribute::Reset)
    )?;

    stdout.flush()?;
    Ok(())
}

fn draw_tabs(stdout: &mut io::Stdout, active: Tab) -> Result<()> {
    for tab in [Tab::Today, Tab::List, Tab::Stale, Tab::Search] {
        if tab == active {
            queue!(stdout, SetAttribute(Attribute::Reverse))?;
        }
        queue!(stdout, Print(format!(" {} ", tab.title())))?;
        if tab == active {
            queue!(stdout, SetAttribute(Attribute::Reset))?;
        }
        queue!(stdout, Print(" "))?;
    }
    Ok(())
}

fn draw_detail(
    stdout: &mut io::Stdout,
    vault: &Path,
    row: Option<&Row>,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) -> Result<()> {
    let Some(row) = row else {
        queue!(stdout, MoveTo(x, y), Print("No selection"))?;
        return Ok(());
    };

    let fragment = fragment::read_fragment(vault, &row.id)?;
    let detail_width = width.saturating_sub(x).saturating_sub(1) as usize;
    let mut lines = vec![
        fragment.title,
        format!("ID: {}", fragment.id),
        format!("Type: {}", fragment.fragment_type),
    ];
    if !fragment.tags.is_empty() {
        lines.push(format!("Tags: {}", fragment.tags.join(", ")));
    }
    for (key, value) in &fragment.extra_fields {
        if let Some(s) = value.as_str() {
            lines.push(format!("{}: {}", key, s));
        }
    }
    lines.push(String::new());
    lines.extend(fragment.body.lines().map(|line| line.to_string()));

    for (idx, line) in lines
        .into_iter()
        .take(height.saturating_sub(y + 1) as usize)
        .enumerate()
    {
        queue!(
            stdout,
            MoveTo(x, y + idx as u16),
            Print(truncate(&line, detail_width))
        )?;
    }

    Ok(())
}

fn short_id(id: &str, len: usize) -> &str {
    if id.len() > len {
        &id[..len]
    } else {
        id
    }
}

fn truncate(s: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for c in s.chars().take(max_width) {
        out.push(c);
    }
    out
}
