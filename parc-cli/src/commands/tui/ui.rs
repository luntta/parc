use std::path::Path;

use chrono::{DateTime, Utc};
use parc_core::config::Config;
use parc_core::index::{self, BacklinkInfo};
use parc_core::search::{parse_query, TextTerm};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Tabs, Wrap,
};
use ratatui::Frame;
use rusqlite::Connection;

use super::app::App;
use super::cache::FragmentCache;
use super::command::{CommandEntry, LauncherKind};
use super::highlight;
use super::markdown::{self, Actionable};
use super::{
    CaptureField, CaptureForm, Focus, LauncherIntent, LauncherItem, LauncherPopup, Mode,
    OverlayState, Row, Tab,
};

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
        Constraint::Length(4),
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
    draw_footer(frame, outer[2], &status, app);

    match &mut app.mode {
        Mode::Normal => {}
        Mode::Help => draw_help(frame, area),
        Mode::Confirm { prompt, .. } => draw_confirm(frame, area, prompt),
        Mode::Input { prompt, value, .. } => draw_input(frame, area, prompt, value),
        Mode::Capture(form) => draw_capture(frame, area, form),
        Mode::Launcher(popup) => {
            draw_launcher_popup(frame, area, vault, config, &mut app.cache, popup, &status)
        }
        // Overlay labels are painted inside draw_detail.
        Mode::Overlay(_) => {}
    }
}

fn draw_menu(frame: &mut Frame, area: Rect, tab: Tab) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(MENU_BORDER))
        .title(" parc ");
    let titles: Vec<&str> = Tab::ALL.iter().map(|t| t.title()).collect();
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

    let search_terms = if app.tab == Tab::Search {
        app.search_view_query
            .as_deref()
            .map(parsed_search_terms)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let (items, selected_visual_index) = list_items(
        &app.rows,
        config.id_display_length,
        app.list_state.selected(),
        &search_terms,
    );

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    let mut render_state = ratatui::widgets::ListState::default();
    render_state.select(selected_visual_index);
    frame.render_stateful_widget(list, area, &mut render_state);
}

fn list_items(
    rows: &[Row],
    id_len: usize,
    selected_index: Option<usize>,
    search_terms: &[String],
) -> (Vec<ListItem<'static>>, Option<usize>) {
    let mut items = Vec::new();
    let mut row_to_visual = Vec::with_capacity(rows.len());
    let mut last_section: Option<&str> = None;

    for row in rows {
        if row.section.as_deref() != last_section {
            if let Some(section) = row.section.as_deref() {
                let count = rows
                    .iter()
                    .filter(|candidate| candidate.section.as_deref() == Some(section))
                    .count();
                items.push(ListItem::new(section_header(section, count)));
            }
            last_section = row.section.as_deref();
        }

        row_to_visual.push(items.len());
        items.push(ListItem::new(format_row(row, id_len, search_terms)));
    }

    let selected_visual_index = selected_index.and_then(|idx| row_to_visual.get(idx).copied());
    (items, selected_visual_index)
}

fn section_header(section: &str, count: usize) -> Line<'static> {
    Line::from(Span::styled(
        format!("{} ({})", section, count),
        Style::default().fg(ACTIVE_TAB).add_modifier(Modifier::BOLD),
    ))
}

fn format_row(row: &Row, id_len: usize, search_terms: &[String]) -> Line<'static> {
    let short = short_id(&row.id, id_len).to_string();
    let status = row.status.as_deref().unwrap_or("-").to_string();
    let indent = if row.section.is_some() { "  " } else { "" };
    let prefix = format!(
        "{}{}  {:<8} {:<10} ",
        indent, short, row.fragment_type, status
    );

    let mut spans = vec![Span::raw(prefix)];
    spans.extend(highlight::spans_for_text(
        &row.title,
        Style::default(),
        &row.title_match_indices,
        search_terms,
    ));
    if let Some(metadata) = row_metadata(row) {
        spans.push(Span::styled(
            format!("  {}", metadata),
            Style::default().fg(MUTED_TEXT),
        ));
    }
    Line::from(spans)
}

