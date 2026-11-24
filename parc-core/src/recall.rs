//! Recall: the other fragments that bear on a given one, however old.
//!
//! Each signal ranks its own candidates: words shared through the full-text
//! index, shared tags, and links in either direction. The rankings are merged
//! by reciprocal rank, which needs no common scale, so a later signal (such as
//! embeddings) joins as one more ranking.
//!
//! A candidate is returned only with enough evidence: a link, a shared tag, or
//! [`MIN_SHARED_WORDS`] shared words. Returning nothing is an answer.
//!
//! The whole vault is searched, archived fragments included: old material is
//! what recall is for, and archiving takes a fragment out of lists, not out of
//! memory.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::error::ParcError;

/// Fewest shared words that count as evidence on their own. One word in
/// common is chance more often than not.
pub const MIN_SHARED_WORDS: usize = 2;

/// How many candidates each signal ranks before the rankings are merged.
const POOL: usize = 50;

/// Words of the seed that go into the full-text query, in order of first use.
const MAX_QUERY_WORDS: usize = 100;

/// Damping in reciprocal-rank fusion: a candidate scores `1 / (RRF_K + rank)`
/// in each ranking it appears in. 60 is the usual value; it keeps one signal's
/// first place from outweighing agreement between signals.
const RRF_K: f64 = 60.0;

#[derive(Debug, Clone)]
pub struct RecallQuery {
    /// The fragment to find related ones for (a full ID).
    pub seed: String,
    /// Only consider fragments created at or before this time. `None` means
    /// the whole vault.
    pub as_of: Option<DateTime<Utc>>,
    /// Most candidates to return.
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Candidate {
    pub id: String,
    /// Merged reciprocal-rank score. Only the order means anything.
    pub score: f64,
    pub reasons: Reasons,
}

/// Why a candidate was returned.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Reasons {
    /// Words of the seed's title or body that the candidate also uses
    /// (lowercased, common words left out), sorted.
    pub shared_words: Vec<String>,
    pub shared_tags: Vec<String>,
    /// A link between the seed and the candidate, in either direction.
    pub linked: bool,
}

impl Reasons {
    fn is_evidence(&self) -> bool {
        self.linked || !self.shared_tags.is_empty() || self.shared_words.len() >= MIN_SHARED_WORDS
    }
}

/// Rank the fragments related to `query.seed`, best first. Never returns the
/// seed itself.
pub fn recall(conn: &Connection, query: &RecallQuery) -> Result<Vec<Candidate>, ParcError> {
    let (title, body): (String, String) = conn
        .query_row(
            "SELECT title, body FROM fragments WHERE id = ?1",
            [&query.seed],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ParcError::FragmentNotFound(query.seed.clone()),
            e => e.into(),
        })?;
    let as_of = query.as_of.map(|t| t.to_rfc3339());

    let mut candidates: HashMap<String, Candidate> = HashMap::new();
    let mut fuse = |ranking: Vec<(String, Reasons)>| {
        for (rank, (id, reasons)) in ranking.into_iter().enumerate() {
            let c = candidates.entry(id.clone()).or_insert_with(|| Candidate {
                id,
                score: 0.0,
                reasons: Reasons::default(),
            });
            c.score += 1.0 / (RRF_K + rank as f64 + 1.0);
            merge(&mut c.reasons, reasons);
        }
    };

    let mut words = content_words(&format!("{title}\n{body}"));
    words.truncate(MAX_QUERY_WORDS);
    fuse(by_words(conn, &query.seed, as_of.as_deref(), &words)?);
    fuse(by_tags(conn, &query.seed, as_of.as_deref())?);
    fuse(by_links(conn, &query.seed, as_of.as_deref())?);

    let mut found: Vec<Candidate> = candidates
        .into_values()
        .filter(|c| c.reasons.is_evidence())
        .collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    found.truncate(query.limit);
    Ok(found)
}

fn merge(into: &mut Reasons, from: Reasons) {
    if into.shared_words.is_empty() {
        into.shared_words = from.shared_words;
    }
    if into.shared_tags.is_empty() {
        into.shared_tags = from.shared_tags;
    }
    into.linked |= from.linked;
}

