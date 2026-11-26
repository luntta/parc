//! Recall benchmark. Every file in `tests/fixtures/recall/*.json` is loaded
//! into one temporary vault, each case asks [`parc_core::recall`] for the
//! fragments related to its seed, and the results are written up as a
//! scorecard. The committed scorecard is the expected output, so a change to
//! retrieval shows up as a diff of it. To accept a new one:
//!
//!     PARC_RECALL_BLESS=1 cargo test -p parc-core --test recall_bench
//!
//! There are two scorecards. `scorecard-lexical.txt` is recall without the
//! meaning model (words, tags and links), checked on every run.
//! `scorecard-meaning.txt` adds the model ([`embed::MODEL`]) and is checked
//! when the model is installed in [`embed::model_dir`]; otherwise that test
//! says it was skipped. `PARC_RECALL_MODEL=<catalog id>` scores another
//! model of [`embed::CATALOG`] into `scorecard-meaning-<id>.txt`.
//!
//! Two totals, never blended: sources found within each case's limit, and
//! cases where nothing is related and nothing was returned.
//!
//! Fixture keys, case names and expected answers stay in the test: a test
//! checks that none of them reach the vault. IDs are real ULIDs whose time is
//! the fragment's `created_at` and whose random part comes from its key, so
//! they are the same on every run.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use parc_core::embed::{self, Model};
use parc_core::hook::ScriptRunner;
use parc_core::ops::Writer;
use parc_core::recall::{self, Candidate, RecallQuery, Seed};
use serde::Deserialize;

/// How far down the results the scorecard looks, so that a source just
/// outside a case's limit shows its rank rather than "not returned".
const DEPTH: usize = 10;

const BLESS_VAR: &str = "PARC_RECALL_BLESS";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/recall")
}

fn scorecard_path(meaning: Option<&Model>) -> PathBuf {
    let name = match meaning {
        None => "scorecard-lexical.txt".to_string(),
        Some(m) if std::ptr::eq(m.spec(), embed::MODEL) || m.spec().id == embed::MODEL.id => {
            "scorecard-meaning.txt".to_string()
        }
        Some(m) => format!("scorecard-meaning-{}.txt", m.spec().id),
    };
    fixtures_dir().join(name)
}

/// The model to score with: [`embed::MODEL`], or the catalog entry
/// `PARC_RECALL_MODEL` names. `None` (with a note) when it isn't installed.
fn installed_model() -> Option<Model> {
    let spec = match std::env::var("PARC_RECALL_MODEL") {
        Ok(id) => embed::CATALOG
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("PARC_RECALL_MODEL: no model {id:?} in the catalog")),
        Err(_) => embed::MODEL,
    };
    let dir = embed::model_dir(spec).expect("no data folder for models");
    if !embed::is_installed(spec, &dir) {
        eprintln!(
            "skipped: the meaning model {} is not installed in {}",
            spec.id,
            dir.display()
        );
        return None;
    }
    Some(Model::load(spec, &dir).unwrap())
}

// ── Fixture files ──────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFile {
    #[allow(dead_code)]
    description: String,
    fragments: Vec<FixtureFragment>,
    cases: Vec<FixtureCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFragment {
    key: String,
    created_at: DateTime<Utc>,
    #[serde(rename = "type")]
    fragment_type: String,
    title: String,
    body: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    name: String,
    seed: String,
    expected_evidence: Vec<String>,
    candidate_limit: Option<usize>,
    /// "abstain" when nothing in the vault is related to the seed.
    expected_result: Option<String>,
    /// For judging a model's reflection later (roadmap: recall step 4).
    useful_inference: Option<String>,
    unsupported_inference: Option<String>,
    /// What the case is for.
    why: Option<String>,
}

// ── The loaded vault ───────────────────────────────────────────────────

struct Loaded {
    id: String,
    created_at: DateTime<Utc>,
    /// [`recall::content_words`] of the title and body, plus the tags.
    words: Vec<String>,
    /// The title and body as one text, and the tags: the fragment as a draft.
    text: String,
    tags: Vec<String>,
    /// Its vectors, when scoring with a model.
    vector: Option<Vec<Vec<f32>>>,
}

struct Case {
    name: String,
    /// Keys are scoped by file: `ferry/capture`.
    seed: String,
    expected: Vec<String>,
    limit: usize,
}