fn row_metadata(row: &Row) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(due) = row.due.as_deref().filter(|value| !value.is_empty()) {
        parts.push(format!("due:{}", due));
    }
    if let Some(priority) = row.priority.as_deref().filter(|value| !value.is_empty()) {
        parts.push(format!("pri:{}", priority));
    }
    if let Some(assignee) = row.assignee.as_deref().filter(|value| !value.is_empty()) {
        parts.push(format!("@{}", assignee));
    }
    if let Some(age) = updated_age(&row.updated_at) {
        parts.push(format!("upd:{}", age));
    }
    for tag in row.tags.iter().take(2) {
        parts.push(format!("#{}", tag));
    }
    if row.tags.len() > 2 {
        parts.push(format!("+{} tags", row.tags.len() - 2));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn updated_age(updated_at: &str) -> Option<String> {
    let dt = DateTime::parse_from_rfc3339(updated_at)
        .ok()?
        .with_timezone(&Utc);
    let duration = Utc::now().signed_duration_since(dt);
    if duration.num_days() >= 1 {
        Some(format!("{}d", duration.num_days()))
    } else if duration.num_hours() >= 1 {
        Some(format!("{}h", duration.num_hours()))
    } else {
        Some(format!("{}m", duration.num_minutes().max(0)))
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

    let inner = block.inner(area);
    app.detail_viewport = inner.height;

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
        app.detail_items.clear();
        app.detail_body_offset = 0;
        return;
    };
    let search_input = if app.tab == Tab::Search {
        app.search_view_query.clone()
    } else {
        None
    };
    let appendix = backlinks_appendix(app, vault, &row.id);
    let detail = detail_lines(
        vault,
        &mut app.cache,
        &row,
        search_input.as_deref(),
        appendix.as_deref(),
    );
    app.detail_items = detail.items;
    app.detail_body_offset = detail.body_offset;
    render_detail_lines(
        frame,
        area,
        block,
        detail.lines,
        &mut app.detail_scroll,
        &mut app.detail_max_scroll,
    );

    if let Mode::Overlay(state) = &app.mode {
        draw_overlay_labels(
            frame,
            inner,
            app.detail_scroll,
            app.detail_body_offset,
            &app.detail_items,
            state,
        );
    }
}

fn draw_overlay_labels(
    frame: &mut Frame,
    inner: Rect,
    detail_scroll: u16,
    body_offset: usize,
    items: &[Actionable],
    state: &OverlayState,
) {
    let buf = frame.buffer_mut();
    let badge_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    for (label, item_idx) in &state.labels {
        let Some(item) = items.get(*item_idx) else {
            continue;
        };
        let absolute_line = item.logical_line + body_offset;
        if absolute_line < detail_scroll as usize {
            continue;
        }
        let row_offset = absolute_line - detail_scroll as usize;
        if row_offset >= inner.height as usize {
            continue;
        }
        let y = inner.y + row_offset as u16;
        let badge = format!("[{}]", label);
        // Anchor at column 0 of the inner area; cheap and unambiguous.
        // Skip if there isn't room for the badge.
        if inner.width < badge.len() as u16 {
            continue;
        }
        buf.set_string(inner.x, y, badge, badge_style);
    }
}

/// Output of `detail_lines`: the fully composed line list plus positional
/// metadata for in-body actions. `body_offset` is the count of header lines
/// (title, ID, type, tags, fields, blank) prepended before the rendered
/// markdown body — used to translate `Actionable.logical_line` (body-relative)
/// into a `lines` index (header-relative).
pub(super) struct DetailRender {
    pub lines: Vec<Line<'static>>,
    pub items: Vec<Actionable>,
    pub body_offset: usize,
}

/// Build the markdown for a "## Backlinks" section, or `None` if there are
/// no inbound links. Each entry is rendered as `[[id|title (type)]]` so the
/// existing wiki-link follow path picks them up automatically.
fn backlinks_appendix(app: &mut App, vault: &Path, fragment_id: &str) -> Option<String> {
    let conn: &Connection = app.ensure_index(vault)?;
    let backlinks = index::get_backlinks(conn, fragment_id).ok()?;
    if backlinks.is_empty() {
        return None;
    }
    Some(format_backlinks(&backlinks))
}

fn format_backlinks(backlinks: &[BacklinkInfo]) -> String {
    let mut out = String::from("\n\n## Backlinks\n\n");
    for bl in backlinks {
        // Sanitize title for wiki-link alias: strip the bracket and pipe
        // characters that would otherwise re-enter wiki-link parsing.
        let safe_title: String = bl
            .source_title
            .chars()
            .filter(|c| *c != ']' && *c != '[' && *c != '|')
            .collect();
        let display = if safe_title.trim().is_empty() {
            format!("({})", bl.source_type)
        } else {
            format!("{} ({})", safe_title, bl.source_type)
        };
        out.push_str(&format!("- [[{}|{}]]\n", bl.source_id, display));
    }
    out
}

fn detail_lines(
    vault: &Path,
    cache: &mut FragmentCache,
    row: &Row,
    search_input: Option<&str>,
    appendix_md: Option<&str>,
) -> DetailRender {
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
            let body_offset = lines.len();
            // Build the source we render: body, then optional appendix
            // (currently used for the auto-generated Backlinks section).
            // Concatenating before render keeps Actionable line indices
            // valid against the combined output without per-section
            // bookkeeping.
            let render_input = match appendix_md {
                Some(extra) => format!("{}{}", fragment.body, extra),
                None => fragment.body.clone(),
            };
            let rendered = if search_input.is_some() {
                markdown::render_body_highlighted(&render_input, &search_terms)
            } else {
                markdown::render_body(&render_input)
            };
            lines.extend(rendered.lines);
            DetailRender {
                lines,
                items: rendered.items,
                body_offset,
            }
        }
        Err(err) => DetailRender {
            lines: vec![Line::from(Span::styled(
                format!("Failed to load fragment: {}", err),
                Style::default().fg(Color::Red),
            ))],
            items: Vec::new(),
            body_offset: 0,
        },
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

fn draw_launcher_popup(
    frame: &mut Frame,
    area: Rect,
    vault: &Path,
    config: &Config,
    cache: &mut FragmentCache,
    popup: &mut LauncherPopup,
    status: &str,
) {
    popup.clamp_selection();
    let popup_area = centered_rect(92, 86, area);
    frame.render_widget(Clear, popup_area);

    let title = match popup.kind() {
        LauncherKind::Universal => " launcher ",
        LauncherKind::Commands => " commands ",
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(ACTIVE_TAB))
        .title(title);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if inner.width < 24 || inner.height < 5 {
        let paragraph = Paragraph::new("Launcher needs a larger terminal.")
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

    draw_launcher_query(frame, chunks[0], &popup.input);

    let body = chunks[1];
    let left_width = (body.width / 2).max(32).min(body.width.saturating_sub(24));
    let panes =
        Layout::horizontal([Constraint::Length(left_width), Constraint::Min(1)]).split(body);
    draw_launcher_results(frame, panes[0], popup, config);
    draw_launcher_detail(frame, panes[1], vault, cache, popup);
    draw_launcher_footer(frame, chunks[2], popup, status);
}

fn draw_launcher_query(frame: &mut Frame, area: Rect, search_input: &str) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(MENU_BORDER))
        .title(" query ");
    let line = Line::from(vec![
        Span::styled("input ", Style::default().fg(ACTIVE_TAB)),
        Span::raw(search_input.to_string()),
        Span::styled("_", Style::default().fg(ACTIVE_TAB)),
    ]);
    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_launcher_results(
    frame: &mut Frame,
    area: Rect,
    popup: &mut LauncherPopup,
    config: &Config,
) {
    let total = match popup.kind() {
        LauncherKind::Universal => popup.item_count(),
        LauncherKind::Commands => popup.commands.len(),
    };
    let title_label = match popup.kind() {
        LauncherKind::Universal => "results",
        LauncherKind::Commands => "commands",
    };
    let title = if total == 0 {
        format!(" {} ", title_label)
    } else {
        let cur = popup.list_state.selected().map(|i| i + 1).unwrap_or(0);
        format!(" {}  {}/{} ", title_label, cur, total)
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

    let items: Vec<ListItem> = match popup.kind() {
        LauncherKind::Universal => universal_result_items(popup, config),
        LauncherKind::Commands => command_result_items(popup),
    };

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    frame.render_stateful_widget(list, area, &mut popup.list_state);
}

fn universal_result_items(popup: &LauncherPopup, config: &Config) -> Vec<ListItem<'static>> {
    if popup.items.is_empty() && popup.input.trim().is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "Type to find fragments, commands, and views",
            Style::default().fg(MUTED_TEXT),
        )))]
    } else if let Some(err) = popup.error.as_ref().filter(|_| popup.items.is_empty()) {
        vec![ListItem::new(Line::from(Span::styled(
            err.clone(),
            Style::default().fg(Color::Red),
        )))]
    } else if popup.items.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No results",
            Style::default().fg(MUTED_TEXT),
        )))]
    } else {
        let search_terms = parsed_search_terms(&popup.input);
        popup
            .items
            .iter()
            .map(|item| {
                ListItem::new(format_launcher_item(
                    item,
                    config.id_display_length,
                    &search_terms,
                ))
            })
            .collect()
    }
}

