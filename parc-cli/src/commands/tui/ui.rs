use std::io::{self, Write};
use std::path::Path;

use anyhow::Result;
use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{
    self, BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate,
};
use parc_core::config::Config;
use parc_core::fragment;

use super::{Row, Tab};

const MENU_BORDER: Color = Color::DarkCyan;
const LIST_BORDER: Color = Color::DarkBlue;
const DETAIL_BORDER: Color = Color::DarkGreen;
const FOOTER_BORDER: Color = Color::DarkGrey;
const ACTIVE_TAB: Color = Color::Yellow;
const MUTED_TEXT: Color = Color::DarkGrey;

#[derive(Clone, Copy)]
struct Rect {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
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
    // Begin a synchronized update so terminals that support DEC mode 2026
    // commit the whole frame at once instead of revealing it line-by-line.
    // On terminals that don't, this is just an inert escape sequence.
    queue!(stdout, BeginSynchronizedUpdate)?;
    if width < 48 || height < 10 {
        queue!(
            stdout,
            Clear(ClearType::All),
            MoveTo(0, 0),
            SetForegroundColor(MENU_BORDER),
            Print("parc"),
            ResetColor,
            MoveTo(0, 1),
            Print("Terminal is too small for the TUI.")
        )?;
        queue!(stdout, EndSynchronizedUpdate)?;
        stdout.flush()?;
        return Ok(());
    }

    let menu_height = if tab == Tab::Search { 5 } else { 3 };
    let footer_height = 3;
    let body_height = height.saturating_sub(menu_height + footer_height);
    let left_width = (width / 2).max(34).min(width.saturating_sub(24));
    let right_width = width.saturating_sub(left_width);
    let menu_rect = Rect {
        x: 0,
        y: 0,
        width,
        height: menu_height,
    };
    let list_rect = Rect {
        x: 0,
        y: menu_height,
        width: left_width,
        height: body_height,
    };
    let detail_rect = Rect {
        x: left_width,
        y: menu_height,
        width: right_width,
        height: body_height,
    };
    let footer_rect = Rect {
        x: 0,
        y: height.saturating_sub(footer_height),
        width,
        height: footer_height,
    };

    queue!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
    draw_box(stdout, menu_rect, " parc ", MENU_BORDER)?;
    draw_tabs(stdout, menu_rect, tab)?;

    if tab == Tab::Search {
        let search_rect = Rect {
            x: 2,
            y: 2,
            width: width.saturating_sub(4),
            height: 3,
        };
        draw_box(stdout, search_rect, " search ", MENU_BORDER)?;
        queue!(
            stdout,
            MoveTo(search_rect.x + 2, search_rect.y + 1),
            SetForegroundColor(ACTIVE_TAB),
            Print("/"),
            ResetColor,
            Print(truncate(
                search_input,
                search_rect.width.saturating_sub(5) as usize
            ))
        )?;
    }

    draw_box(
        stdout,
        list_rect,
        &format!(" {} ", tab.title()),
        LIST_BORDER,
    )?;
    draw_box(stdout, detail_rect, " detail ", DETAIL_BORDER)?;

    let visible_rows = list_rect.height.saturating_sub(2) as usize;
    for (idx, row) in rows.iter().take(visible_rows).enumerate() {
        let y = list_rect.y + 1 + idx as u16;
        queue!(stdout, MoveTo(list_rect.x + 1, y))?;
        if idx == selected {
            queue!(stdout, SetAttribute(Attribute::Reverse))?;
        }
        let section = row.section.as_deref().unwrap_or("");
        let short = short_id(&row.id, config.id_display_length);
        let status = row.status.as_deref().unwrap_or("-");
        let line = if section.is_empty() {
            format!(
                "{}  {:<8} {:<10} {}",
                short, row.fragment_type, status, row.title
            )
        } else {
            format!(
                "{}  {:<8} {:<10} {} - {}",
                short, row.fragment_type, status, section, row.title
            )
        };
        queue!(
            stdout,
            Print(truncate(&line, list_rect.width.saturating_sub(2) as usize))
        )?;
        if idx == selected {
            queue!(stdout, SetAttribute(Attribute::Reset))?;
        }
    }

    draw_detail(stdout, vault, rows.get(selected), detail_rect)?;

    let footer = if status.is_empty() {
        "tab/1-4 tabs  j/k move  / search  r reload  q quit".to_string()
    } else {
        format!("{}  -  tab/1-4 tabs  j/k move  / search  q quit", status)
    };
    draw_box(stdout, footer_rect, " keys ", FOOTER_BORDER)?;
    queue!(
        stdout,
        MoveTo(footer_rect.x + 2, footer_rect.y + 1),
        SetForegroundColor(MUTED_TEXT),
        Print(truncate(
            &footer,
            footer_rect.width.saturating_sub(4) as usize
        )),
        ResetColor
    )?;

    queue!(stdout, EndSynchronizedUpdate)?;
    stdout.flush()?;
    Ok(())
}

