use std::sync::Arc;

use nucleo::pattern::{CaseMatching, Normalization};
use nucleo::{Config, Matcher, Nucleo, Status, Utf32String};

/// A candidate fragment to be ranked by the fuzzy matcher. Matching runs
/// against `title` and `body` concatenated; only character indices that fall
/// within the title are returned in [`FuzzyHit::title_match_indices`].
#[derive(Clone, Debug)]
pub struct FuzzyItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub fragment_type: String,
    pub status: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl FuzzyItem {
    fn haystack(&self) -> String {
        if self.body.is_empty() {
            self.title.clone()
        } else {
            format!("{}\n{}", self.title, self.body)
        }
    }

    fn title_char_count(&self) -> u32 {
        self.title.chars().count() as u32
    }
}

#[derive(Clone, Debug)]
pub struct FuzzyHit {
    pub item: FuzzyItem,
    pub score: u32,
    pub title_match_indices: Vec<u32>,
}

/// High-level wrapper around [`nucleo::Nucleo`] with a single concatenated
/// haystack column. Caller drives the lifecycle: `set_candidates` →
/// `set_pattern` → tick (poll or block) → `hits`.
pub struct FuzzyEngine {
    nuc: Nucleo<FuzzyItem>,
    matcher: Matcher,
}

impl FuzzyEngine {
    pub fn new() -> Self {
        Self::with_notify(Arc::new(|| {}))
    }

    pub fn with_notify(notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        let nuc = Nucleo::new(Config::DEFAULT, notify, None, 1);
        Self {
            nuc,
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    pub fn set_candidates(&mut self, items: Vec<FuzzyItem>) {
        self.nuc.restart(true);
        let injector = self.nuc.injector();
        for item in items {
            injector.push(item, |i, cols| {
                cols[0] = Utf32String::from(i.haystack());
            });
        }
    }

    pub fn set_pattern(&mut self, pattern: &str) {
        self.nuc.pattern.reparse(
            0,
            pattern,
            CaseMatching::Smart,
            Normalization::Smart,
            false,
        );
    }

    pub fn tick(&mut self, timeout_ms: u64) -> Status {
        self.nuc.tick(timeout_ms)
    }

    /// Tick repeatedly until the worker reports it is no longer running.
    /// Intended for synchronous (CLI) callers.
    pub fn poll_until_done(&mut self) -> Status {
        let mut last = self.nuc.tick(10);
        while last.running {
            last = self.nuc.tick(50);
        }
        last
    }

    pub fn matched_count(&self) -> u32 {
        self.nuc.snapshot().matched_item_count()
    }

    pub fn injected_count(&self) -> u32 {
        self.nuc.snapshot().item_count()
    }

    pub fn hits(&mut self, limit: usize) -> Vec<FuzzyHit> {
        let snap = self.nuc.snapshot();
        let total = snap.matched_item_count() as usize;
        let take = total.min(limit);
        if take == 0 {
            return Vec::new();
        }

        let pattern = snap.pattern().column_pattern(0);
        let mut indices_buf: Vec<u32> = Vec::new();
        let mut out = Vec::with_capacity(take);

        for item in snap.matched_items(0..take as u32) {
            let data = item.data.clone();
            let title_chars = data.title_char_count();
            indices_buf.clear();
            let score = pattern
                .indices(item.matcher_columns[0].slice(..), &mut self.matcher, &mut indices_buf)
                .unwrap_or(0);
            indices_buf.sort_unstable();
            indices_buf.dedup();
            let title_match_indices: Vec<u32> = indices_buf
                .iter()
                .copied()
                .take_while(|&i| i < title_chars)
                .collect();
            out.push(FuzzyHit {
                item: data,
                score,
                title_match_indices,
            });
        }
        out
    }
}

impl Default for FuzzyEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, title: &str, body: &str) -> FuzzyItem {
        FuzzyItem {
            id: id.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            fragment_type: "note".to_string(),
            status: None,
            tags: Vec::new(),
            created_at: "2026-04-26T00:00:00Z".to_string(),
            updated_at: "2026-04-26T00:00:00Z".to_string(),
        }
    }

    fn run(engine: &mut FuzzyEngine) {
        engine.poll_until_done();
    }