struct Bench<'m> {
    meaning: Option<&'m Model>,
    _tmp: tempfile::TempDir,
    vault: PathBuf,
    by_key: BTreeMap<String, Loaded>,
    key_of: HashMap<String, String>,
    cases: Vec<Case>,
    /// Every string that is a label rather than vault content.
    labels: Vec<String>,
}

fn load() -> Bench<'static> {
    load_with(None)
}

/// The benchmark vault, with every fragment's vector computed when scoring
/// with a model.
fn load_with(meaning: Option<&Model>) -> Bench<'_> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no recall fixtures");

    let mut fragments: Vec<(String, FixtureFragment)> = Vec::new();
    let mut cases = Vec::new();
    let mut labels = Vec::new();
    for path in &files {
        let stem = path.file_stem().unwrap().to_str().unwrap().to_string();
        let file: FixtureFile = serde_json::from_slice(&std::fs::read(path).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let scope = |key: &str| format!("{stem}/{key}");
        for f in file.fragments {
            fragments.push((scope(&f.key), f));
        }
        for c in file.cases {
            let abstain = match c.expected_result.as_deref() {
                None => false,
                Some("abstain") => true,
                Some(other) => panic!("{}: unknown expected_result {other:?}", c.name),
            };
            assert_eq!(
                abstain,
                c.expected_evidence.is_empty(),
                "{}: a case abstains exactly when it expects no evidence",
                c.name
            );
            let limit = c.candidate_limit.unwrap_or(0);
            assert!(
                limit >= c.expected_evidence.len(),
                "{}: candidate_limit is below the number of expected sources",
                c.name
            );
            labels.extend(
                [
                    Some(c.name.clone()),
                    c.useful_inference,
                    c.unsupported_inference,
                    c.why,
                ]
                .into_iter()
                .flatten(),
            );
            cases.push(Case {
                name: c.name,
                seed: scope(&c.seed),
                expected: c.expected_evidence.iter().map(|k| scope(k)).collect(),
                limit,
            });
        }
    }

    let mut by_key = BTreeMap::new();
    for (key, f) in &fragments {
        let id = fixture_id(key, f.created_at);
        let mut words = recall::content_words(&format!("{}\n{}", f.title, f.body));
        words.extend(f.tags.iter().map(|t| t.to_lowercase()));
        let text = format!("{}\n{}", f.title, f.body);
        let loaded = Loaded {
            id,
            created_at: f.created_at,
            words,
            vector: meaning.map(|m| m.embed(&embed::passage(&text)).unwrap()),
            text,
            tags: f.tags.clone(),
        };
        assert!(
            by_key.insert(key.clone(), loaded).is_none(),
            "duplicate key {key}"
        );
        labels.push(key.clone());
    }
    for case in &cases {
        for key in std::iter::once(&case.seed).chain(&case.expected) {
            assert!(by_key.contains_key(key), "{}: unknown key {key}", case.name);
        }
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();
    let schemas = parc_core::schema::load_schemas(&vault).unwrap();
    let config = parc_core::config::load_config(&vault).unwrap();
    let mut writer = Writer::new(&vault, ScriptRunner::default());
    fragments.sort_by_key(|(key, f)| (f.created_at, key.clone()));
    for (key, f) in fragments {
        let schema = schemas
            .resolve(&f.fragment_type)
            .unwrap_or_else(|| panic!("{key}: unknown type {}", f.fragment_type));
        let mut fragment =
            parc_core::fragment::new_fragment(&f.fragment_type, &f.title, schema, &config);
        fragment.id = by_key[&key].id.clone();
        fragment.created_at = f.created_at;
        fragment.updated_at = f.created_at;
        fragment.created_by = None;
        fragment.tags = f.tags;
        fragment.body = f.body;
        writer
            .create(fragment)
            .unwrap_or_else(|e| panic!("{key}: {e}"));
    }

    if let Some(model) = meaning {
        let conn = parc_core::index::open_index(&vault).unwrap();
        let report = embed::fill(&conn, model, None, 4, &mut |_, _| {}).unwrap();
        assert_eq!(report.remaining, 0);
    }

    let key_of = by_key
        .iter()
        .map(|(key, l)| (l.id.clone(), key.clone()))
        .collect();
    Bench {
        meaning,
        _tmp: tmp,
        vault,
        by_key,
        key_of,
        cases,
        labels,
    }
}

/// A ULID dated `created_at`, its random part a hash of the key: stable
/// across runs and toolchains (FNV-1a, not `std`'s hasher).
fn fixture_id(key: &str, created_at: DateTime<Utc>) -> String {
    let fnv = |salt: u8| {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in key.bytes().chain([salt]) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    let random = ((u128::from(fnv(1)) << 64) | u128::from(fnv(2))) & ((1u128 << 80) - 1);
    let ms = u64::try_from(created_at.timestamp_millis()).expect("date after 1970");
    ulid::Ulid::from_parts(ms, random).to_string()
}

impl Bench<'_> {
    fn recall(&self, seed_key: &str, as_of: bool) -> Vec<Candidate> {
        self.recall_seed(
            seed_key,
            Seed::Fragment(self.by_key[seed_key].id.clone()),
            as_of,
        )
    }

    /// The same fragment, sent as unsaved text with its ID.
    fn recall_as_draft(&self, seed_key: &str) -> Vec<Candidate> {
        let seed = &self.by_key[seed_key];
        let draft = Seed::Draft {
            text: seed.text.clone(),
            tags: seed.tags.clone(),
            id: Some(seed.id.clone()),
        };
        self.recall_seed(seed_key, draft, true)
    }

    fn recall_seed(&self, seed_key: &str, seed: Seed, as_of: bool) -> Vec<Candidate> {
        let conn = parc_core::index::open_index(&self.vault).unwrap();
        recall::recall(
            &conn,
            &RecallQuery {
                seed,
                as_of: as_of.then_some(self.by_key[seed_key].created_at),
                limit: DEPTH,
            },
            self.meaning,
        )
        .unwrap()
    }

    fn key(&self, id: &str) -> &str {
        &self.key_of[id]
    }

    /// The words (and tags) two fixture fragments have in common, for
    /// telling a miss of meaning from a miss of wording.
    fn shared(&self, a: &str, b: &str) -> String {
        let theirs: HashSet<&String> = self.by_key[b].words.iter().collect();
        let shared: Vec<&str> = self.by_key[a]
            .words
            .iter()
            .filter(|w| theirs.contains(w))
            .map(String::as_str)
            .collect();
        let words = match shared.as_slice() {
            [] => "shares no words with the seed".to_string(),
            words => format!("shares with the seed: {}", words.join(", ")),
        };
        match self.closeness(a, b) {
            Some(s) => format!("{words}; meaning {s:.3}"),
            None => words,
        }
    }

    /// How close two fixture fragments are in meaning, with a model.
    fn closeness(&self, a: &str, b: &str) -> Option<f32> {
        let (a, b) = (
            self.by_key[a].vector.as_ref()?,
            self.by_key[b].vector.as_ref()?,
        );
        Some(embed::similarity(a, b))
    }

    /// Where `key` ranks by meaning alone among the fragments written before
    /// the case's seed.
    fn meaning_rank(&self, case: &Case, key: &str) -> Option<usize> {
        let own = self.closeness(&case.seed, key)?;
        let seed = &self.by_key[&case.seed];
        let closer = self
            .by_key
            .iter()
            .filter(|(k, l)| *k != &case.seed && *k != key && l.created_at <= seed.created_at)
            .filter(|(k, _)| self.closeness(&case.seed, k).is_some_and(|s| s > own))
            .count();
        Some(closer + 1)
    }

    /// How close in meaning each case's seed is to the fragments written
    /// before it: the expected ones, and all the others. What the model's
    /// thresholds are calibrated on.
    fn closeness_spread(&self) -> Option<(Vec<f32>, Vec<f32>)> {
        let (mut related, mut unrelated) = (Vec::new(), Vec::new());
        for case in &self.cases {
            let seed = &self.by_key[&case.seed];
            for (key, l) in &self.by_key {
                if *key == case.seed || l.created_at > seed.created_at {
                    continue;
                }
                let s = self.closeness(&case.seed, key)?;
                if case.expected.contains(key) {
                    related.push(s);
                } else {
                    unrelated.push(s);
                }
            }
        }
        related.sort_by(f32::total_cmp);
        unrelated.sort_by(f32::total_cmp);
        Some((related, unrelated))
    }

    /// The fragment written before `seed` that is closest to it in meaning,
    /// among those the case doesn't expect: how close chance comes.
    fn closest_unexpected(&self, case: &Case) -> Option<(String, f32)> {
        let seed = &self.by_key[&case.seed];
        self.by_key
            .iter()
            .filter(|(key, l)| {
                *key != &case.seed
                    && l.created_at <= seed.created_at
                    && !case.expected.contains(key)
            })
            .filter_map(|(key, _)| Some((key.clone(), self.closeness(&case.seed, key)?)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }
}

// ── Scorecard ──────────────────────────────────────────────────────────

fn scorecard(bench: &Bench) -> String {
    let mut found = 0;
    let mut wanted = 0;
    let mut abstained = 0;
    let mut abstain_cases = 0;
    // Fragments a case doesn't expect, returned within its limit.
    let mut others = 0;
    let mut blocks = String::new();

    for case in &bench.cases {
        let seed = &bench.by_key[&case.seed];
        let results = bench.recall(&case.seed, true);
        for c in &results {
            assert_ne!(c.id, seed.id, "{}: returned its own seed", case.name);
            let key = bench.key(&c.id);
            assert!(
                bench.by_key[key].created_at <= seed.created_at,
                "{}: returned {key}, written after the seed",
                case.name
            );
        }

        let mut lines = Vec::new();
        let verdict = if case.expected.is_empty() {
            abstain_cases += 1;
            if results.is_empty() {
                abstained += 1;
                "abstained".to_string()
            } else {
                format!("should abstain, returned {}", results.len())
            }
        } else {
            let mut hits = 0;
            for key in &case.expected {
                let id = &bench.by_key[key].id;
                let rank = results.iter().position(|c| &c.id == id).map(|i| i + 1);
                let at = match rank {
                    Some(r) if r <= case.limit => {
                        hits += 1;
                        format!("rank {r}")
                    }
                    Some(r) => format!("rank {r}, outside the top {}", case.limit),
                    None => format!("not returned; {}", bench.shared(&case.seed, key)),
                };
                let at = match (rank, bench.closeness(&case.seed, key)) {
                    (Some(_), Some(s)) => format!("{at}; meaning {s:.3}"),
                    _ => at,
                };
                let at = match bench.meaning_rank(case, key) {
                    Some(r) => format!("{at} (#{r} by meaning)"),
                    None => at,
                };
                lines.push(format!("   wanted   {key}: {at}"));
            }
            wanted += case.expected.len();
            found += hits;
            others += results
                .iter()
                .take(case.limit)
                .filter(|c| !case.expected.contains(&bench.key(&c.id).to_string()))
                .count();
            format!(
                "found {hits} of {} in the top {}",
                case.expected.len(),
                case.limit
            )
        };
        let shown = if case.expected.is_empty() {
            results.len()
        } else {
            results.len().min(case.limit.max(3))
        };
        for (i, c) in results.iter().take(shown).enumerate() {
            let label = if i == 0 { "returned" } else { "        " };
            lines.push(format!(
                "   {label} {}. {:<28} {}",
                i + 1,
                bench.key(&c.id),
                describe(c)
            ));
        }
        if let Some((key, s)) = bench.closest_unexpected(case) {
            lines.push(format!("   closest unexpected: {key}, meaning {s:.3}"));
        }
        writeln!(blocks, "\n== {}  [{verdict}]", case.name).unwrap();
        for line in lines {
            writeln!(blocks, "{line}").unwrap();
        }
    }

    let mut out = String::new();
    match bench.meaning {
        None => writeln!(
            out,
            "Recall scorecard: lexical baseline (parc_core::recall)"
        ),
        Some(m) => writeln!(
            out,
            "Recall scorecard: with meaning, {} (similar from {}, unrelated below {})",
            m.spec().key(),
            m.spec().similar,
            m.spec().unrelated
        ),
    }
    .unwrap();
    writeln!(
        out,
        "Fixtures: tests/fixtures/recall/*.json. Accept changes: {BLESS_VAR}=1 cargo test -p parc-core --test recall_bench"
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "Sources found within each case's limit: {found} of {wanted}"
    )
    .unwrap();
    writeln!(
        out,
        "Abstained where nothing is related:     {abstained} of {abstain_cases}"
    )
    .unwrap();
    writeln!(out, "Unexpected fragments within the limits: {others}").unwrap();
    if let Some((related, unrelated)) = bench.closeness_spread() {
        let at = |v: &[f32], p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        writeln!(
            out,
            "\nMeaning, seed to an earlier fragment it isn't about ({} pairs): median {:.3}, \
             75th percentile {:.3}, 90th {:.3}, 99th {:.3}, highest {:.3}",
            unrelated.len(),
            at(&unrelated, 0.5),
            at(&unrelated, 0.75),
            at(&unrelated, 0.9),
            at(&unrelated, 0.99),
            unrelated[unrelated.len() - 1],
        )
        .unwrap();
        writeln!(
            out,
            "Meaning, seed to an expected source ({} pairs): lowest {:.3}, median {:.3}, highest {:.3}",
            related.len(),
            related[0],
            at(&related, 0.5),
            related[related.len() - 1],
        )
        .unwrap();
    }
    out + &blocks
}

fn describe(c: &Candidate) -> String {
    let r = &c.reasons;
    let mut parts = Vec::new();
    if r.linked {
        parts.push("linked".to_string());
    }
    if !r.shared_tags.is_empty() {
        parts.push(format!("tags: {}", r.shared_tags.join(", ")));
    }
    if !r.shared_words.is_empty() {
        parts.push(format!("words: {}", r.shared_words.join(", ")));
    }
    match r.similarity {
        Some(s) if r.similar => parts.push(format!("similar {s:.3}")),
        Some(s) => parts.push(format!("meaning {s:.3}")),
        None => {}
    }
    if std::env::var_os("PARC_RECALL_SCORES").is_some() {
        parts.push(format!("score {:.5}", c.score));
    }
    parts.join("; ")
}

// ── Tests ──────────────────────────────────────────────────────────────

#[test]
fn lexical_scorecard_matches_the_committed_one() {
    check_scorecard(&load());
}

#[test]
fn meaning_scorecard_matches_the_committed_one() {
    let Some(model) = installed_model() else {
        return;
    };
    let bench = load_with(Some(&model));
    check_scorecard(&bench);
    // A capture recalled while typed and once saved gives one answer with
    // the model too.
    for case in &bench.cases {
        assert_eq!(
            bench.recall_as_draft(&case.seed),
            bench.recall(&case.seed, true),
            "{}",
            case.name
        );
    }
}

fn check_scorecard(bench: &Bench) {
    let now = scorecard(bench);
    let path = scorecard_path(bench.meaning);
    if std::env::var_os(BLESS_VAR).is_some() {
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    if committed != now {
        let diff = similar::TextDiff::from_lines(&committed, &now)
            .unified_diff()
            .header("committed", "now")
            .to_string();
        panic!(
            "the recall scorecard changed. If the change is intended, accept it with\n  \
             {BLESS_VAR}=1 cargo test -p parc-core --test recall_bench\n\n{diff}"
        );
    }
}

#[test]
fn fixture_ids_are_dated_and_stable() {
    let first = load();
    let second = load();
    for (key, loaded) in &first.by_key {
        assert_eq!(loaded.id, second.by_key[key].id, "{key}");
        let ulid = ulid::Ulid::from_string(&loaded.id).unwrap();
        assert_eq!(
            ulid.timestamp_ms() as i64,
            loaded.created_at.timestamp_millis(),
            "{key}"
        );
    }
    let ids: HashSet<_> = first.by_key.values().map(|l| &l.id).collect();
    assert_eq!(ids.len(), first.by_key.len(), "IDs collide");
}

#[test]
fn labels_never_reach_the_vault() {
    let bench = load();
    let mut checked = 0;
    for entry in std::fs::read_dir(bench.vault.join("fragments")).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap();
        for label in &bench.labels {
            assert!(
                !text.contains(label.as_str()),
                "label {label:?} is in the vault"
            );
        }
        for field in ["key:", "expected", "candidate_limit", "inference"] {
            assert!(!text.contains(field), "{field:?} is in the vault");
        }
        checked += 1;
    }
    assert_eq!(checked, bench.by_key.len());
}

/// The scorecard asserts that nothing written after a seed comes back. That
/// proves something only if such a fragment would come back without the
/// cut-off.
#[test]
fn as_of_is_what_keeps_later_fragments_out() {
    let bench = load();
    let decoy = &bench.by_key["ferry/future_decoy"].id;
    let ids = |found: Vec<Candidate>| found.into_iter().map(|c| c.id).collect::<Vec<_>>();
    assert!(ids(bench.recall("ferry/capture", false)).contains(decoy));
    assert!(!ids(bench.recall("ferry/capture", true)).contains(decoy));
}

/// A capture is recalled from its text while it is written, and from the
/// index once saved: both must give the same answer.
#[test]
fn a_draft_recalls_what_its_saved_fragment_does() {
    let bench = load();
    for case in &bench.cases {
        assert_eq!(
            bench.recall_as_draft(&case.seed),
            bench.recall(&case.seed, true),
            "{}",
            case.name
        );
    }
}
