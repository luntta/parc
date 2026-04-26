use std::io::{self, Read};
use std::path::Path;

use anyhow::{bail, Result};

const SHORT_TITLE_LIMIT: usize = 120;

pub fn run(
    vault: &Path,
    text: Vec<String>,
    tags: Vec<String>,
    links: Vec<String>,
    json: bool,
) -> Result<()> {
    let raw = if text.is_empty() {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        input
    } else {
        text.join(" ")
    };

    let input = raw.trim_end_matches(['\r', '\n']);
    if input.trim().is_empty() {
        bail!("capture text is empty");
    }

    let (title, body) = split_capture_text(input);

    crate::commands::new::run(
        vault,
        "note",
        Some(title),
        Some(body),
        tags,
        links,
        None,
        None,
        None,
        None,
        json,
    )
}

pub(crate) fn split_capture_text(input: &str) -> (String, String) {
    if !input.contains('\n') && input.chars().count() <= SHORT_TITLE_LIMIT {
        return (input.trim().to_string(), String::new());
    }

    if let Some((first, rest)) = input.split_once('\n') {
        (
            first.trim_end_matches('\r').trim().to_string(),
            rest.to_string(),
        )
    } else {
        (input.trim().to_string(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_single_line_is_title_only() {
        let (title, body) = split_capture_text("Look into connection pooling");

        assert_eq!(title, "Look into connection pooling");
        assert_eq!(body, "");
    }

    #[test]
    fn multiline_uses_first_line_as_title() {
        let (title, body) = split_capture_text("Scratch note\nLine one\nLine two");

        assert_eq!(title, "Scratch note");
        assert_eq!(body, "Line one\nLine two");
    }
}