fn command_result_items(popup: &LauncherPopup) -> Vec<ListItem<'static>> {
    if popup.commands.is_empty() {
        return vec![ListItem::new(Line::from(Span::styled(
            "No commands",
            Style::default().fg(MUTED_TEXT),
        )))];
    }

    popup
        .commands
        .iter()
        .map(|command| ListItem::new(format_command(*command)))
        .collect()
}

fn format_command(command: CommandEntry) -> Line<'static> {
    Line::from(vec![
        Span::raw(command.label.to_string()),
        Span::styled(
            format!("  {}", command.key),
            Style::default().fg(MUTED_TEXT),
        ),
    ])
}

fn format_launcher_item(
    item: &LauncherItem,
    id_len: usize,
    search_terms: &[String],
) -> Line<'static> {
    match item {
        LauncherItem::Command(command) => {
            let mut line = vec![Span::styled(
                "› ",
                Style::default().fg(ACTIVE_TAB).add_modifier(Modifier::BOLD),
            )];
            line.extend(format_command(*command).spans);
            Line::from(line)
        }
        LauncherItem::Intent(intent) => Line::from(vec![
            Span::styled(
                "↯ ",
                Style::default()
                    .fg(if intent.valid { ACTIVE_TAB } else { Color::Red })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(intent.label.clone()),
        ]),
        LauncherItem::Fragment(row) => {
            let mut line = vec![Span::styled("• ", Style::default().fg(MUTED_TEXT))];
            line.extend(format_row(row, id_len, search_terms).spans);
            Line::from(line)
        }
    }
}

