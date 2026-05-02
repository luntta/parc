use std::path::Path;

use parc_core::config::Config;
use parc_core::search::{parse_query, TextTerm};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Tabs, Wrap,
};
use ratatui::Frame;

use super::app::App;
use super::cache::FragmentCache;
use super::highlight;
use super::markdown;
use super::{CaptureField, CaptureForm, Focus, Mode, Row, SearchPopup, Tab};

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

    let outer = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .split(area);

    draw_menu(frame, outer[0], app.tab);

    let body = outer[1];
    let left_width = (body.width / 2).max(34).min(body.width.saturating_sub(24));
    let body_chunks =
        Layout::horizontal([Constraint::Length(left_width), Constraint::Min(1)]).split(body);

    draw_list(frame, body_chunks[0], app, config);
    draw_detail(frame, body_chunks[1], vault, app);

    let status = app.status.text().to_string();
    draw_footer(frame, outer[2], &status, app.focus);

    match &mut app.mode {
        Mode::Normal => {}
        Mode::Help => draw_help(frame, area),
        Mode::Confirm { prompt, .. } => draw_confirm(frame, area, prompt),
        Mode::Input { prompt, value, .. } => draw_input(frame, area, prompt, value),
        Mode::Capture(form) => draw_capture(frame, area, form),
        Mode::Search(popup) => {
            draw_search_popup(frame, area, vault, config, &mut app.cache, popup, &status)
        }
    }
}

fn draw_menu(frame: &mut Frame, area: Rect, tab: Tab) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(MENU_BORDER))
        .title(" parc ");
    let titles: Vec<&str> = [Tab::Today, Tab::List, Tab::Stale]
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
    let prefix = match &row.section {
        Some(section) => format!(
            "{}  {:<8} {:<10} {} - ",
            short, row.fragment_type, status, section
        ),
        None => format!("{}  {:<8} {:<10} ", short, row.fragment_type, status),
    };

    let mut spans = vec![Span::raw(prefix)];
    spans.extend(highlight::spans_for_text(
        &row.title,
        Style::default(),
        &row.title_match_indices,
        &[],
    ));
    Line::from(spans)
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

    let selected_row = app
        .list_state
        .selected()
        .and_then(|i| app.rows.get(i))
        .cloned();
    let Some(row) = selected_row else {
        let paragraph = Paragraph::new(Line::from(Span::styled(
            "No selection",
            Style::default().fg(MUTED_TEXT),
        )))
        .block(block);
        frame.render_widget(paragraph, area);
        app.detail_max_scroll = 0;
        return;
    };
    let lines = detail_lines(vault, &mut app.cache, &row, None);
    render_detail_lines(
        frame,
        area,
        block,
        lines,
        &mut app.detail_scroll,
        &mut app.detail_max_scroll,
    );
}

