use std::path::Path;

use parc_core::config::Config;
use parc_core::fragment;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use super::{Row, Tab};

const MENU_BORDER: Color = Color::DarkGray;
const LIST_BORDER: Color = Color::Blue;
const DETAIL_BORDER: Color = Color::Green;
const FOOTER_BORDER: Color = Color::DarkGray;
const ACTIVE_TAB: Color = Color::Yellow;
const MUTED_TEXT: Color = Color::DarkGray;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    frame: &mut Frame,
    vault: &Path,
    config: &Config,
    tab: Tab,
    rows: &[Row],
    list_state: &mut ListState,
    search_input: &str,
    status: &str,
) {
    let area = frame.area();
    if area.width < 48 || area.height < 10 {
        let msg = Paragraph::new("Terminal is too small for the TUI.")
            .style(Style::default().fg(Color::Red));
        frame.render_widget(msg, area);
        return;
    }

    let search_height = if tab == Tab::Search { 3 } else { 0 };
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(search_height),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);

    draw_menu(frame, outer[0], tab);
    if tab == Tab::Search {
        draw_search_input(frame, outer[1], search_input);
    }

    let body = outer[2];
    let left_width = (body.width / 2).max(34).min(body.width.saturating_sub(24));
    let body_chunks = Layout::horizontal([
        Constraint::Length(left_width),
        Constraint::Min(1),
    ])
    .split(body);

    draw_list(frame, body_chunks[0], tab, rows, list_state, config);
    draw_detail(frame, body_chunks[1], vault, list_state, rows);

    draw_footer(frame, outer[3], status);
}

fn draw_menu(frame: &mut Frame, area: Rect, tab: Tab) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(MENU_BORDER))
        .title(" parc ");
    let titles: Vec<&str> = [Tab::Today, Tab::List, Tab::Stale, Tab::Search]
        .iter()
        .map(|t| t.title())
        .collect();
    let tabs = Tabs::new(titles)
        .block(block)
        .select(tab.index())
        .style(Style::default().fg(MUTED_TEXT))
        .highlight_style(
            Style::default()
                .fg(ACTIVE_TAB)
                .add_modifier(Modifier::REVERSED),
        )
        .divider(" ");
    frame.render_widget(tabs, area);
}

fn draw_search_input(frame: &mut Frame, area: Rect, search_input: &str) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(MENU_BORDER))
        .title(" search ");
    let line = Line::from(vec![
        Span::styled("/", Style::default().fg(ACTIVE_TAB)),
        Span::raw(search_input.to_string()),
    ]);
    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_list(
    frame: &mut Frame,
    area: Rect,
    tab: Tab,
    rows: &[Row],
    list_state: &mut ListState,
    config: &Config,
) {
    let title = format!(" {} ", tab.title());
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(LIST_BORDER))
        .title(title);

    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| ListItem::new(format_row(row, config.id_display_length)))
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, list_state);
}

fn format_row(row: &Row, id_len: usize) -> Line<'static> {
    let short = short_id(&row.id, id_len).to_string();
    let status = row.status.as_deref().unwrap_or("-").to_string();
    let title = row.title.clone();
    match &row.section {
        Some(section) => Line::from(format!(
            "{}  {:<8} {:<10} {} - {}",
            short, row.fragment_type, status, section, title
        )),
        None => Line::from(format!(
            "{}  {:<8} {:<10} {}",
            short, row.fragment_type, status, title
        )),
    }
}

fn draw_detail(
    frame: &mut Frame,
    area: Rect,
    vault: &Path,
    list_state: &ListState,
    rows: &[Row],
) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(DETAIL_BORDER))
        .title(" detail ");

    let row = list_state.selected().and_then(|i| rows.get(i));
    let Some(row) = row else {
        let paragraph = Paragraph::new(Line::from(Span::styled(
            "No selection",
            Style::default().fg(MUTED_TEXT),
        )))
        .block(block);
        frame.render_widget(paragraph, area);
        return;
    };

    let lines = match fragment::read_fragment(vault, &row.id) {
        Ok(fragment) => {
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(
                fragment.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!("ID: {}", fragment.id)));
            lines.push(Line::from(format!("Type: {}", fragment.fragment_type)));
            if !fragment.tags.is_empty() {
                lines.push(Line::from(format!("Tags: {}", fragment.tags.join(", "))));
            }
            for (key, value) in &fragment.extra_fields {
                if let Some(s) = value.as_str() {
                    lines.push(Line::from(format!("{}: {}", key, s)));
                }
            }
            lines.push(Line::from(""));
            lines.extend(fragment.body.lines().map(|l| Line::from(l.to_string())));
            lines
        }
        Err(err) => vec![Line::from(Span::styled(
            format!("Failed to load fragment: {}", err),
            Style::default().fg(Color::Red),
        ))],
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, status: &str) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(FOOTER_BORDER))
        .title(" keys ");
    let text = if status.is_empty() {
        "tab/1-4 tabs  j/k move  / search  r reload  q quit".to_string()
    } else {
        format!("{}  -  tab/1-4 tabs  j/k move  / search  q quit", status)
    };
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(MUTED_TEXT))
        .block(block);
    frame.render_widget(paragraph, area);
}

fn short_id(id: &str, len: usize) -> &str {
    if id.len() > len {
        &id[..len]
    } else {
        id
    }
}