    #[test]
    fn empty_pattern_returns_all_in_order() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![
            item("1", "alpha", ""),
            item("2", "beta", ""),
            item("3", "gamma", ""),
        ]);
        run(&mut e);
        let hits = e.hits(10);
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].item.id, "1");
        assert_eq!(hits[1].item.id, "2");
        assert_eq!(hits[2].item.id, "3");
        for h in &hits {
            assert!(h.title_match_indices.is_empty());
        }
    }

    #[test]
    fn subsequence_match_in_title() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![
            item("a", "fileserver", ""),
            item("b", "unrelated", ""),
            item("c", "filesharer", ""),
        ]);
        e.set_pattern("flsr");
        run(&mut e);
        let hits = e.hits(10);
        let ids: Vec<&str> = hits.iter().map(|h| h.item.id.as_str()).collect();
        assert!(ids.contains(&"a"));
        assert!(ids.contains(&"c"));
        assert!(!ids.contains(&"b"));
        // "fileserver" → f, l, s, r match indices into the title
        let a = hits.iter().find(|h| h.item.id == "a").unwrap();
        assert!(!a.title_match_indices.is_empty());
        for &i in &a.title_match_indices {
            assert!(i < a.item.title.chars().count() as u32);
        }
    }

    #[test]
    fn smart_case_insensitive_when_pattern_lowercase() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![item("u", "UpperCase", ""), item("l", "lowercase", "")]);
        e.set_pattern("upper");
        run(&mut e);
        let hits = e.hits(10);
        assert!(hits.iter().any(|h| h.item.id == "u"));
    }

    #[test]
    fn smart_case_sensitive_when_pattern_has_uppercase() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![item("u", "UpperCase", ""), item("l", "uppercase", "")]);
        e.set_pattern("Upper");
        run(&mut e);
        let hits = e.hits(10);
        let ids: Vec<&str> = hits.iter().map(|h| h.item.id.as_str()).collect();
        assert_eq!(ids, vec!["u"]);
    }

    #[test]
    fn body_text_is_searched_but_indices_are_title_only() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![
            item("t", "rust crate", ""),
            item("b", "unrelated title", "this body contains rust"),
        ]);
        e.set_pattern("rust");
        run(&mut e);
        let hits = e.hits(10);
        let ids: Vec<&str> = hits.iter().map(|h| h.item.id.as_str()).collect();
        assert!(ids.contains(&"t"));
        assert!(ids.contains(&"b"));
        let body_hit = hits.iter().find(|h| h.item.id == "b").unwrap();
        // body matches must not produce title indices
        assert!(body_hit.title_match_indices.is_empty());
        let title_hit = hits.iter().find(|h| h.item.id == "t").unwrap();
        assert!(!title_hit.title_match_indices.is_empty());
    }

    #[test]
    fn pattern_with_no_matches_returns_nothing() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![item("1", "alpha", ""), item("2", "beta", "")]);
        e.set_pattern("zzzqqq");
        run(&mut e);
        assert_eq!(e.hits(10).len(), 0);
    }

    #[test]
    fn higher_score_ranks_first() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![
            item("far", "f_a_r_apart", ""),
            item("near", "far", ""),
        ]);
        e.set_pattern("far");
        run(&mut e);
        let hits = e.hits(10);
        assert_eq!(hits[0].item.id, "near");
    }

    #[test]
    fn limit_caps_returned_hits() {
        let mut e = FuzzyEngine::new();
        e.set_candidates((0..50).map(|i| item(&i.to_string(), "alpha", "")).collect());
        run(&mut e);
        assert_eq!(e.hits(10).len(), 10);
    }

    #[test]
    fn set_candidates_replaces_old_set() {
        let mut e = FuzzyEngine::new();
        e.set_candidates(vec![item("1", "foo", "")]);
        run(&mut e);
        assert_eq!(e.hits(10).len(), 1);

        e.set_candidates(vec![item("2", "bar", ""), item("3", "baz", "")]);
        run(&mut e);
        let hits = e.hits(10);
        let ids: Vec<&str> = hits.iter().map(|h| h.item.id.as_str()).collect();
        assert!(!ids.contains(&"1"));
        assert!(ids.contains(&"2"));
        assert!(ids.contains(&"3"));
    }
}