fn detail_lines(
    vault: &Path,
    cache: &mut FragmentCache,
    row: &Row,
    search_input: Option<&str>,
) -> Vec<Line<'static>> {
    let id = row.id.clone();
    let search_terms = search_input.map(parsed_search_terms).unwrap_or_default();
    let title_match_indices: &[u32] = if search_input.is_some() {
        row.title_match_indices.as_slice()
    } else {
        &[]
    };

    match cache.get_or_load(vault, &id) {
        Ok(fragment) => {
            let muted = Style::default().fg(MUTED_TEXT);
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(highlight::spans_for_text(
                &fragment.title,
                Style::default().add_modifier(Modifier::BOLD),
                title_match_indices,
                &search_terms,
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
            if search_input.is_some() {
                lines.extend(markdown::render_body_highlighted(
                    &fragment.body,
                    &search_terms,
                ));
            } else {
                lines.extend(markdown::render_body(&fragment.body));
            }
            lines
        }
        Err(err) => vec![Line::from(Span::styled(
            format!("Failed to load fragment: {}", err),
            Style::default().fg(Color::Red),
        ))],
    }
}

fn render_detail_lines(
    frame: &mut Frame,
    area: Rect,
    block: Block<'static>,
    lines: Vec<Line<'static>>,
    detail_scroll: &mut u16,
    detail_max_scroll: &mut u16,
) {
    let inner = block.inner(area);
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    let total = paragraph.line_count(inner.width) as u16;
    let viewport = inner.height;
    *detail_max_scroll = total.saturating_sub(viewport);
    if *detail_scroll > *detail_max_scroll {
        *detail_scroll = *detail_max_scroll;
    }

    let paragraph = paragraph.scroll((*detail_scroll, 0));
    frame.render_widget(paragraph, area);

    if total > viewport {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        let mut scrollbar_state = ScrollbarState::new(total.saturating_sub(viewport) as usize)
            .position(*detail_scroll as usize);
        frame.render_stateful_widget(scrollbar, inner, &mut scrollbar_state);
    }
}

fn draw_search_popup(
    frame: &mut Frame,
    area: Rect,
    vault: &Path,
    config: &Config,
    cache: &mut FragmentCache,
    popup: &mut SearchPopup,
    status: &str,
) {
    popup.clamp_selection();
    let popup_area = centered_rect(92, 86, area);
    frame.render_widget(Clear, popup_area);

    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(ACTIVE_TAB))
        .title(" search ");
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.width < 24 || inner.height < 5 {
        let paragraph = Paragraph::new("Search needs a larger terminal.")
            .style(Style::default().fg(Color::Red));
        frame.render_widget(paragraph, inner);
        return;
    }

    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    draw_search_query(frame, chunks[0], &popup.input);

    let body = chunks[1];
    let left_width = (body.width / 2).max(32).min(body.width.saturating_sub(24));
    let panes =
        Layout::horizontal([Constraint::Length(left_width), Constraint::Min(1)]).split(body);
    draw_search_results(frame, panes[0], popup, config);
    draw_search_preview(frame, panes[1], vault, cache, popup);
    draw_search_footer(frame, chunks[2], popup, status);
}

fn draw_search_query(frame: &mut Frame, area: Rect, search_input: &str) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(MENU_BORDER))
        .title(" query ");
    let line = Line::from(vec![
        Span::styled("> ", Style::default().fg(ACTIVE_TAB)),
        Span::raw(search_input.to_string()),
        Span::styled("_", Style::default().fg(ACTIVE_TAB)),
    ]);
    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_search_results(frame: &mut Frame, area: Rect, popup: &mut SearchPopup, config: &Config) {
    let total = popup.rows.len();
    let title = if total == 0 {
        " results ".to_string()
    } else {
        let cur = popup.list_state.selected().map(|i| i + 1).unwrap_or(0);
        format!(" results  {}/{} ", cur, total)
    };
    let border_color = if popup.focus == Focus::List {
        LIST_BORDER_FOCUSED
    } else {
        LIST_BORDER
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let items: Vec<ListItem> = if popup.input.trim().is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Type a query to search",
            Style::default().fg(MUTED_TEXT),
        )))]
    } else if let Some(err) = &popup.error {
        vec![ListItem::new(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        )))]
    } else if popup.rows.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No results",
            Style::default().fg(MUTED_TEXT),
        )))]
    } else {
        popup
            .rows
            .iter()
            .map(|row| ListItem::new(format_row(row, config.id_display_length)))
            .collect()
    };

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, &mut popup.list_state);
}

fn draw_search_preview(
    frame: &mut Frame,
    area: Rect,
    vault: &Path,
    cache: &mut FragmentCache,
    popup: &mut SearchPopup,
) {
    let border_color = if popup.focus == Focus::Detail {
        DETAIL_BORDER_FOCUSED
    } else {
        DETAIL_BORDER
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border_color))
        .title(" preview ");

    let selected_row = popup
        .list_state
        .selected()
        .and_then(|idx| popup.rows.get(idx))
        .cloned();
    let Some(row) = selected_row else {
        let message = if popup.input.trim().is_empty() {
            "Preview appears here"
        } else {
            "No selection"
        };
        let paragraph = Paragraph::new(Line::from(Span::styled(
            message,
            Style::default().fg(MUTED_TEXT),
        )))
        .block(block);
        frame.render_widget(paragraph, area);
        popup.detail_max_scroll = 0;
        return;
    };

    let lines = detail_lines(vault, cache, &row, Some(&popup.input));
    render_detail_lines(
        frame,
        area,
        block,
        lines,
        &mut popup.detail_scroll,
        &mut popup.detail_max_scroll,
    );
}