fn draw_launcher_detail(
    frame: &mut Frame,
    area: Rect,
    vault: &Path,
    cache: &mut FragmentCache,
    popup: &mut LauncherPopup,
) {
    match popup.selected_item() {
        Some(LauncherItem::Fragment(_)) => draw_search_preview(frame, area, vault, cache, popup),
        Some(LauncherItem::Command(_)) => draw_command_preview(frame, area, popup),
        Some(LauncherItem::Intent(intent)) => draw_intent_preview(frame, area, intent),
        None => match popup.kind() {
            LauncherKind::Universal => draw_search_preview(frame, area, vault, cache, popup),
            LauncherKind::Commands => draw_command_preview(frame, area, popup),
        },
    }
}

fn draw_search_preview(
    frame: &mut Frame,
    area: Rect,
    vault: &Path,
    cache: &mut FragmentCache,
    popup: &mut LauncherPopup,
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

    let selected_row = popup.selected_row().cloned();
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

    // Launcher preview discards items and never appends backlinks —
    // actions only fire on the main detail pane.
    let detail = detail_lines(vault, cache, &row, Some(&popup.input), None);
    render_detail_lines(
        frame,
        area,
        block,
        detail.lines,
        &mut popup.detail_scroll,
        &mut popup.detail_max_scroll,
    );
}

