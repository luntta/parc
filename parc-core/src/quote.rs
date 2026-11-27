//! Quote syntax and text normalization shared across core, CLI, TUI, recall,
//! Agent, and desktop viewer.
//!
//! Quotes in a fragment's body anchor a reader's comments to a passage in an
//! attached document:
//!
//! - `> p. 12 · "quoted passage"`
//! - `> slide 3 · "quoted passage"`
//! - `> § Heading · "quoted passage"`
//! - Multi-document sources prefix the filename:
//!   `> report.pdf p. 12 · "quoted passage"`
//!
//! Because pdf.js and xberg produce slightly different typography (ligatures,
//! curly vs straight quotes, soft/line-end hyphens, whitespace), [`normalize`]
//! folds text into one canonical form so quotes can always be located again.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

/// The kind of location anchor a quote carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum QuoteAnchor {
    Page { number: usize },
    Slide { number: usize },
    Section { heading: String },
}

/// A parsed quote from a fragment body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    /// 1-based line number of the quote line in the fragment body.
    pub line: usize,
    /// Filename prefix when a source has multiple attachments, e.g. `report.pdf`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub anchor: QuoteAnchor,
    /// Verbatim text inside the quotes.
    pub text: String,
}

/// Where a quote was found inside a document's extracted parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteLocation {
    /// 1-based part number (page, slide, or section).
    pub part: usize,
    /// Character offset in the original part string where the match starts.
    pub offset: usize,
    /// Character length in the original part string of the matched span.
    pub length: usize,
}

/// Parse all document quotes from a Markdown fragment body, ignoring fenced
/// code blocks.
pub fn parse_quotes(body: &str) -> Vec<Quote> {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches:
    //   > [filename ]p. 12 · "quote"
    //   > [filename ]slide 3 · "quote"
    //   > [filename ]§ Heading · "quote"
    // Accepts both middle dot `·` and ASCII `-` / `|` as separator, and straight
    // or curly double quotes around the passage.
    let re = RE.get_or_init(|| {
        Regex::new(
            r#"^\s*>\s*(?:(?P<file>[^\s/\\]+?\.[A-Za-z0-9]{1,8})\s+)?(?:(?P<page>p\.\s*(?P<pnum>\d+))|(?P<slide>slide\s+(?P<snum>\d+))|(?P<sec>§\s*(?P<heading>[^·\n]+?)))\s*·\s*["“](?P<quote>.+?)["”]\s*$"#,
        )
        .unwrap()
    });

    let mut quotes = Vec::new();
    let mut in_fenced = false;
    let body = body.strip_prefix('\n').unwrap_or(body);

    for (idx, raw_line) in body.split('\n').enumerate() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fenced = !in_fenced;
            continue;
        }
        if in_fenced {
            continue;
        }
        if let Some(caps) = re.captures(line) {
            let filename = caps.name("file").map(|m| m.as_str().to_string());
            let anchor = if let Some(n) = caps.name("pnum") {
                let Ok(number) = n.as_str().parse::<usize>() else {
                    continue;
                };
                if number == 0 {
                    continue;
                }
                QuoteAnchor::Page { number }
            } else if let Some(n) = caps.name("snum") {
                let Ok(number) = n.as_str().parse::<usize>() else {
                    continue;
                };
                if number == 0 {
                    continue;
                }
                QuoteAnchor::Slide { number }
            } else if let Some(h) = caps.name("heading") {
                let heading = h.as_str().trim().to_string();
                if heading.is_empty() {
                    continue;
                }
                QuoteAnchor::Section { heading }
            } else {
                continue;
            };
            let text = caps["quote"].to_string();
            quotes.push(Quote {
                line: idx + 1,
                filename,
                anchor,
                text,
            });
        }
    }
    quotes
}

/// Format a quote line for insertion into Notes.
pub fn format_quote(filename: Option<&str>, anchor: &QuoteAnchor, text: &str) -> String {
    let prefix = filename.map(|f| format!("{f} ")).unwrap_or_default();
    let loc = match anchor {
        QuoteAnchor::Page { number } => format!("p. {number}"),
        QuoteAnchor::Slide { number } => format!("slide {number}"),
        QuoteAnchor::Section { heading } => format!("§ {heading}"),
    };
    let clean = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'");
    format!("> {prefix}{loc} · \"{clean}\"")
}

