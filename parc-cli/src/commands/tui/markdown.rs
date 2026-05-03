use std::ops::Range;
use std::sync::LazyLock;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena, Options};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;

use super::highlight;

const HEADING_COLOR: Color = Color::Cyan;
const CODE_COLOR: Color = Color::Yellow;
const INLINE_CODE_COLOR: Color = Color::Magenta;
const LINK_COLOR: Color = Color::Cyan;
const QUOTE_COLOR: Color = Color::DarkGray;
const RULE_COLOR: Color = Color::DarkGray;

static WIKI_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]\|]+?)(?:\|([^\]]+?))?\]\]").unwrap());

static FENCED_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```[^\n]*\n.*?```").unwrap());

static INLINE_CODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]+`").unwrap());

static CHECKBOX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*[-*+][ \t]+(\[[ xX]\])").unwrap());

/// Output of rendering a fragment body to ratatui lines, with positional
/// metadata for in-detail actions (link follow, checkbox toggle).
#[derive(Debug, Clone)]
pub(super) struct RenderedBody {
    pub lines: Vec<Line<'static>>,
    pub items: Vec<Actionable>,
}

/// Something the user can act on inside the rendered detail body.
#[derive(Debug, Clone)]
pub(super) struct Actionable {
    pub kind: ActionKind,
    /// Index into `RenderedBody.lines` where this item is rendered.
    pub logical_line: usize,
    /// Byte range in the source body string. For checkboxes this covers the
    /// `[ ]`/`[x]` literal only, so a toggle is a 3-byte rewrite.
    pub source_range: Range<usize>,
}

#[derive(Debug, Clone)]
pub(super) enum ActionKind {
    WikiLink {
        target: String,
        // Reserved for future overlay hints / yank-link by content.
        #[allow(dead_code)]
        display_text: Option<String>,
    },
    Checkbox {
        // Source-of-truth for the checkbox is the byte range in the body —
        // this is just informational so a future overlay can preview the
        // toggle direction.
        #[allow(dead_code)]
        checked: bool,
    },
}

pub(super) fn render_body(body: &str) -> RenderedBody {
    render_body_inner(body, &[])
}

pub(super) fn render_body_highlighted(body: &str, search_terms: &[String]) -> RenderedBody {
    render_body_inner(body, search_terms)
}

#[derive(Debug)]
struct SourceWikiLink {
    range: Range<usize>,
    target: String,
    display_text: Option<String>,
}

#[derive(Debug)]
struct SourceCheckbox {
    range: Range<usize>,
    checked: bool,
}

struct RenderCtx {
    wiki_links: std::vec::IntoIter<SourceWikiLink>,
    checkboxes: std::vec::IntoIter<SourceCheckbox>,
    items: Vec<Actionable>,
}

fn render_body_inner(body: &str, search_terms: &[String]) -> RenderedBody {
    let arena = Arena::new();
    let root = parse_document(&arena, body, &Options::default());
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut ctx = RenderCtx {
        wiki_links: scan_wiki_links(body).into_iter(),
        checkboxes: scan_checkboxes(body).into_iter(),
        items: Vec::new(),
    };
    for child in root.children() {
        render_block(child, &mut lines, 0, search_terms, &mut ctx);
    }
    while matches!(lines.last(), Some(line) if line_is_empty(line)) {
        lines.pop();
    }
    RenderedBody {
        lines,
        items: ctx.items,
    }
}

fn line_is_empty(line: &Line<'_>) -> bool {
    line.spans.iter().all(|s| s.content.trim().is_empty())
}

fn scan_wiki_links(body: &str) -> Vec<SourceWikiLink> {
    let code_ranges = find_code_ranges(body);
    let mut out = Vec::new();
    for cap in WIKI_LINK_RE.captures_iter(body) {
        let m = cap.get(0).unwrap();
        if code_ranges.iter().any(|r| r.contains(&m.start())) {
            continue;
        }
        let target = cap[1].trim().to_string();
        if target.is_empty() {
            continue;
        }
        let display_text = cap.get(2).map(|m| m.as_str().trim().to_string());
        out.push(SourceWikiLink {
            range: m.start()..m.end(),
            target,
            display_text,
        });
    }
    out
}

fn scan_checkboxes(body: &str) -> Vec<SourceCheckbox> {
    let code_ranges = find_code_ranges(body);
    let mut out = Vec::new();
    for cap in CHECKBOX_RE.captures_iter(body) {
        let bracket = cap.get(1).unwrap();
        if code_ranges.iter().any(|r| r.contains(&bracket.start())) {
            continue;
        }
        let inner = bracket.as_str().as_bytes()[1];
        let checked = inner == b'x' || inner == b'X';
        out.push(SourceCheckbox {
            range: bracket.start()..bracket.end(),
            checked,
        });
    }
    out
}

fn find_code_ranges(body: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    for m in FENCED_CODE_RE.find_iter(body) {
        ranges.push(m.start()..m.end());
    }
    for m in INLINE_CODE_RE.find_iter(body) {
        ranges.push(m.start()..m.end());
    }
    ranges
}

fn render_block<'a>(
    node: &'a AstNode<'a>,
    out: &mut Vec<Line<'static>>,
    indent: usize,
    search_terms: &[String],
    ctx: &mut RenderCtx,
) {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Document => {
            for child in node.children() {
                render_block(child, out, indent, search_terms, ctx);
            }
        }
        NodeValue::Paragraph => {
            let pending_line = out.len();
            let spans = inline_spans(node, Style::default(), search_terms, ctx, pending_line);
            push_with_indent(out, spans, indent);
            out.push(Line::raw(""));
        }
        NodeValue::Heading(h) => {
            let style = Style::default()
                .fg(HEADING_COLOR)
                .add_modifier(Modifier::BOLD);
            let pending_line = out.len();
            let mut spans = vec![Span::styled(
                format!("{} ", "#".repeat(h.level as usize)),
                style,
            )];
            spans.extend(inline_spans(node, style, search_terms, ctx, pending_line));
            push_with_indent(out, spans, indent);
            out.push(Line::raw(""));
        }
        NodeValue::List(_) => {
            for item in node.children() {
                render_list_item(item, out, indent, search_terms, ctx);
            }
            if indent == 0 {
                out.push(Line::raw(""));
            }
        }
        NodeValue::Item(_) => {
            render_list_item(node, out, indent, search_terms, ctx);
        }
        NodeValue::CodeBlock(b) => {
            let style = Style::default().fg(CODE_COLOR);
            for line in b.literal.lines() {
                let mut spans = Vec::new();
                if indent > 0 {
                    spans.push(Span::raw(" ".repeat(indent)));
                }
                spans.extend(highlight::spans_for_text(line, style, &[], search_terms));
                out.push(Line::from(spans));
            }
            out.push(Line::raw(""));
        }
        NodeValue::BlockQuote => {
            let items_before = ctx.items.len();
            let mut nested: Vec<Line<'static>> = Vec::new();
            for child in node.children() {
                render_block(child, &mut nested, 0, search_terms, ctx);
            }
            let nested_offset = out.len();
            for line in nested {
                let mut spans = vec![Span::styled("│ ", Style::default().fg(QUOTE_COLOR))];
                spans.extend(line.spans);
                out.push(Line::from(spans));
            }
            // Rebase actionables emitted inside the blockquote: their
            // logical_line was relative to the nested buffer.
            for item in &mut ctx.items[items_before..] {
                item.logical_line += nested_offset;
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
                out.push(Line::from(highlight::spans_for_text(
                    line,
                    Style::default(),
                    &[],
                    search_terms,
                )));
            }
            out.push(Line::raw(""));
        }
        _ => {
            for child in node.children() {
                render_block(child, out, indent, search_terms, ctx);
            }
        }
    }
}

fn render_list_item<'a>(
    node: &'a AstNode<'a>,
    out: &mut Vec<Line<'static>>,
    indent: usize,
    search_terms: &[String],
    ctx: &mut RenderCtx,
) {
    let mut first = true;
    for child in node.children() {
        let data = child.data.borrow();
        match &data.value {
            NodeValue::Paragraph => {
                let pending_line = out.len();
                if first {
                    consume_checkbox_for_item(child, ctx, pending_line);
                }
                let spans = inline_spans(child, Style::default(), search_terms, ctx, pending_line);
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
                    render_list_item(nested, out, indent + 2, search_terms, ctx);
                }
            }
            _ => {
                render_block(child, out, indent + 2, search_terms, ctx);
            }
        }
    }
}

/// If the first text run of this list item begins with `[ ]`/`[x]`, consume
/// the next pre-scanned checkbox from the queue and emit an Actionable.
fn consume_checkbox_for_item<'a>(
    paragraph: &'a AstNode<'a>,
    ctx: &mut RenderCtx,
    pending_line: usize,
) {
    let leading = first_text_prefix(paragraph);
    let trimmed = leading.trim_start();
    if !is_checkbox_prefix(trimmed) {
        return;
    }
    let Some(src) = ctx.checkboxes.next() else {
        return;
    };
    ctx.items.push(Actionable {
        kind: ActionKind::Checkbox {
            checked: src.checked,
        },
        logical_line: pending_line,
        source_range: src.range,
    });
}

fn first_text_prefix<'a>(node: &'a AstNode<'a>) -> String {
    for child in node.children() {
        let data = child.data.borrow();
        if let NodeValue::Text(text) = &data.value {
            return text.to_string();
        }
    }
    String::new()
}

fn is_checkbox_prefix(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 3
        && bytes[0] == b'['
        && (bytes[1] == b' ' || bytes[1] == b'x' || bytes[1] == b'X')
        && bytes[2] == b']'
}

fn inline_spans<'a>(
    node: &'a AstNode<'a>,
    base: Style,
    search_terms: &[String],
    ctx: &mut RenderCtx,
    pending_line: usize,
) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    for child in node.children() {
        collect_inline(child, base, search_terms, ctx, pending_line, &mut out);
    }
    out
}

fn collect_inline<'a>(
    node: &'a AstNode<'a>,
    style: Style,
    search_terms: &[String],
    ctx: &mut RenderCtx,
    pending_line: usize,
    out: &mut Vec<Span<'static>>,
) {
    let data = node.data.borrow();
    match &data.value {
        NodeValue::Text(text) => {
            push_text_with_wikilinks(text, style, search_terms, ctx, pending_line, out);
        }
        NodeValue::Strong => {
            let s = style.add_modifier(Modifier::BOLD);
            for child in node.children() {
                collect_inline(child, s, search_terms, ctx, pending_line, out);
            }
        }
        NodeValue::Emph => {
            let s = style.add_modifier(Modifier::ITALIC);
            for child in node.children() {
                collect_inline(child, s, search_terms, ctx, pending_line, out);
            }
        }
        NodeValue::Strikethrough => {
            let s = style.add_modifier(Modifier::CROSSED_OUT);
            for child in node.children() {
                collect_inline(child, s, search_terms, ctx, pending_line, out);
            }
        }
        NodeValue::Code(c) => {
            out.extend(highlight::spans_for_text(
                &c.literal,
                Style::default().fg(INLINE_CODE_COLOR),
                &[],
                search_terms,
            ));
        }
        NodeValue::Link(_) => {
            let s = Style::default()
                .fg(LINK_COLOR)
                .add_modifier(Modifier::UNDERLINED);
            for child in node.children() {
                collect_inline(child, s, search_terms, ctx, pending_line, out);
            }
        }
        NodeValue::SoftBreak | NodeValue::LineBreak => {
            out.push(Span::raw(" "));
        }
        _ => {
            for child in node.children() {
                collect_inline(child, style, search_terms, ctx, pending_line, out);
            }
        }
    }
}

/// Splits a Text node by `[[...]]` wiki-link occurrences and emits styled spans.
/// Each emitted wiki-link span also produces an `Actionable::WikiLink` entry
/// using the next pre-scanned source range from `ctx.wiki_links`.
fn push_text_with_wikilinks(
    text: &str,
    style: Style,
    search_terms: &[String],
    ctx: &mut RenderCtx,
    pending_line: usize,
    out: &mut Vec<Span<'static>>,
) {
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
                        out.extend(highlight::spans_for_text(
                            &text[last..i],
                            style,
                            &[],
                            search_terms,
                        ));
                    }
                    out.extend(highlight::spans_for_text(
                        &text[i..j + 2],
                        link_style,
                        &[],
                        search_terms,
                    ));
                    if let Some(src) = ctx.wiki_links.next() {
                        ctx.items.push(Actionable {
                            kind: ActionKind::WikiLink {
                                target: src.target,
                                display_text: src.display_text,
                            },
                            logical_line: pending_line,
                            source_range: src.range,
                        });
                    }
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
        out.extend(highlight::spans_for_text(
            &text[last..],
            style,
            &[],
            search_terms,
        ));
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
    use super::{render_body, render_body_highlighted, ActionKind};
    use ratatui::style::Modifier;

    #[test]
    fn renders_heading_and_paragraph() {
        let rendered = render_body("# Hello\n\nworld");
        assert!(rendered
            .lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("Hello"))));
        assert!(rendered
            .lines
            .iter()
            .any(|l| l.spans.iter().any(|s| s.content.contains("world"))));
    }

    #[test]
    fn renders_wiki_link_as_distinct_span() {
        let rendered = render_body("see [[01ABCD]] for details");
        let spans: Vec<&str> = rendered
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(spans.iter().any(|s| *s == "[[01ABCD]]"));
    }

    #[test]
    fn renders_bullet_list() {
        let rendered = render_body("- one\n- two\n");
        let joined: String = rendered
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("");
        assert!(joined.contains("• one"));
        assert!(joined.contains("• two"));
    }

    #[test]
    fn highlights_search_terms_in_body_text() {
        let rendered = render_body_highlighted("hello world", &[String::from("world")]);
        let spans: Vec<&str> = rendered
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(spans.iter().any(|span| *span == "world"));
    }

    #[test]
    fn highlights_search_terms_in_inline_and_block_code() {
        let rendered = render_body_highlighted(
            "Use `TUI` here\n\n```\ntui block\n```",
            &[String::from("tui")],
        );
        let highlighted: Vec<&str> = rendered
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter(|span| span.style.add_modifier.contains(Modifier::BOLD))
            .map(|span| span.content.as_ref())
            .collect();

        assert!(highlighted.contains(&"TUI"));
        assert!(highlighted.contains(&"tui"));
    }

    #[test]
    fn records_wiki_link_actionable() {
        let body = "see [[01ABCD]] and [[XYZ|alias]] for context";
        let rendered = render_body(body);
        assert_eq!(rendered.items.len(), 2);
        match &rendered.items[0].kind {
            ActionKind::WikiLink {
                target,
                display_text,
            } => {
                assert_eq!(target, "01ABCD");
                assert!(display_text.is_none());
            }
            other => panic!("expected WikiLink, got {:?}", other),
        }
        match &rendered.items[1].kind {
            ActionKind::WikiLink {
                target,
                display_text,
            } => {
                assert_eq!(target, "XYZ");
                assert_eq!(display_text.as_deref(), Some("alias"));
            }
            other => panic!("expected WikiLink, got {:?}", other),
        }
        // source ranges should round-trip
        assert_eq!(&body[rendered.items[0].source_range.clone()], "[[01ABCD]]");
        assert_eq!(
            &body[rendered.items[1].source_range.clone()],
            "[[XYZ|alias]]"
        );
        // both on the same logical line (single paragraph)
        assert_eq!(rendered.items[0].logical_line, 0);
        assert_eq!(rendered.items[1].logical_line, 0);
    }

    #[test]
    fn ignores_wiki_links_in_code() {
        let body = "see `[[ignored]]` and ```\n[[also-ignored]]\n``` and [[real]]";
        let rendered = render_body(body);
        assert_eq!(rendered.items.len(), 1);
        match &rendered.items[0].kind {
            ActionKind::WikiLink { target, .. } => assert_eq!(target, "real"),
            other => panic!("expected WikiLink, got {:?}", other),
        }
    }

    #[test]
    fn records_checkbox_actionables() {
        let body = "- [ ] open\n- [x] done\n- plain bullet\n";
        let rendered = render_body(body);
        let checkboxes: Vec<_> = rendered
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ActionKind::Checkbox { checked } => Some((*checked, item.source_range.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(checkboxes.len(), 2);
        assert_eq!(checkboxes[0].0, false);
        assert_eq!(&body[checkboxes[0].1.clone()], "[ ]");
        assert_eq!(checkboxes[1].0, true);
        assert_eq!(&body[checkboxes[1].1.clone()], "[x]");
    }

    #[test]
    fn ignores_checkboxes_in_code_blocks() {
        let body = "```\n- [ ] code\n```\n\n- [ ] real\n";
        let rendered = render_body(body);
        let count = rendered
            .items
            .iter()
            .filter(|item| matches!(item.kind, ActionKind::Checkbox { .. }))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn wiki_link_with_utf8_before_it() {
        let body = "café [[id]]";
        let rendered = render_body(body);
        assert_eq!(rendered.items.len(), 1);
        // source range should be byte-correct despite multi-byte char
        assert_eq!(&body[rendered.items[0].source_range.clone()], "[[id]]");
    }
}
