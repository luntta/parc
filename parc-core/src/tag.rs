use comrak::nodes::NodeValue;
use comrak::{parse_document, Arena, Options};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

static HASHTAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#([a-zA-Z][a-zA-Z0-9_-]*)").unwrap());

/// Extract hashtags from Markdown body, ignoring code blocks and inline code.
pub fn extract_inline_tags(markdown: &str) -> Vec<String> {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &Options::default());

    let mut tags = HashSet::new();

    fn walk_node<'a>(
        node: &'a comrak::nodes::AstNode<'a>,
        tags: &mut HashSet<String>,
        in_link: bool,
    ) {
        let data = node.data.borrow();
        match &data.value {
            // Skip code blocks and inline code
            NodeValue::CodeBlock(_) | NodeValue::Code(_) => return,
            // Track when we're inside a link (to skip URL fragments)
            NodeValue::Link(_) => {
                for child in node.children() {
                    walk_node(child, tags, true);
                }
                return;
            }
            NodeValue::Text(text) => {
                if !in_link {
                    for cap in HASHTAG_RE.captures_iter(text) {
                        tags.insert(cap[1].to_lowercase());
                    }
                }
            }
            _ => {}
        }

        for child in node.children() {
            walk_node(child, tags, in_link);
        }
    }

    walk_node(root, &mut tags, false);

    let mut result: Vec<String> = tags.into_iter().collect();
    result.sort();
    result
}

/// Merge frontmatter tags and inline tags, deduplicated, case-insensitive.
pub fn merge_tags(frontmatter_tags: &[String], inline_tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    // Frontmatter tags come first
    for tag in frontmatter_tags {
        let lower = tag.to_lowercase();
        if seen.insert(lower) {
            result.push(tag.to_lowercase());
        }
    }

    // Then inline tags
    for tag in inline_tags {
        let lower = tag.to_lowercase();
        if seen.insert(lower.clone()) {
            result.push(lower);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_basic() {
        let tags = extract_inline_tags("This has #backend and #frontend tags.");
        assert!(tags.contains(&"backend".to_string()));
        assert!(tags.contains(&"frontend".to_string()));
    }

    #[test]
    fn test_ignore_code_block() {
        let md = "Normal #visible text\n\n```\n#ignored in code\n```\n";
        let tags = extract_inline_tags(md);
        assert!(tags.contains(&"visible".to_string()));
        assert!(!tags.contains(&"ignored".to_string()));
    }

    #[test]
    fn test_ignore_inline_code() {
        let tags = extract_inline_tags("Normal #visible and `#ignored` code.");
        assert!(tags.contains(&"visible".to_string()));
        assert!(!tags.contains(&"ignored".to_string()));
    }

    #[test]
    fn test_ignore_pure_numeric() {
        let tags = extract_inline_tags("Issue #123 and #valid-tag");
        assert!(!tags.contains(&"123".to_string()));
        assert!(tags.contains(&"valid-tag".to_string()));
    }

    #[test]
    fn test_case_insensitive_dedup() {
        let tags = extract_inline_tags("Both #Backend and #backend here.");
        assert_eq!(tags, vec!["backend".to_string()]);
    }

    #[test]
    fn test_merge_tags() {
        let frontmatter = vec!["backend".to_string(), "Search".to_string()];
        let inline = vec!["backend".to_string(), "newone".to_string()];
        let merged = merge_tags(&frontmatter, &inline);
        assert_eq!(merged, vec!["backend", "search", "newone"]);
    }

    #[test]
    fn test_ignore_link_fragment() {
        let tags = extract_inline_tags("Click [here](#section) for more. And #real-tag.");
        assert!(!tags.contains(&"section".to_string()));
        assert!(tags.contains(&"real-tag".to_string()));
    }
}