/// Normalize text for quote matching across xberg and pdf.js:
/// - NFKC (expands ligatures such as ﬁ, ﬂ, ﬀ, ﬃ, ﬄ, ﬅ, ﬆ and compatibility forms)
/// - Curly quotes to straight (`‘’‚‛` -> `'`, `“”„‟` -> `"`)
/// - Unicode dashes and minus (`‐‑‒–—―−`) to ASCII `-`
/// - Soft hyphens (`\u{00AD}`) removed
/// - Line-end hyphenation (`word-\nword`) rejoined
/// - Runs of whitespace collapsed to a single ASCII space and trimmed
///
/// The renderer keeps an identical copy tested against the same fixtures.
pub fn normalize(input: &str) -> String {
    normalize_with_map(input).0
}

/// Normalize `input`, also returning for each character of the normalized
/// output the 0-based character index in `input` it came from.
pub fn normalize_with_map(input: &str) -> (String, Vec<usize>) {
    let orig: Vec<char> = input.chars().collect();
    let n = orig.len();

    // Pass 1: rejoin line-end hyphens (`<alnum><dash>[spaces]\r?\n[spaces]<alnum>`)
    // and drop soft hyphens, tracking original char index of each surviving char.
    let mut joined: Vec<(char, usize)> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        let c = orig[i];
        if c == '\u{00AD}' {
            i += 1;
            continue;
        }
        if is_dash(c) && joined.last().is_some_and(|(prev, _)| prev.is_alphanumeric()) {
            // Look ahead for optional horizontal spaces, then a newline, then optional horizontal spaces, then an alphanumeric char.
            let mut j = i + 1;
            while j < n && (orig[j] == ' ' || orig[j] == '\t') {
                j += 1;
            }
            if j < n && (orig[j] == '\n' || orig[j] == '\r') {
                if orig[j] == '\r' && j + 1 < n && orig[j + 1] == '\n' {
                    j += 2;
                } else {
                    j += 1;
                }
                while j < n && (orig[j] == ' ' || orig[j] == '\t') {
                    j += 1;
                }
                if j < n && orig[j].is_alphanumeric() {
                    // Skip the hyphen and line break; continue from the next word fragment.
                    i = j;
                    continue;
                }
            }
        }
        joined.push((c, i));
        i += 1;
    }

    // Pass 2: NFKC + quote/dash folding + whitespace collapsing.
    let mut out = String::with_capacity(joined.len());
    let mut map: Vec<usize> = Vec::with_capacity(joined.len());
    let mut prev_space = true; // trims leading whitespace

    for (raw_ch, orig_idx) in joined {
        let s = raw_ch.to_string();
        for ch in s.nfkc() {
            if ch == '\u{00AD}' {
                continue;
            }
            let folded = fold_char(ch);
            if folded.is_whitespace() {
                if !prev_space {
                    out.push(' ');
                    map.push(orig_idx);
                    prev_space = true;
                }
            } else {
                out.push(folded);
                map.push(orig_idx);
                prev_space = false;
            }
        }
    }

    // Trim trailing space
    while out.ends_with(' ') {
        out.pop();
        map.pop();
    }

    (out, map)
}

fn is_dash(c: char) -> bool {
    matches!(
        c,
        '-' | '\u{2010}'
            | '\u{2011}'
            | '\u{2012}'
            | '\u{2013}'
            | '\u{2014}'
            | '\u{2015}'
            | '\u{2212}'
    )
}

fn fold_char(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        c if is_dash(c) => '-',
        other => other,
    }
}

/// Find `quote` inside `parts`, checking `part_hint` (1-based) first, then
/// adjacent parts outward from the hint, then all remaining parts.
pub fn locate(parts: &[String], quote: &str, part_hint: Option<usize>) -> Option<QuoteLocation> {
    let needle = normalize(quote);
    if needle.is_empty() || parts.is_empty() {
        return None;
    }
    let needle_lower = needle.to_lowercase();

    // Order of 0-based part indices to search.
    let order = search_order(parts.len(), part_hint);

    // Exact normalized match first.
    for &idx in &order {
        if let Some((offset, length)) = find_in_part(&parts[idx], &needle, false) {
            return Some(QuoteLocation {
                part: idx + 1,
                offset,
                length,
            });
        }
    }
    // Case-insensitive fallback.
    for &idx in &order {
        if let Some((offset, length)) = find_in_part(&parts[idx], &needle_lower, true) {
            return Some(QuoteLocation {
                part: idx + 1,
                offset,
                length,
            });
        }
    }
    None
}

