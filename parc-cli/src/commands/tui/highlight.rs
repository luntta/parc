use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

const SEARCH_HIGHLIGHT: Color = Color::Yellow;

pub(super) fn spans_for_text(
    text: &str,
    base_style: Style,
    match_indices: &[u32],
    search_terms: &[String],
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return Vec::new();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut highlighted = vec![false; chars.len()];

    for &idx in match_indices {
        if let Some(slot) = highlighted.get_mut(idx as usize) {
            *slot = true;
        }
    }

    mark_term_matches(&chars, search_terms, &mut highlighted);

    if highlighted.iter().all(|is_highlighted| !*is_highlighted) {
        return vec![Span::styled(text.to_string(), base_style)];
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_highlighted = highlighted[0];

    for (ch, is_highlighted) in chars.into_iter().zip(highlighted) {
        if is_highlighted != current_highlighted && !current.is_empty() {
            spans.push(styled_span(
                std::mem::take(&mut current),
                base_style,
                current_highlighted,
            ));
            current_highlighted = is_highlighted;
        }
        current.push(ch);
    }

    if !current.is_empty() {
        spans.push(styled_span(current, base_style, current_highlighted));
    }

    spans
}

fn mark_term_matches(chars: &[char], search_terms: &[String], highlighted: &mut [bool]) {
    if chars.is_empty() {
        return;
    }

    let haystack: Vec<char> = chars.iter().map(|c| c.to_ascii_lowercase()).collect();
    for term in search_terms {
        let needle: Vec<char> = term.chars().map(|c| c.to_ascii_lowercase()).collect();
        if needle.is_empty() || needle.len() > haystack.len() {
            continue;
        }

        for start in 0..=haystack.len() - needle.len() {
            if haystack[start..start + needle.len()] == needle {
                for slot in &mut highlighted[start..start + needle.len()] {
                    *slot = true;
                }
            }
        }
    }
}

fn styled_span(text: String, base_style: Style, highlighted: bool) -> Span<'static> {
    if highlighted {
        Span::styled(
            text,
            base_style.fg(SEARCH_HIGHLIGHT).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(text, base_style)
    }
}

#[cfg(test)]
mod tests {
    use super::spans_for_text;
    use ratatui::style::Style;

    #[test]
    fn splits_exact_search_terms_into_highlighted_spans() {
        let spans = spans_for_text(
            "hello world",
            Style::default(),
            &[],
            &[String::from("world")],
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1].content.as_ref(), "world");
    }

    #[test]
    fn highlights_character_indices() {
        let spans = spans_for_text("fileserver", Style::default(), &[0, 2, 4], &[]);
        let highlighted: String = spans
            .iter()
            .filter(|span| span.style.fg.is_some())
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(highlighted, "fls");
    }
}