/// SQL that keeps `f` to fragments other than the seed (?1), created at or
/// before the cut-off (?2, NULL for none). Dates are compared as instants:
/// stored ones may carry fractional seconds.
const ELIGIBLE: &str = "f.id != ?1 AND (?2 IS NULL OR julianday(f.created_at) <= julianday(?2))";

/// Fragments sharing the seed's words, ranked by BM25.
fn by_words(
    conn: &Connection,
    seed: &str,
    as_of: Option<&str>,
    words: &[String],
) -> Result<Vec<(String, Reasons)>, ParcError> {
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let seed_words: HashSet<&str> = words.iter().map(String::as_str).collect();
    let fts_query = words
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut stmt = conn.prepare(&format!(
        "SELECT f.id, fragments_fts.title, fragments_fts.body, fragments_fts.tags
         FROM fragments_fts JOIN fragments f ON f.seq = fragments_fts.rowid
         WHERE fragments_fts MATCH ?3 AND {ELIGIBLE}
         ORDER BY bm25(fragments_fts) LIMIT {POOL}"
    ))?;
    let rows = stmt.query_map(rusqlite::params![seed, as_of, fts_query], |row| {
        Ok((
            row.get::<_, String>(0)?,
            format!(
                "{}\n{}\n{}",
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?
            ),
        ))
    })?;
    let mut ranked = Vec::new();
    for row in rows {
        let (id, text) = row?;
        let shared: BTreeSet<String> = content_words(&text)
            .into_iter()
            .filter(|w| seed_words.contains(w.as_str()))
            .collect();
        let reasons = Reasons {
            shared_words: shared.into_iter().collect(),
            ..Reasons::default()
        };
        ranked.push((id, reasons));
    }
    Ok(ranked)
}

/// Fragments sharing the seed's tags, most shared tags first, then newest.
fn by_tags(
    conn: &Connection,
    seed: &str,
    as_of: Option<&str>,
) -> Result<Vec<(String, Reasons)>, ParcError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT f.id, t.tag FROM fragment_tags t JOIN fragments f ON f.id = t.fragment_id
         WHERE t.tag IN (SELECT tag FROM fragment_tags WHERE fragment_id = ?1) AND {ELIGIBLE}
         ORDER BY f.created_at DESC, f.id, t.tag"
    ))?;
    let mut order: Vec<String> = Vec::new();
    let mut tags: HashMap<String, Vec<String>> = HashMap::new();
    for row in stmt.query_map(rusqlite::params![seed, as_of], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })? {
        let (id, tag) = row?;
        let list = tags.entry(id.clone()).or_default();
        if list.is_empty() {
            order.push(id);
        }
        list.push(tag);
    }
    // Stable: equal counts keep the newest-first order of the query.
    order.sort_by_key(|id| std::cmp::Reverse(tags[id].len()));
    order.truncate(POOL);
    Ok(order
        .into_iter()
        .map(|id| {
            let shared_tags = tags.remove(&id).unwrap_or_default();
            (
                id,
                Reasons {
                    shared_tags,
                    ..Reasons::default()
                },
            )
        })
        .collect())
}

/// Fragments the seed links to or that link to it, newest first.
fn by_links(
    conn: &Connection,
    seed: &str,
    as_of: Option<&str>,
) -> Result<Vec<(String, Reasons)>, ParcError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT f.id FROM fragments f
         WHERE f.id IN (
             SELECT resolved_id FROM fragment_links WHERE source_id = ?1
             UNION SELECT source_id FROM fragment_links WHERE resolved_id = ?1
         ) AND {ELIGIBLE}
         ORDER BY f.created_at DESC, f.id LIMIT {POOL}"
    ))?;
    let ids = stmt
        .query_map(rusqlite::params![seed, as_of], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids
        .into_iter()
        .map(|id| {
            (
                id,
                Reasons {
                    linked: true,
                    ..Reasons::default()
                },
            )
        })
        .collect())
}

/// The words of `text` worth matching on: lowercased, split at anything that
/// isn't a letter or digit, without common English and Finnish words, numbers
/// and single letters. Unique, in order of first use.
pub fn content_words(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() > 1)
        .filter(|w| !w.chars().all(|c| c.is_numeric()))
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

