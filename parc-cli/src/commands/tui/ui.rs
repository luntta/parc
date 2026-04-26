use std::path::Path;

use parc_core::config::Config;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Tabs, Wrap,
};
use ratatui::Frame;

use super::app::App;
use super::markdown;
use super::{Focus, Mode, Row, Tab};

const MENU_BORDER: Color = Color::DarkGray;
const LIST_BORDER: Color = Color::Blue;
const LIST_BORDER_FOCUSED: Color = Color::LightBlue;
const DETAIL_BORDER: Color = Color::Green;
const DETAIL_BORDER_FOCUSED: Color = Color::LightGreen;
const FOOTER_BORDER: Color = Color::DarkGray;
const ACTIVE_TAB: Color = Color::Yellow;
const MUTED_TEXT: Color = Color::DarkGray;

pub(super) fn draw(frame: &mut Frame, vault: &Path, config: &Config, app: &mut App) {
    let area = frame.area();
    if area.width < 48 || area.height < 10 {
        let msg = Paragraph::new("Terminal is too small for the TUI.")
            .style(Style::default().fg(Color::Red));
        frame.render_widget(msg, area);
        return;
    }

    let search_height = if app.tab == Tab::Search { 3 } else { 0 };
    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(search_height),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);

    draw_menu(frame, outer[0], app.tab);
    if app.tab == Tab::Search {
        draw_search_input(frame, outer[1], &app.search_input);
    }

    let body = outer[2];
    let left_width = (body.width / 2).max(34).min(body.width.saturating_sub(24));
    let body_chunks =
        Layout::horizontal([Constraint::Length(left_width), Constraint::Min(1)]).split(body);

    draw_list(frame, body_chunks[0], app, config);
    draw_detail(frame, body_chunks[1], vault, app);

    draw_footer(frame, outer[3], app.status.text(), app.focus);

    match &app.mode {
        Mode::Normal => {}
        Mode::Help => draw_help(frame, area),
        Mode::Confirm { prompt, .. } => draw_confirm(frame, area, prompt),
        Mode::Input { prompt, value, .. } => draw_input(frame, area, prompt, value),
    }
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

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App, config: &Config) {
    let total = app.rows.len();
    let title = if total == 0 {
        format!(" {} ", app.tab.title())
    } else {
        let cur = app.list_state.selected().map(|i| i + 1).unwrap_or(0);
        format!(" {}  {}/{} ", app.tab.title(), cur, total)
    };
    let border_color = if app.focus == Focus::List {
        LIST_BORDER_FOCUSED
    } else {
        LIST_BORDER
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| ListItem::new(format_row(row, config.id_display_length)))
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, &mut app.list_state);
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

