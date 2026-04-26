use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena, Options};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const HEADING_COLOR: Color = Color::Cyan;
const CODE_COLOR: Color = Color::Yellow;
const INLINE_CODE_COLOR: Color = Color::Magenta;
const LINK_COLOR: Color = Color::Cyan;
const QUOTE_COLOR: Color = Color::DarkGray;
const RULE_COLOR: Color = Color::DarkGray;

pub(super) fn render_body(body: &str) -> Vec<Line<'static>> {
    let arena = Arena::new();
    let root = parse_document(&arena, body, &Options::default());
    let mut lines: Vec<Line<'static>> = Vec::new();
    for child in root.children() {
        render_block(child, &mut lines, 0);
    }
    while matches!(lines.last(), Some(line) if line_is_empty(line)) {
        lines.pop();
    }
    lines
}

fn line_is_empty(line: &Line<'_>) -> bool {
    line.spans.iter().all(|s| s.content.trim().is_empty())
}

fn render_block<'a>(node: &'a AstNode<'a>, out: &mut Vec<Line<'static>>, indent: usize) {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Document => {
            for child in node.children() {
                render_block(child, out, indent);
            }
        }
        NodeValue::Paragraph => {
            let spans = inline_spans(node, Style::default());
            push_with_indent(out, spans, indent);
            out.push(Line::raw(""));
        }
        NodeValue::Heading(h) => {
            let style = Style::default()
                .fg(HEADING_COLOR)
                .add_modifier(Modifier::BOLD);
            let mut spans = vec![Span::styled(
                format!("{} ", "#".repeat(h.level as usize)),
                style,
            )];
            spans.extend(inline_spans(node, style));
            push_with_indent(out, spans, indent);
            out.push(Line::raw(""));
        }
        NodeValue::List(_) => {
            for item in node.children() {
                render_list_item(item, out, indent);
            }
            if indent == 0 {
                out.push(Line::raw(""));
            }
        }
        NodeValue::Item(_) => {
            render_list_item(node, out, indent);
        }
        NodeValue::CodeBlock(b) => {
            let style = Style::default().fg(CODE_COLOR);
            for line in b.literal.lines() {
                let mut spans = Vec::new();
                if indent > 0 {
                    spans.push(Span::raw(" ".repeat(indent)));
                }
                spans.push(Span::styled(line.to_string(), style));
                out.push(Line::from(spans));
            }
            out.push(Line::raw(""));
        }
        NodeValue::BlockQuote => {
            let mut nested: Vec<Line<'static>> = Vec::new();
            for child in node.children() {
                render_block(child, &mut nested, 0);
            }
            for line in nested {
                let mut spans = vec![Span::styled("│ ", Style::default().fg(QUOTE_COLOR))];
                spans.extend(line.spans);
                out.push(Line::from(spans));
            }
        }
        NodeValue::ThematicBreak => {
            out.push(Line::from(Span::styled(
                "─".repeat(40),
                Style::default().fg(RULE_COLOR),
            )));
            out.push(Line::raw(""));
        }
        NodeValue::HtmlBlock(b) => {
            for line in b.literal.lines() {
                out.push(Line::raw(line.to_string()));
            }
            out.push(Line::raw(""));
        }
        _ => {
            for child in node.children() {
                render_block(child, out, indent);
            }
        }
    }
}

fn render_list_item<'a>(node: &'a AstNode<'a>, out: &mut Vec<Line<'static>>, indent: usize) {
    let mut first = true;
    for child in node.children() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::Paragraph => {
                let spans = inline_spans(child, Style::default());
                let mut line_spans: Vec<Span<'static>> = Vec::new();
                if indent > 0 {
                    line_spans.push(Span::raw(" ".repeat(indent)));
                }
                if first {
                    line_spans.push(Span::styled("• ", Style::default().fg(LINK_COLOR)));
                    first = false;
                } else {
                    line_spans.push(Span::raw("  "));
                }
                line_spans.extend(spans);
                out.push(Line::from(line_spans));
            }
            NodeValue::List(_) => {
                for nested in child.children() {
                    render_list_item(nested, out, indent + 2);
                }
            }
            _ => {
                render_block(child, out, indent + 2);
            }
        }
    }
}

fn inline_spans<'a>(node: &'a AstNode<'a>, base: Style) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    for child in node.children() {
        collect_inline(child, base, &mut out);
    }
    out
}

fn collect_inline<'a>(node: &'a AstNode<'a>, style: Style, out: &mut Vec<Span<'static>>) {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Text(text) => {
            push_text_with_wikilinks(text, style, out);
        }
        NodeValue::Strong => {
            let s = style.add_modifier(Modifier::BOLD);
            for child in node.children() {
                collect_inline(child, s, out);
            }
        }
        NodeValue::Emph => {
            let s = style.add_modifier(Modifier::ITALIC);
            for child in node.children() {
                collect_inline(child, s, out);
            }
        }
        NodeValue::Strikethrough => {
            let s = style.add_modifier(Modifier::CROSSED_OUT);
            for child in node.children() {
                collect_inline(child, s, out);
            }
        }
        NodeValue::Code(c) => {
            out.push(Span::styled(
                c.literal.clone(),
                Style::default().fg(INLINE_CODE_COLOR),
            ));
        }
        NodeValue::Link(_) => {
            let s = Style::default()
                .fg(LINK_COLOR)
                .add_modifier(Modifier::UNDERLINED);
            for child in node.children() {
                collect_inline(child, s, out);
            }
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => {
            out.push(Span::raw(" "));
        }
        _ => {
            for child in node.children() {
                collect_inline(child, style, out);
            }
        }
    }
}

/// Splits a Text node by `[[...]]` wiki-link occurrences and emits styled spans.
fn push_text_with_wikilinks(text: &str, style: Style, out: &mut Vec<Span<'static>>) {
    let link_style = Style::default()
        .fg(LINK_COLOR)
        .add_modifier(Modifier::UNDERLINED);
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut last = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            let mut found = false;
            while j + 1 < bytes.len() {
                if bytes[j] == b']' && bytes[j + 1] == b']' {
                    if i > last {
                        out.push(Span::styled(text[last..i].to_string(), style));
                    }
                    out.push(Span::styled(text[i..j + 2].to_string(), link_style));
                    last = j + 2;
                    i = last;
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                break;
            }
            continue;
        }
        i += 1;
    }
    if last < text.len() {
        out.push(Span::styled(text[last..].to_string(), style));
    }
}

fn push_with_indent(out: &mut Vec<Line<'static>>, spans: Vec<Span<'static>>, indent: usize) {
    if indent == 0 {
        out.push(Line::from(spans));
    } else {
        let mut padded = vec![Span::raw(" ".repeat(indent))];
        padded.extend(spans);
        out.push(Line::from(padded));
    }
}

#[cfg(test)]
mod tests {
    use super::render_body;

    #[test]
    fn renders_heading_and_paragraph() {
        let lines = render_body("# Hello\n\nworld");
        assert!(lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("Hello"))));
        assert!(lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("world"))));
    }

    #[test]
    fn renders_wiki_link_as_distinct_span() {
        let lines = render_body("see [[01ABCD]] for details");
        let spans: Vec<&str> = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(spans.iter().any(|s| *s == "[[01ABCD]]"));
    }

    #[test]
    fn renders_bullet_list() {
        let lines = render_body("- one\n- two\n");
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
        assert!(joined.contains("• one"));
        assert!(joined.contains("• two"));
    }
}