fn draw_command_preview(frame: &mut Frame, area: Rect, popup: &mut LauncherPopup) {
    let border_color = if popup.focus == Focus::Detail {
        DETAIL_BORDER_FOCUSED
    } else {
        DETAIL_BORDER
    };
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(border_color))
        .title(" command ");

    let Some(command) = popup.selected_command() else {
        let paragraph = Paragraph::new(Line::from(Span::styled(
            "No command selected",
            Style::default().fg(MUTED_TEXT),
        )))
        .block(block);
        frame.render_widget(paragraph, area);
        popup.detail_max_scroll = 0;
        return;
    };

    let mut lines = vec![
        Line::from(Span::styled(
            command.label,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(command.description),
        Line::from(""),
        Line::from(Span::styled(
            format!("Key: {}", command.key),
            Style::default().fg(MUTED_TEXT),
        )),
    ];
    if !command.aliases.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("Search: {}", command.aliases.join(", ")),
            Style::default().fg(MUTED_TEXT),
        )));
    }
    if command.requires_selection {
        lines.push(Line::from(Span::styled(
            "Requires selected fragment",
            Style::default().fg(MUTED_TEXT),
        )));
    }

    render_detail_lines(
        frame,
        area,
        block,
        lines,
        &mut popup.detail_scroll,
        &mut popup.detail_max_scroll,
    );
}