fn draw_search_footer(frame: &mut Frame, area: Rect, popup: &SearchPopup, status: &str) {
    let focus_label = match popup.focus {
        Focus::List => "[results]",
        Focus::Detail => "[preview]",
    };
    let text = if let Some(err) = &popup.error {
        Line::from(Span::styled(err.clone(), Style::default().fg(Color::Red)))
    } else if !status.is_empty() {
        Line::from(Span::styled(
            status.to_string(),
            Style::default().fg(ACTIVE_TAB),
        ))
    } else {
        Line::from(Span::styled(
            format!(
                "{} type query  arrows move  S-tab focus  Enter edit  Esc close",
                focus_label
            ),
            Style::default().fg(MUTED_TEXT),
        ))
    };
    frame.render_widget(Paragraph::new(text), area);
}

fn parsed_search_terms(search_input: &str) -> Vec<String> {
    parse_query(search_input)
        .map(|query| {
            query
                .text_terms
                .into_iter()
                .filter_map(|term| match term {
                    TextTerm::Word(word) | TextTerm::Phrase(word) => Some(word),
                })
                .collect()
        })
        .unwrap_or_default()
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
        "{} 1-3 tabs  / search  S-tab focus  arrows move  c capture  e edit  t toggle  p promote  a archive  d delete  y yank  ? help  q quit",
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
        Line::from("  1-3         switch tab (Today/List/Stale)"),
        Line::from("  Tab         cycle tabs"),
        Line::from("  Shift-Tab   toggle pane focus (list / detail)"),
        Line::from("  / or 4      open search popup"),
        Line::from("  ↓ / ↑       move within focused pane"),
        Line::from("  PgDn/PgUp   page within focused pane"),
        Line::from("  Home / End  top / bottom of focused pane"),
        Line::from("  Ctrl-d/u    half-page scroll"),
        Line::from(""),
        Line::from("Actions on selected fragment"),
        Line::from("  c           capture a new fragment"),
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
        Line::from("  Search      type to query, Enter edits, Esc closes"),
        Line::from("  q           quit"),
        Line::from("  Esc         cancel modal / close search"),
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

fn draw_capture(frame: &mut Frame, area: Rect, form: &CaptureForm) {
    let popup = centered_rect_lines(88, 14, area);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(ACTIVE_TAB))
        .title(" capture ");

    let body = vec![
        Line::from(""),
        capture_field_line(form, CaptureField::Text, &form.text),
        capture_field_line(form, CaptureField::Type, form.current_type()),
        capture_field_line(form, CaptureField::Tags, &form.tags),
        capture_field_line(form, CaptureField::Status, &form.status),
        capture_field_line(form, CaptureField::Due, &form.due),
        capture_field_line(form, CaptureField::Priority, &form.priority),
        capture_field_line(form, CaptureField::Assignee, &form.assignee),
        Line::from(""),
        Line::from(Span::styled(
            "Enter create  -  Tab fields  -  Esc cancel",
            Style::default().fg(MUTED_TEXT),
        )),
        Line::from(Span::styled(
            "Type field: arrows cycle, first letter jumps",
            Style::default().fg(MUTED_TEXT),
        )),
    ];

    let paragraph = Paragraph::new(body).block(block);
    frame.render_widget(paragraph, popup);
}

fn capture_field_line(form: &CaptureForm, field: CaptureField, value: &str) -> Line<'static> {
    let active = form.focus == field;
    let border_style = if active {
        Style::default().fg(ACTIVE_TAB)
    } else {
        Style::default().fg(MUTED_TEXT)
    };
    let label_style = border_style.add_modifier(Modifier::BOLD);
    let value_style = if active {
        Style::default().fg(Color::White)
    } else {
        Style::default()
    };
    let display = if field == CaptureField::Type {
        format!("{} / {}", form.type_index + 1, form.type_choices.len())
    } else {
        value.to_string()
    };
    let value = if field == CaptureField::Type {
        format!("{}  {}", value, display)
    } else {
        display
    };
    let value = fit_capture_value(&format!("{}{}", value, if active { "_" } else { "" }), 40);

    Line::from(vec![
        Span::raw("  "),
        Span::styled("┌─", border_style),
        Span::styled(format!(" {:<9} ", field.label()), label_style),
        Span::styled("┤ ", border_style),
        Span::styled(format!("{:<40}", value), value_style),
        Span::styled(" ├─┐", border_style),
    ])
}

fn fit_capture_value(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len <= width {
        value.to_string()
    } else {
        value.chars().skip(len.saturating_sub(width)).collect()
    }
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