fn draw_detail(frame: &mut Frame, area: Rect, vault: &Path, app: &mut App) {
    let border_color = if app.focus == Focus::Detail {
        DETAIL_BORDER_FOCUSED
    } else {
        DETAIL_BORDER
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border_color))
        .title(" detail ");

    let row_id = app
        .list_state
        .selected()
        .and_then(|i| app.rows.get(i))
        .map(|r| r.id.clone());
    let Some(id) = row_id else {
        let paragraph = Paragraph::new(Line::from(Span::styled(
            "No selection",
            Style::default().fg(MUTED_TEXT),
        )))
        .block(block);
        frame.render_widget(paragraph, area);
        app.detail_max_scroll = 0;
        return;
    };

    let lines = match app.cache.get_or_load(vault, &id) {
        Ok(fragment) => {
            let muted = Style::default().fg(MUTED_TEXT);
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(
                fragment.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                format!("ID: {}", fragment.id),
                muted,
            )));
            lines.push(Line::from(Span::styled(
                format!("Type: {}", fragment.fragment_type),
                muted,
            )));
            if !fragment.tags.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("Tags: {}", fragment.tags.join(", ")),
                    muted,
                )));
            }
            for (key, value) in &fragment.extra_fields {
                if let Some(s) = value.as_str() {
                    lines.push(Line::from(Span::styled(format!("{}: {}", key, s), muted)));
                }
            }
            lines.push(Line::from(""));
            lines.extend(markdown::render_body(&fragment.body));
            lines
        }
        Err(err) => vec![Line::from(Span::styled(
            format!("Failed to load fragment: {}", err),
            Style::default().fg(Color::Red),
        ))],
    };

    let inner = block.inner(area);
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    let total = paragraph.line_count(inner.width) as u16;
    let viewport = inner.height;
    app.detail_max_scroll = total.saturating_sub(viewport);
    if app.detail_scroll > app.detail_max_scroll {
        app.detail_scroll = app.detail_max_scroll;
    }

    let paragraph = paragraph.scroll((app.detail_scroll, 0));
    frame.render_widget(paragraph, area);

    if total > viewport {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        let mut scrollbar_state =
            ScrollbarState::new(total.saturating_sub(viewport) as usize)
                .position(app.detail_scroll as usize);
        frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, status: &str, focus: Focus) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(FOOTER_BORDER))
        .title(" keys ");
    let focus_label = match focus {
        Focus::List => "[list]",
        Focus::Detail => "[detail]",
    };
    let base = format!(
        "{} 1-4 tabs  S-tab focus  arrows move  e edit  t toggle  p promote  a archive  d delete  y yank  ? help  q quit",
        focus_label
    );
    let text = if status.is_empty() {
        base
    } else {
        format!("{}  -  {}", status, base)
    };
    let paragraph = Paragraph::new(text)
        .style(Style::default().fg(MUTED_TEXT))
        .block(block);
    frame.render_widget(paragraph, area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Navigation"),
        Line::from("  1-4         switch tab (Today/List/Stale/Search)"),
        Line::from("  Tab         cycle tabs"),
        Line::from("  Shift-Tab   toggle pane focus (list / detail)"),
        Line::from("  /           jump to search"),
        Line::from("  ↓ / ↑       move within focused pane"),
        Line::from("  PgDn/PgUp   page within focused pane"),
        Line::from("  Home / End  top / bottom of focused pane"),
        Line::from("  Ctrl-d/u    half-page scroll"),
        Line::from(""),
        Line::from("Actions on selected fragment"),
        Line::from("  e           edit in $EDITOR"),
        Line::from("  t           toggle todo status (open ↔ done)"),
        Line::from("  p           promote to another type"),
        Line::from("  a           archive (toggle)"),
        Line::from("  d           delete (with confirm)"),
        Line::from("  y           show full id in status bar"),
        Line::from("  r           reload current view"),
        Line::from(""),
        Line::from("General"),
        Line::from("  ?           toggle this help"),
        Line::from("  q           quit"),
        Line::from("  Esc         cancel modal / clear search"),
    ];

    let popup = centered_rect(64, 80, area);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(ACTIVE_TAB))
        .title(" help (Esc to close) ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup);
}

fn draw_confirm(frame: &mut Frame, area: Rect, prompt: &str) {
    let popup = centered_rect_lines(56, 5, area);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::Red))
        .title(" confirm ");
    let body = vec![
        Line::from(""),
        Line::from(prompt.to_string()),
        Line::from(""),
        Line::from(Span::styled(
            "y to confirm  -  n / Esc to cancel",
            Style::default().fg(MUTED_TEXT),
        )),
    ];
    let paragraph = Paragraph::new(body).block(block);
    frame.render_widget(paragraph, popup);
}

fn draw_input(frame: &mut Frame, area: Rect, prompt: &str, value: &str) {
    let popup = centered_rect_lines(60, 5, area);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(ACTIVE_TAB))
        .title(format!(" {} ", prompt));
    let body = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(ACTIVE_TAB)),
            Span::raw(value.to_string()),
            Span::styled("_", Style::default().fg(ACTIVE_TAB)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Enter to submit  -  Esc to cancel",
            Style::default().fg(MUTED_TEXT),
        )),
    ];
    let paragraph = Paragraph::new(body).block(block);
    frame.render_widget(paragraph, popup);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_w = area.width * percent_x / 100;
    let popup_h = area.height * percent_y / 100;
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    Rect {
        x,
        y,
        width: popup_w,
        height: popup_h,
    }
}

fn centered_rect_lines(percent_x: u16, lines: u16, area: Rect) -> Rect {
    let popup_w = (area.width * percent_x / 100).min(area.width);
    let popup_h = lines.min(area.height);
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    Rect {
        x,
        y,
        width: popup_w,
        height: popup_h,
    }
}

fn short_id(id: &str, len: usize) -> &str {
    if id.len() > len {
        &id[..len]
    } else {
        id
    }
}