fn draw_intent_preview(frame: &mut Frame, area: Rect, intent: &LauncherIntent) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(if intent.valid {
            DETAIL_BORDER
        } else {
            Color::Red
        }))
        .title(" action ");

    let mut lines = vec![
        Line::from(Span::styled(
            intent.label.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(intent.description.clone()),
    ];
    if let Some(detail) = &intent.detail {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            detail.clone(),
            Style::default().fg(if intent.valid { MUTED_TEXT } else { Color::Red }),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        if intent.valid {
            "Enter runs this against the selected fragment."
        } else {
            "Enter will not run until the value is valid."
        },
        Style::default().fg(MUTED_TEXT),
    )));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_launcher_footer(frame: &mut Frame, area: Rect, popup: &LauncherPopup, status: &str) {
    let text = if let Some(err) = &popup.error {
        Line::from(Span::styled(err.clone(), Style::default().fg(Color::Red)))
    } else if !status.is_empty() {
        Line::from(Span::styled(
            status.to_string(),
            Style::default().fg(ACTIVE_TAB),
        ))
    } else {
        let help = match popup.kind() {
            LauncherKind::Universal => {
                "type fragments/commands/views  > command-only  Enter open/run  Esc close"
            }
            LauncherKind::Commands => "type command  Backspace to search  Enter run  Esc close",
        };
        Line::from(Span::styled(help, Style::default().fg(MUTED_TEXT)))
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

fn draw_footer(frame: &mut Frame, area: Rect, status: &str, app: &App) {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(FOOTER_BORDER))
        .title(" keys ");
    let context = footer_context(app);
    let context_style = Style::default().fg(Color::White);
    let muted = Style::default().fg(MUTED_TEXT);
    let status_line = if status.is_empty() {
        Line::from(Span::styled(context, context_style))
    } else {
        Line::from(vec![
            Span::styled(status.to_string(), Style::default().fg(ACTIVE_TAB)),
            Span::styled("  ", muted),
            Span::styled(context, muted),
        ])
    };
    let command_line = Line::from(Span::styled(footer_commands(app.focus), muted));
    let paragraph = Paragraph::new(vec![status_line, command_line]).block(block);
    frame.render_widget(paragraph, area);
}

fn footer_context(app: &App) -> String {
    let focus_label = match app.focus {
        Focus::List => "[list]",
        Focus::Detail => "[detail]",
    };
    match app.rows.len() {
        0 => format!("{}  0  {}", app.tab.title(), focus_label),
        total => {
            let cur = app.list_state.selected().map(|idx| idx + 1).unwrap_or(0);
            format!("{}  {}/{}  {}", app.tab.title(), cur, total, focus_label)
        }
    }
}

fn footer_commands(focus: Focus) -> &'static str {
    match focus {
        Focus::List => {
            "/ launcher  Ctrl-P commands  Tab tabs  S-tab detail  arrows move  c capture  e edit  ? help"
        }
        Focus::Detail => {
            "/ launcher  Ctrl-P commands  Tab tabs  S-tab list  arrows scroll  f follow  x toggle  e edit  ? help"
        }
    }
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Navigation"),
        Line::from("  1-6         switch tab (Today/List/Stale/Due/Review/Search)"),
        Line::from("  Tab         cycle tabs"),
        Line::from("  Shift-Tab   toggle pane focus (list / detail)"),
        Line::from("  /           open universal launcher"),
        Line::from("  Ctrl-P      open launcher command-only"),
        Line::from("  ↓ / ↑       move within focused pane"),
        Line::from("  PgDn/PgUp   page within focused pane"),
        Line::from("  Home / End  top / bottom of focused pane"),
        Line::from("  Ctrl-d/u    half-page scroll"),
        Line::from(""),
        Line::from("Actions on selected fragment"),
        Line::from("  c           capture a new fragment"),
        Line::from("  e           edit in $EDITOR"),
        Line::from("  t           toggle todo status (open ↔ done)"),
        Line::from("  s           set status"),
        Line::from("  D / P       set due date / priority"),
        Line::from("  @ / #       set assignee / tags"),
        Line::from("  p           promote to another type"),
        Line::from("  a           archive (toggle)"),
        Line::from("  d           delete (with confirm)"),
        Line::from("  y           copy full id to clipboard"),
        Line::from("  r           reload current view"),
        Line::from(""),
        Line::from("In the detail pane (Shift-Tab to focus)"),
        Line::from("  f           follow a [[wiki-link]] (overlay picks one)"),
        Line::from("  x           toggle a [ ] / [x] checkbox"),
        Line::from("  Ctrl-o      jump back through follow history"),
        Line::from("  Ctrl-i      jump forward through follow history"),
        Line::from(""),
        Line::from("General"),
        Line::from("  ?           toggle this help"),
        Line::from("  Launcher    plain text finds fragments, commands, and views"),
        Line::from("  Actions     status done, due friday, priority high, assignee alice"),
        Line::from("  Search      type DSL text, Enter opens result, Esc closes"),
        Line::from("  Commands    type a command or prefix > for command-only"),
        Line::from("  q           quit"),
        Line::from("  Esc         cancel modal / close launcher"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bl(id: &str, ty: &str, title: &str) -> BacklinkInfo {
        BacklinkInfo {
            source_id: id.into(),
            source_type: ty.into(),
            source_title: title.into(),
        }
    }

    #[test]
    fn format_backlinks_emits_wiki_link_per_entry() {
        let out = format_backlinks(&[
            bl("01ABC", "todo", "Pay invoice"),
            bl("02DEF", "note", "Sketch idea"),
        ]);
        assert!(out.starts_with("\n\n## Backlinks\n\n"));
        assert!(out.contains("- [[01ABC|Pay invoice (todo)]]"));
        assert!(out.contains("- [[02DEF|Sketch idea (note)]]"));
    }

    #[test]
    fn format_backlinks_strips_brackets_and_pipes_from_title() {
        let out = format_backlinks(&[bl("01XYZ", "decision", "Use [a|b] split")]);
        // Neither character should appear in the alias portion.
        assert!(out.contains("- [[01XYZ|Use ab split (decision)]]"));
    }

    #[test]
    fn format_backlinks_handles_empty_title() {
        let out = format_backlinks(&[bl("01XYZ", "note", "   ")]);
        assert!(out.contains("- [[01XYZ|(note)]]"));
    }

    #[test]
    fn format_row_highlights_exact_search_terms_in_title() {
        let row = Row {
            id: "01ABCDEF".into(),
            title: "TUI launcher".into(),
            fragment_type: "note".into(),
            status: None,
            priority: None,
            due: None,
            assignee: None,
            tags: Vec::new(),
            updated_at: "2026-05-03T00:00:00Z".into(),
            section: None,
            title_match_indices: Vec::new(),
            score: 0,
        };

        let line = format_row(&row, 8, &[String::from("tui")]);
        assert!(line.spans.iter().any(|span| {
            span.content.as_ref() == "TUI" && span.style.fg == Some(Color::Yellow)
        }));
    }
}