fn draw_tabs(stdout: &mut io::Stdout, rect: Rect, active: Tab) -> Result<()> {
    queue!(stdout, MoveTo(rect.x + 2, rect.y + 1))?;
    for tab in [Tab::Today, Tab::List, Tab::Stale, Tab::Search] {
        if tab == active {
            queue!(
                stdout,
                SetForegroundColor(ACTIVE_TAB),
                SetAttribute(Attribute::Reverse)
            )?;
        } else {
            queue!(stdout, SetForegroundColor(MUTED_TEXT))?;
        }
        queue!(stdout, Print(format!(" {} ", tab.title())))?;
        if tab == active {
            queue!(stdout, SetAttribute(Attribute::Reset))?;
        }
        queue!(stdout, ResetColor)?;
        queue!(stdout, Print(" "))?;
    }
    Ok(())
}

fn draw_detail(stdout: &mut io::Stdout, vault: &Path, row: Option<&Row>, rect: Rect) -> Result<()> {
    let Some(row) = row else {
        queue!(
            stdout,
            MoveTo(rect.x + 2, rect.y + 1),
            SetForegroundColor(MUTED_TEXT),
            Print("No selection"),
            ResetColor
        )?;
        return Ok(());
    };

    let fragment = fragment::read_fragment(vault, &row.id)?;
    let detail_width = rect.width.saturating_sub(4) as usize;
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

    for (idx, line) in wrap_lines(lines, detail_width)
        .into_iter()
        .take(rect.height.saturating_sub(2) as usize)
        .enumerate()
    {
        queue!(
            stdout,
            MoveTo(rect.x + 2, rect.y + 1 + idx as u16),
            Print(line)
        )?;
    }

    Ok(())
}

fn draw_box(stdout: &mut io::Stdout, rect: Rect, title: &str, color: Color) -> Result<()> {
    if rect.width < 2 || rect.height < 2 {
        return Ok(());
    }

    let inner_width = rect.width.saturating_sub(2) as usize;
    let top = titled_border(title, inner_width);
    let bottom = format!("\u{2514}{}\u{2518}", "\u{2500}".repeat(inner_width));

    queue!(
        stdout,
        SetForegroundColor(color),
        MoveTo(rect.x, rect.y),
        Print(top)
    )?;

    for y in rect.y + 1..rect.y + rect.height.saturating_sub(1) {
        queue!(
            stdout,
            MoveTo(rect.x, y),
            Print("\u{2502}"),
            MoveTo(rect.x + rect.width.saturating_sub(1), y),
            Print("\u{2502}")
        )?;
    }

    queue!(
        stdout,
        MoveTo(rect.x, rect.y + rect.height.saturating_sub(1)),
        Print(bottom),
        ResetColor
    )?;

    Ok(())
}

fn titled_border(title: &str, inner_width: usize) -> String {
    if inner_width == 0 {
        return "\u{250C}\u{2510}".to_string();
    }

    let clean = truncate(title, inner_width);
    let remaining = inner_width.saturating_sub(clean.len());
    format!(
        "\u{250C}{}{}\u{2510}",
        clean,
        "\u{2500}".repeat(remaining)
    )
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

fn wrap_lines(lines: Vec<String>, max_width: usize) -> Vec<String> {
    lines
        .into_iter()
        .flat_map(|line| wrap_line(&line, max_width))
        .collect()
}

fn wrap_line(line: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![String::new()];
    }
    if line.trim().is_empty() {
        return vec![String::new()];
    }

    let mut wrapped = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in line.split_whitespace() {
        let word_width = word.chars().count();
        if word_width > max_width {
            if !current.is_empty() {
                wrapped.push(current);
                current = String::new();
                current_width = 0;
            }
            wrapped.extend(chunk_word(word, max_width));
            continue;
        }

        if current.is_empty() {
            current.push_str(word);
            current_width = word_width;
        } else if current_width + 1 + word_width <= max_width {
            current.push(' ');
            current.push_str(word);
            current_width += 1 + word_width;
        } else {
            wrapped.push(current);
            current = word.to_string();
            current_width = word_width;
        }
    }

    if !current.is_empty() {
        wrapped.push(current);
    }
    wrapped
}

fn chunk_word(word: &str, max_width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for c in word.chars() {
        if current.chars().count() == max_width {
            chunks.push(current);
            current = String::new();
        }
        current.push(c);
    }

    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::wrap_line;

    #[test]
    fn wraps_detail_line_on_word_boundaries() {
        assert_eq!(
            wrap_line("alpha beta gamma delta", 12),
            vec!["alpha beta".to_string(), "gamma delta".to_string()]
        );
    }

    #[test]
    fn wraps_detail_line_with_long_words() {
        assert_eq!(
            wrap_line("alpha betagammadelta z", 8),
            vec![
                "alpha".to_string(),
                "betagamm".to_string(),
                "adelta".to_string(),
                "z".to_string()
            ]
        );
    }
}