/// Common words that say nothing about a topic. English, then Finnish (with
/// the usual inflections of pronouns and auxiliaries, since Finnish has no
/// separate words for "of", "in" or "to").
const STOPWORDS: &[&str] = &[
    // English
    "a",
    "about",
    "above",
    "after",
    "afterwards",
    "again",
    "against",
    "all",
    "almost",
    "also",
    "am",
    "an",
    "and",
    "any",
    "anyway",
    "are",
    "around",
    "as",
    "at",
    "be",
    "because",
    "been",
    "before",
    "being",
    "below",
    "between",
    "both",
    "but",
    "by",
    "can",
    "cannot",
    "could",
    "did",
    "didn",
    "do",
    "does",
    "doesn",
    "doing",
    "don",
    "done",
    "down",
    "during",
    "each",
    "either",
    "else",
    "even",
    "ever",
    "every",
    "few",
    "for",
    "from",
    "further",
    "get",
    "gets",
    "got",
    "had",
    "hadn",
    "has",
    "hasn",
    "have",
    "haven",
    "having",
    "he",
    "her",
    "here",
    "hers",
    "herself",
    "him",
    "himself",
    "his",
    "how",
    "however",
    "i",
    "if",
    "in",
    "into",
    "is",
    "isn",
    "it",
    "its",
    "itself",
    "just",
    "ll",
    "may",
    "maybe",
    "me",
    "might",
    "more",
    "most",
    "much",
    "must",
    "my",
    "myself",
    "no",
    "nor",
    "not",
    "now",
    "of",
    "off",
    "on",
    "once",
    "one",
    "only",
    "or",
    "other",
    "our",
    "ours",
    "ourselves",
    "out",
    "over",
    "own",
    "quite",
    "rather",
    "re",
    "really",
    "same",
    "she",
    "should",
    "shouldn",
    "so",
    "some",
    "still",
    "such",
    "than",
    "that",
    "the",
    "their",
    "theirs",
    "them",
    "themselves",
    "then",
    "there",
    "these",
    "they",
    "this",
    "those",
    "though",
    "through",
    "to",
    "too",
    "under",
    "until",
    "up",
    "us",
    "ve",
    "very",
    "was",
    "wasn",
    "we",
    "well",
    "were",
    "weren",
    "what",
    "when",
    "where",
    "which",
    "while",
    "who",
    "whom",
    "why",
    "will",
    "with",
    "won",
    "would",
    "wouldn",
    "yet",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
    // Finnish
    "ei",
    "eikä",
    "en",
    "emme",
    "et",
    "ette",
    "eivät",
    "että",
    "he",
    "heidän",
    "heidät",
    "heihin",
    "heille",
    "heillä",
    "heiltä",
    "heistä",
    "heitä",
    "hän",
    "häneen",
    "hänelle",
    "hänellä",
    "häneltä",
    "hänen",
    "hänessä",
    "hänestä",
    "hänet",
    "häntä",
    "ja",
    "jo",
    "joka",
    "jolla",
    "jolle",
    "jolta",
    "jonka",
    "jos",
    "jossa",
    "josta",
    "jota",
    "jotka",
    "joita",
    "joiden",
    "jotta",
    "kanssa",
    "kaikki",
    "koska",
    "kuin",
    "kuinka",
    "kuka",
    "kun",
    "me",
    "meidän",
    "meidät",
    "meille",
    "meillä",
    "meiltä",
    "meistä",
    "meitä",
    "mikä",
    "miksi",
    "mille",
    "millä",
    "miltä",
    "minkä",
    "minne",
    "minua",
    "minulla",
    "minulle",
    "minulta",
    "minun",
    "minussa",
    "minusta",
    "minut",
    "minuun",
    "minä",
    "missä",
    "mistä",
    "mitkä",
    "mitä",
    "mutta",
    "myös",
    "ne",
    "niiden",
    "niihin",
    "niille",
    "niillä",
    "niiltä",
    "niin",
    "niissä",
    "niistä",
    "niitä",
    "noin",
    "nyt",
    "nämä",
    "näiden",
    "näitä",
    "ole",
    "olemme",
    "olen",
    "olet",
    "olette",
    "oli",
    "olimme",
    "olin",
    "olisi",
    "olit",
    "olivat",
    "olla",
    "olleet",
    "ollut",
    "on",
    "ovat",
    "se",
    "sekä",
    "sen",
    "siihen",
    "siinä",
    "siitä",
    "sille",
    "sillä",
    "siltä",
    "sinua",
    "sinulla",
    "sinulle",
    "sinulta",
    "sinun",
    "sinussa",
    "sinusta",
    "sinut",
    "sinä",
    "sitten",
    "sitä",
    "taas",
    "tai",
    "te",
    "teidän",
    "teille",
    "teillä",
    "teiltä",
    "teistä",
    "teitä",
    "tuo",
    "tuolla",
    "tuon",
    "tuota",
    "tämä",
    "tämän",
    "tässä",
    "tästä",
    "tätä",
    "vaan",
    "vai",
    "vain",
    "vielä",
    "voi",
    "yli",
    // Finnish postpositions and adverbs ("after", "before", "maybe", "very", ...)
    "aikana",
    "aina",
    "ehkä",
    "eli",
    "ennen",
    "hyvin",
    "ihan",
    "ilman",
    "joku",
    "jokin",
    "joten",
    "jotain",
    "jälkeen",
    "kautta",
    "kuitenkin",
    "kuten",
    "melko",
    "mitään",
    "mukaan",
    "muu",
    "muut",
    "paljon",
    "sama",
    "siis",
    "takia",
    "todella",
    "toinen",
    "tosi",
    "vuoksi",
    "vähän",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::ScriptRunner;
    use crate::ops::Writer;
    use std::path::Path;

    fn note(writer: &mut Writer, title: &str, body: &str, links: &[&str]) -> String {
        let vault = writer.vault().to_path_buf();
        let config = crate::config::load_config(&vault).unwrap();
        let schemas = crate::schema::load_schemas(&vault).unwrap();
        let mut f =
            crate::fragment::new_fragment("note", title, schemas.resolve("note").unwrap(), &config);
        f.body = body.to_string();
        f.links = links.iter().map(|s| s.to_string()).collect();
        writer.create(f).unwrap().id
    }

    fn ids(vault: &Path, seed: &str) -> Vec<(String, bool)> {
        let conn = crate::index::open_index(vault).unwrap();
        let query = RecallQuery {
            seed: seed.to_string(),
            as_of: None,
            limit: 10,
        };
        recall(&conn, &query)
            .unwrap()
            .into_iter()
            .map(|c| (c.id, c.reasons.linked))
            .collect()
    }

    #[test]
    fn links_count_in_both_directions_without_shared_words() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let mut w = Writer::new(&vault, ScriptRunner::default());
        let old = note(&mut w, "Ferry desk", "Route cards", &[]);
        let seed = note(&mut w, "Orientation", "Question slips", &[&old]);
        let later = note(&mut w, "Follow-up", "Went fine", &[&seed]);
        note(&mut w, "Unrelated", "Kitchen timer", &[]);

        let mut found = ids(&vault, &seed);
        found.sort();
        let mut want = vec![(old, true), (later, true)];
        want.sort();
        assert_eq!(found, want);
    }

    #[test]
    fn unknown_seed_is_an_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let conn = crate::index::open_index(&vault).unwrap();
        let query = RecallQuery {
            seed: "01JQ7V3XKP5GQZ2N8R6T1WBMVH".into(),
            as_of: None,
            limit: 5,
        };
        assert!(matches!(
            recall(&conn, &query),
            Err(ParcError::FragmentNotFound(_))
        ));
    }

    #[test]
    fn content_words_drop_common_words_numbers_and_repeats() {
        assert_eq!(
            content_words("The ferry desk, the FERRY queue at 6 a.m. — ja jono"),
            vec!["ferry", "desk", "queue", "jono"]
        );
    }

    #[test]
    fn content_words_keep_letters_outside_ascii() {
        assert_eq!(
            content_words("Kävin kirjastossa"),
            vec!["kävin", "kirjastossa"]
        );
    }

    #[test]
    fn one_shared_word_is_not_evidence() {
        let mut r = Reasons {
            shared_words: vec!["room".into()],
            ..Reasons::default()
        };
        assert!(!r.is_evidence());
        r.shared_words.push("volunteer".into());
        assert!(r.is_evidence());
        assert!(Reasons {
            linked: true,
            ..Reasons::default()
        }
        .is_evidence());
    }
}
