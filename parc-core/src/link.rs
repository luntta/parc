use regex::Regex;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::LazyLock;

static WIKI_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]\|]+?)(?:\|([^\]]+?))?\]\]").unwrap());

static FENCED_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```[^\n]*\n.*?```").unwrap());

static INLINE_CODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`[^`]+`").unwrap());

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    pub target: String,
    pub display_text: Option<String>,
}

/// Parse wiki-links from Markdown body, ignoring code blocks and inline code.
/// Supports `[[id-prefix]]` and `[[id-prefix|display text]]`.
pub fn parse_wiki_links(body: &str) -> Vec<WikiLink> {
    let code_ranges = find_code_ranges(body);

    let mut seen = HashSet::new();
    let mut links = Vec::new();

    for cap in WIKI_LINK_RE.captures_iter(body) {
        let m = cap.get(0).unwrap();
        if code_ranges.iter().any(|r| r.contains(&m.start())) {
            continue;
        }

        let target = cap[1].trim().to_string();
        if target.is_empty() {
            continue;
        }
        if seen.insert(target.to_uppercase()) {
            let display_text = cap.get(2).map(|m| m.as_str().trim().to_string());
            links.push(WikiLink {
                target,
                display_text,
            });
        }
    }

    links
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

/// Merge frontmatter links with wiki-links extracted from the body.
/// Frontmatter links come first. Deduplicates by uppercase comparison.
/// `resolve_fn` maps a target string to an optional full ID.
pub fn merge_links<F>(frontmatter_links: &[String], body_links: &[WikiLink], resolve_fn: F) -> Vec<String>
where
    F: Fn(&str) -> Option<String>,
{
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for link in frontmatter_links {
        let upper = link.to_uppercase();
        if seen.insert(upper) {
            result.push(link.clone());
        }
    }

    for wl in body_links {
        if let Some(full_id) = resolve_fn(&wl.target) {
            let upper = full_id.to_uppercase();
            if seen.insert(upper) {
                result.push(full_id);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_link() {
        let links = parse_wiki_links("See [[01JQ7V4Y]] for details.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "01JQ7V4Y");
        assert_eq!(links[0].display_text, None);
    }

    #[test]
    fn test_link_with_display_text() {
        let links = parse_wiki_links("See [[01JQ7V4Y|Decision about SQLite]] for details.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "01JQ7V4Y");
        assert_eq!(links[0].display_text, Some("Decision about SQLite".to_string()));
    }

    #[test]
    fn test_multiple_links() {
        let links = parse_wiki_links("Link [[AAA]] and [[BBB|text]] here.");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "AAA");
        assert_eq!(links[1].target, "BBB");
    }

    #[test]
    fn test_ignore_code_block() {
        let md = "Normal [[VISIBLE]] text\n\n```\n[[IGNORED]] in code\n```\n";
        let links = parse_wiki_links(md);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "VISIBLE");
    }

    #[test]
    fn test_ignore_inline_code() {
        let links = parse_wiki_links("Normal [[VISIBLE]] and `[[IGNORED]]` code.");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "VISIBLE");
    }

    #[test]
    fn test_empty_link_ignored() {
        let links = parse_wiki_links("Empty [[]] link.");
        assert_eq!(links.len(), 0);
    }

    #[test]
    fn test_dedup_same_target() {
        let links = parse_wiki_links("Link [[AAA]] and again [[AAA|other text]].");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "AAA");
    }

    #[test]
    fn test_dedup_case_insensitive() {
        let links = parse_wiki_links("Link [[aaa]] and [[AAA]].");
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn test_merge_links() {
        let fm = vec!["FULL_ID_A".to_string(), "FULL_ID_B".to_string()];
        let body = vec![
            WikiLink { target: "PREFIX_C".to_string(), display_text: None },
            WikiLink { target: "PREFIX_A".to_string(), display_text: None },
        ];
        let merged = merge_links(&fm, &body, |prefix| match prefix {
            "PREFIX_C" => Some("FULL_ID_C".to_string()),
            "PREFIX_A" => Some("FULL_ID_A".to_string()),
            _ => None,
        });
        assert_eq!(merged, vec!["FULL_ID_A", "FULL_ID_B", "FULL_ID_C"]);
    }

    #[test]
    fn test_merge_links_unresolvable_skipped() {
        let fm = vec!["FULL_ID_A".to_string()];
        let body = vec![WikiLink { target: "UNKNOWN".to_string(), display_text: None }];
        let merged = merge_links(&fm, &body, |_| None);
        assert_eq!(merged, vec!["FULL_ID_A"]);
    }
}
