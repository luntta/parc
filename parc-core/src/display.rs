//! Human-readable output only. Keep protocol payloads and stored text intact.

/// The first `len` Unicode scalar values, without splitting UTF-8.
pub fn short_id(id: &str, len: usize) -> &str {
    id.char_indices().nth(len).map_or(id, |(end, _)| &id[..end])
}

/// Safe for a multi-line terminal body. No data-controlled terminal commands.
pub fn sanitize_terminal_text(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || !c.is_control())
        .collect()
}

/// A title, table cell, status line or log field must not start another line.
pub fn sanitize_terminal_line(text: &str) -> String {
    text.chars()
        .filter_map(|c| match c {
            '\n' | '\u{2028}' | '\u{2029}' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_prefixes_and_terminal_boundaries() {
        assert_eq!(short_id("abc🎵é", 4), "abc🎵");
        assert_eq!(short_id("é", 0), "");
        assert_eq!(short_id("é", usize::MAX), "é");
        let text = "one\r\n二\u{1b}[31m\u{9b}\u{7}";
        assert_eq!(sanitize_terminal_text(text), "one\n二[31m");
        assert_eq!(sanitize_terminal_line(text), "one 二[31m");
    }
}