fn search_order(len: usize, hint_1based: Option<usize>) -> Vec<usize> {
    let mut order = Vec::with_capacity(len);
    let mut seen = vec![false; len];
    if let Some(hint) = hint_1based.and_then(|h| h.checked_sub(1)).filter(|&h| h < len) {
        order.push(hint);
        seen[hint] = true;
        for delta in 1..len {
            if let Some(prev) = hint.checked_sub(delta) {
                if !seen[prev] {
                    order.push(prev);
                    seen[prev] = true;
                }
            }
            let next = hint + delta;
            if next < len && !seen[next] {
                order.push(next);
                seen[next] = true;
            }
        }
    }
    for (idx, visited) in seen.into_iter().enumerate() {
        if !visited {
            order.push(idx);
        }
    }
    order
}

fn find_in_part(part: &str, needle: &str, case_insensitive: bool) -> Option<(usize, usize)> {
    let (norm, map) = normalize_with_map(part);
    if norm.is_empty() {
        return None;
    }
    if !case_insensitive {
        let byte_pos = norm.find(needle)?;
        let char_start = norm[..byte_pos].chars().count();
        let char_len = needle.chars().count();
        let orig_start = map[char_start];
        let orig_end = map[char_start + char_len - 1] + 1;
        return Some((orig_start, orig_end.saturating_sub(orig_start)));
    }
    // Case-insensitive match preserving char mapping.
    let norm_chars: Vec<char> = norm.chars().collect();
    let mut lower = String::with_capacity(norm.len());
    let mut lower_to_orig: Vec<usize> = Vec::with_capacity(norm_chars.len());
    for (i, ch) in norm_chars.into_iter().enumerate() {
        for lc in ch.to_lowercase() {
            lower.push(lc);
            lower_to_orig.push(map[i]);
        }
    }
    let byte_pos = lower.find(needle)?;
    let char_start = lower[..byte_pos].chars().count();
    let char_len = needle.chars().count();
    let orig_start = lower_to_orig[char_start];
    let orig_end = lower_to_orig[char_start + char_len - 1] + 1;
    Some((orig_start, orig_end.saturating_sub(orig_start)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_matches_shared_fixtures() {
        // Shared JSON fixture also verified by parc-desktop/test/quote.test.cjs.
        #[derive(Deserialize)]
        struct Case {
            input: String,
            expected: String,
        }
        let raw = include_str!("../tests/fixtures/quote_normalize.json");
        let cases: Vec<Case> = serde_json::from_str(raw).unwrap();
        assert!(!cases.is_empty());
        for c in cases {
            assert_eq!(normalize(&c.input), c.expected, "input: {:?}", c.input);
        }
    }

    #[test]
    fn parse_quotes_supports_page_slide_section_and_multi_file() {
        let body = r#"
Intro note.

> p. 12 · "The ferry timetable changed in autumn."
Comment on page 12.

> slide 3 · "Revenue grew 14%"
Comment on slide 3.

> § Key Findings · "Latency dropped below 20 ms."
Comment on section.

> report.pdf p. 4 · "Appendix table"

```markdown
> p. 99 · "Ignored inside code fence"
```
"#;
        let quotes = parse_quotes(body);
        assert_eq!(quotes.len(), 4);
        assert_eq!(quotes[0].anchor, QuoteAnchor::Page { number: 12 });
        assert_eq!(quotes[0].filename, None);
        assert_eq!(quotes[0].text, "The ferry timetable changed in autumn.");

        assert_eq!(quotes[1].anchor, QuoteAnchor::Slide { number: 3 });
        assert_eq!(quotes[1].text, "Revenue grew 14%");

        assert_eq!(
            quotes[2].anchor,
            QuoteAnchor::Section {
                heading: "Key Findings".into()
            }
        );
        assert_eq!(quotes[2].text, "Latency dropped below 20 ms.");

        assert_eq!(quotes[3].filename.as_deref(), Some("report.pdf"));
        assert_eq!(quotes[3].anchor, QuoteAnchor::Page { number: 4 });
    }

    #[test]
    fn locate_finds_quote_across_normalization_and_moved_pages() {
        let parts = vec![
            "Page one text.".to_string(),
            "The “ﬁnancial” re-\nport — updated.".to_string(),
            "The “financial” report - updated.".to_string(),
        ];
        // Hint prefers page 2 over page 3 when both match
        let hit = locate(&parts, "The \"financial\" report - updated.", Some(2)).unwrap();
        assert_eq!(hit.part, 2);
        // Moved page: hint says 1, finds on 2
        let hit2 = locate(&parts, "The \"financial\" report - updated.", Some(1)).unwrap();
        assert_eq!(hit2.part, 2);
        // Hint says 3, finds on 3
        let hit3 = locate(&parts, "The \"financial\" report - updated.", Some(3)).unwrap();
        assert_eq!(hit3.part, 3);
    }
}
