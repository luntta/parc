//! Recall: the other fragments that bear on a fragment, or on text that isn't
//! saved yet, however old they are.
//!
//! Each signal ranks its own candidates: words shared through the full-text
//! index, shared tags, links in either direction and, when the model is
//! installed, closeness of meaning ([`crate::embed`]). The rankings are merged
//! by reciprocal rank, which needs no common scale. A signal ranks only what
//! it can tell apart: candidates it scores alike (every linked fragment, or
//! fragments sharing the same tags) share a rank, and the other signals order
//! them.
//!
//! A candidate is returned only with enough evidence: a link, a shared tag,
//! [`MIN_SHARED_WORDS`] shared words, or a meaning at least as close as the
//! model's threshold. Returning nothing is an answer.
//!
//! The whole vault is searched, archived fragments included: old material is
//! what recall is for, and archiving takes a fragment out of lists, not out of
//! memory.
//!
//! [`recall`] ranks; [`context`] adds what a reader (a person, or a model)
//! needs to judge each source: its facts and short verbatim excerpts.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::Serialize;

use crate::embed::{self, Model};
use crate::error::ParcError;

/// Fewest shared words that count as evidence on their own. One word in
/// common is chance more often than not.
pub const MIN_SHARED_WORDS: usize = 2;

/// Sources [`context`] returns unless asked for another number.
pub const DEFAULT_LIMIT: usize = 5;
/// Most sources [`context`] returns.
pub const MAX_LIMIT: usize = 20;

/// How many candidates each signal ranks before the rankings are merged.
const POOL: usize = 50;

/// Words of the seed that go into the full-text query, in order of first use.
const MAX_QUERY_WORDS: usize = 100;

/// Damping in reciprocal-rank fusion: a candidate scores `1 / (RRF_K + rank)`
/// in each ranking it appears in. 60 is the usual value; it keeps one signal's
/// first place from outweighing agreement between signals.
const RRF_K: f64 = 60.0;

/// Excerpts per source, and the longest one in characters.
const MAX_EXCERPTS: usize = 2;
const EXCERPT_CHARS: usize = 200;

/// What to find related fragments for.
#[derive(Debug, Clone)]
pub enum Seed {
    /// A saved fragment, as indexed (a full ID).
    Fragment(String),
    /// Text that isn't saved: a capture being typed or dictated (title and
    /// body together), or unsaved edits to the fragment `id`. Its `#tags` and
    /// `[[links]]` count as they would once saved. With an `id`, that fragment
    /// is left out of the results, and its saved frontmatter links and the
    /// fragments linking to it still count.
    Draft {
        text: String,
        tags: Vec<String>,
        id: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct RecallQuery {
    pub seed: Seed,
    /// Only consider fragments created at or before this time. `None` means
    /// the whole vault.
    pub as_of: Option<DateTime<Utc>>,
    /// Most candidates to return.
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Candidate {
    pub id: String,
    /// Merged reciprocal-rank score. Only the order means anything.
    pub score: f64,
    pub reasons: Reasons,
}

/// Why a candidate was returned.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Reasons {
    /// Words of the seed that the candidate also uses (lowercased, common
    /// words left out), sorted.
    pub shared_words: Vec<String>,
    pub shared_tags: Vec<String>,
    /// A link between the seed and the candidate, in either direction.
    pub linked: bool,
    /// How close the two are in meaning (cosine similarity of their vectors,
    /// at most 1), when the model has read both.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
    /// The meaning is close enough to count on its own: the two say the
    /// same thing, perhaps in other words or another language.
    pub similar: bool,
}

/// Similarities that decide evidence, from the meaning model.
#[derive(Debug, Clone, Copy)]
struct Bars {
    /// From here on, meaning alone is evidence.
    similar: f32,
    /// Below this, shared words alone are not: the model reads the two as
    /// unrelated, so the words are chance ("a pitch to a client", "the pitch
    /// at the school").
    unrelated: f32,
}

impl Bars {
    /// No model: meaning never counts, and never vetoes.
    const NONE: Bars = Bars {
        similar: f32::INFINITY,
        unrelated: f32::NEG_INFINITY,
    };
}

impl Reasons {
    /// Whether these reasons are enough to return the candidate. A link or a
    /// shared tag always is: someone made it.
    fn is_evidence(&self, bars: Bars) -> bool {
        let words = self.shared_words.len() >= MIN_SHARED_WORDS
            && self.similarity.is_none_or(|s| s >= bars.unrelated);
        self.linked
            || !self.shared_tags.is_empty()
            || words
            || self.similarity.is_some_and(|s| s >= bars.similar)
    }
}

/// What [`context`] returns: the sources, best first, and what was searched.
#[derive(Debug, Clone, Serialize)]
pub struct RecallContext {
    pub seed: SeedInfo,
    /// Best first. Empty when nothing in the vault is related enough.
    pub sources: Vec<Source>,
    /// True when more fragments qualified than the limit allowed.
    pub truncated: bool,
    /// The meaning model, when one was used, and how much of the vault it
    /// has read so far.
    pub meaning: Option<MeaningInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeaningInfo {
    /// [`embed::ModelSpec::key`].
    pub model: String,
    /// Fragments compared with the seed by meaning.
    pub compared: usize,
    /// Fragments the model hasn't read yet (no up-to-date vector), so found
    /// by words, tags and links only.
    pub pending: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeedInfo {
    /// The saved fragment the seed is, or is a draft of.
    pub id: Option<String>,
    /// The words searched for, in order of first use.
    pub words: Vec<String>,
    pub tags: Vec<String>,
}

/// A related fragment, with enough to judge the relation without opening it.
#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub id: String,
    #[serde(rename = "type")]
    pub fragment_type: String,
    pub title: String,
    pub status: Option<String>,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    /// The file's revision as indexed, as `fragment.get` reports it: a quote
    /// from the excerpts still stands while the revision is unchanged.
    pub revision: Option<String>,
    pub score: f64,
    pub reasons: Reasons,
    /// Up to two lines of the body, verbatim: those with the most shared
    /// words, or the first line with text when no word is shared.
    pub excerpts: Vec<Excerpt>,
}

/// Part of one body line, or one line from an attachment part.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Excerpt {
    /// 1-based line of the body (or of the attachment part) as it is edited (as in `grep`).
    pub line: usize,
    /// 1-based column, in characters, where `text` starts: past 1 when a long
    /// line was cut down to the part around its first match.
    pub column: usize,
    pub text: String,
    /// Shared words in `text`, as `[start, end)` character offsets into it.
    pub ranges: Vec<(usize, usize)>,
    /// Attachment filename when this excerpt comes from an attachment rather than the fragment body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment: Option<String>,
    /// 1-based page, slide, or section number within `attachment`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<usize>,
    /// Human-readable location label (`p. 12`, `slide 3`, `§ Heading`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The seed reduced to what the signals search with.
struct Probe {
    /// A fragment never returned (the seed itself, or the one being edited).
    exclude: Option<String>,
    /// The seed's text as the meaning model reads it.
    passage: String,
    words: Vec<String>,
    tags: Vec<String>,
    /// Fragments the seed links to (full IDs).
    links_to: Vec<String>,
    /// Count the fragments that link to this one.
    linked_from: Option<String>,
}

/// Rank the fragments related to the seed, best first. Never returns the seed
/// itself (or, for a draft, the fragment it is a draft of). With a `meaning`
/// model, closeness of meaning is one more signal, over the fragments whose
/// vectors [`embed::fill`] has computed.
pub fn recall(
    conn: &Connection,
    query: &RecallQuery,
    meaning: Option<&Model>,
) -> Result<Vec<Candidate>, ParcError> {
    let probe = probe(conn, &query.seed)?;
    Ok(rank(conn, &probe, query.as_of, query.limit, meaning)?.0)
}

/// [`recall`] with each source's facts and verbatim excerpts. `limit` is
/// capped at [`MAX_LIMIT`].
pub fn context(
    conn: &Connection,
    query: &RecallQuery,
    meaning: Option<&Model>,
) -> Result<RecallContext, ParcError> {
    let probe = probe(conn, &query.seed)?;
    let limit = query.limit.min(MAX_LIMIT);
    let (mut found, meaning, att_hits) = rank(conn, &probe, query.as_of, limit + 1, meaning)?;
    let truncated = found.len() > limit;
    found.truncate(limit);

    let mut stmt = conn.prepare_cached(
        "SELECT f.type, f.title, f.status, f.archived, f.created_at, f.updated_at, f.body, ff.revision
         FROM fragments f LEFT JOIN fragment_files ff ON ff.id = f.id
         WHERE f.id = ?1",
    )?;
    let mut sources = Vec::with_capacity(found.len());
    for c in found {
        let att_hit = att_hits.get(&c.id);
        let mut source = stmt.query_row([&c.id], |row| {
            let body: String = row.get(6)?;
            let body_ex = excerpts(&body, &c.reasons.shared_words);
            let combined_ex = combine_excerpts(body_ex, att_hit, &c.reasons.shared_words);
            Ok(Source {
                id: String::new(),
                fragment_type: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                archived: row.get::<_, i64>(3)? != 0,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                revision: row.get(7)?,
                score: c.score,
                excerpts: combined_ex,
                reasons: Reasons::default(),
            })
        })?;
        source.id = c.id;
        source.reasons = c.reasons;
        sources.push(source);
    }

    Ok(RecallContext {
        seed: SeedInfo {
            id: probe.exclude,
            words: probe.words,
            tags: probe.tags,
        },
        sources,
        truncated,
        meaning,
    })
}

fn combine_excerpts(
    body_ex: Vec<Excerpt>,
    att_hit: Option<&AttachmentMatch>,
    words: &[String],
) -> Vec<Excerpt> {
    let Some(hit) = att_hit else {
        return body_ex;
    };
    let label = hit.unit.format_ref(hit.part, Some(&hit.text));
    let mut att_ex: Vec<Excerpt> = excerpts(&hit.text, words)
        .into_iter()
        .map(|mut e| {
            e.attachment = Some(hit.filename.clone());
            e.part = Some(hit.part);
            e.label = Some(label.clone());
            e
        })
        .collect();
    if att_ex.is_empty() {
        return body_ex;
    }
    let body_has_match = body_ex.iter().any(|e| !e.ranges.is_empty());
    if !body_has_match {
        return att_ex;
    }
    let mut combined = Vec::with_capacity(MAX_EXCERPTS);
    if let Some(first_body) = body_ex.into_iter().next() {
        combined.push(first_body);
    }
    if combined.len() < MAX_EXCERPTS {
        combined.push(att_ex.remove(0));
    }
    combined
}

fn probe(conn: &Connection, seed: &Seed) -> Result<Probe, ParcError> {
    match seed {
        Seed::Fragment(id) => {
            let (title, body): (String, String) = conn
                .query_row(
                    "SELECT title, body FROM fragments WHERE id = ?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| not_found(e, id))?;
            let text = format!("{title}\n{body}");
            Ok(Probe {
                exclude: Some(id.clone()),
                passage: embed::passage(&text),
                words: query_words(&text),
                tags: column(
                    conn,
                    "SELECT tag FROM fragment_tags WHERE fragment_id = ?1 ORDER BY tag",
                    id,
                )?,
                links_to: column(
                    conn,
                    "SELECT DISTINCT resolved_id FROM fragment_links
                     WHERE source_id = ?1 AND resolved_id IS NOT NULL",
                    id,
                )?,
                linked_from: Some(id.clone()),
            })
        }
        Seed::Draft { text, tags, id } => {
            if let Some(id) = id {
                conn.query_row("SELECT 1 FROM fragments WHERE id = ?1", [id], |_| Ok(()))
                    .map_err(|e| not_found(e, id))?;
            }
            let inline = crate::tag::extract_inline_tags(text);
            let mut links_to = draft_links(conn, text, id.as_deref())?;
            if let Some(id) = id {
                // Frontmatter links aren't in the text; body links are.
                links_to.extend(column(
                    conn,
                    "SELECT DISTINCT resolved_id FROM fragment_links
                     WHERE source_id = ?1 AND from_body = 0 AND resolved_id IS NOT NULL",
                    id,
                )?);
                links_to.sort();
                links_to.dedup();
            }
            Ok(Probe {
                exclude: id.clone(),
                passage: embed::passage(text),
                words: query_words(text),
                tags: crate::tag::merge_tags(tags, &inline),
                links_to,
                linked_from: id.clone(),
            })
        }
    }
}

fn not_found(e: rusqlite::Error, id: &str) -> ParcError {
    match e {
        rusqlite::Error::QueryReturnedNoRows => ParcError::FragmentNotFound(id.to_string()),
        e => e.into(),
    }
}

fn column(conn: &Connection, sql: &str, id: &str) -> Result<Vec<String>, ParcError> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([id], |row| row.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The fragments a draft's `[[links]]` name, resolved as the index resolves
/// body links: ID prefix, exact title, then unique title prefix.
fn draft_links(
    conn: &Connection,
    text: &str,
    own_id: Option<&str>,
) -> Result<Vec<String>, ParcError> {
    let links = crate::link::parse_wiki_links(text);
    if links.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare_cached("SELECT id, title FROM fragments")?;
    let refs = stmt
        .query_map([], |row| {
            Ok(crate::link::FragmentRef {
                id: row.get(0)?,
                title: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids: Vec<String> = links
        .iter()
        .filter_map(
            |l| match crate::link::resolve_link_target(&l.target, &refs) {
                crate::link::ResolveOutcome::Unique(id) if Some(id.as_str()) != own_id => Some(id),
                _ => None,
            },
        )
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

struct AttachmentMatch {
    filename: String,
    part: usize,
    unit: crate::extract::PartUnit,
    text: String,
}

fn rank(
    conn: &Connection,
    probe: &Probe,
    as_of: Option<DateTime<Utc>>,
    limit: usize,
    meaning: Option<&Model>,
) -> Result<
    (
        Vec<Candidate>,
        Option<MeaningInfo>,
        HashMap<String, AttachmentMatch>,
    ),
    ParcError,
> {
    let filter = Eligible {
        exclude: probe.exclude.as_deref(),
        as_of: as_of.map(|t| t.to_rfc3339()),
    };

    let mut candidates: HashMap<String, Candidate> = HashMap::new();
    let mut fuse = |mut ranking: Vec<Ranked>| {
        ranking.sort_by(|a, b| b.strength.total_cmp(&a.strength));
        // Competition ranking: equals share the best rank of their group.
        let mut rank = 0;
        for (i, r) in ranking.iter().enumerate() {
            if i == 0 || r.strength < ranking[i - 1].strength {
                rank = i + 1;
            }
            let c = candidates.entry(r.id.clone()).or_insert_with(|| Candidate {
                id: r.id.clone(),
                score: 0.0,
                reasons: Reasons::default(),
            });
            c.score += 1.0 / (RRF_K + rank as f64);
            merge(&mut c.reasons, r.reasons.clone());
        }
    };
    fuse(by_words(conn, &filter, &probe.words)?);
    let (att_ranking, att_hits) = by_attachment_words(conn, &filter, &probe.words)?;
    fuse(att_ranking);
    fuse(by_tags(conn, &filter, &probe.tags)?);
    fuse(by_links(
        conn,
        &filter,
        &probe.links_to,
        probe.linked_from.as_deref(),
    )?);
    let mut bars = Bars::NONE;
    let mut closeness = HashMap::new();
    let mut info = None;
    if let Some(model) = meaning {
        if !probe.passage.is_empty() {
            let (ranking, all) = by_meaning(conn, &filter, model, &probe.passage)?;
            fuse(ranking);
            closeness = all;
            bars = Bars {
                similar: model.spec().similar,
                unrelated: model.spec().unrelated,
            };
        }
        let eligible: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM fragments f WHERE {ELIGIBLE}"),
            rusqlite::params![filter.exclude, filter.as_of],
            |r| r.get(0),
        )?;
        info = Some(MeaningInfo {
            model: model.spec().key(),
            compared: closeness.len(),
            pending: (eligible as usize).saturating_sub(closeness.len()),
        });
    }

    let mut found: Vec<Candidate> = candidates
        .into_values()
        .map(|mut c| {
            // Candidates the meaning ranking left out still have a closeness,
            // which can veto their shared words.
            c.reasons.similarity = c.reasons.similarity.or(closeness.get(&c.id).copied());
            c
        })
        .filter(|c| c.reasons.is_evidence(bars))
        .map(|mut c| {
            c.reasons.similar = c.reasons.similarity.is_some_and(|s| s >= bars.similar);
            c
        })
        .collect();
    // Equal scores: the older first (IDs sort by creation time). Recall is
    // for what would otherwise be forgotten.
    found.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    found.truncate(limit);
    Ok((found, info, att_hits))
}

/// A signal's candidate: higher `strength` ranks higher, equal strengths
/// share a rank.
struct Ranked {
    id: String,
    reasons: Reasons,
    strength: f64,
}

fn merge(into: &mut Reasons, from: Reasons) {
    if !from.shared_words.is_empty() {
        if into.shared_words.is_empty() {
            into.shared_words = from.shared_words;
        } else {
            let mut combined: BTreeSet<String> = into.shared_words.drain(..).collect();
            combined.extend(from.shared_words);
            into.shared_words = combined.into_iter().collect();
        }
    }
    if into.shared_tags.is_empty() {
        into.shared_tags = from.shared_tags;
    }
    into.linked |= from.linked;
    into.similarity = into.similarity.or(from.similarity);
}

/// Which fragments may be returned: not `exclude`, and created at or before
/// `as_of` if one is set.
struct Eligible<'a> {
    exclude: Option<&'a str>,
    as_of: Option<String>,
}

/// SQL that keeps `f` to fragments other than ?1 (NULL for none), created at
/// or before ?2 (NULL for no cut-off). Dates are compared as instants: stored
/// ones may carry fractional seconds.
const ELIGIBLE: &str = "(?1 IS NULL OR f.id <> ?1)
     AND (?2 IS NULL OR julianday(f.created_at) <= julianday(?2))";

/// Fragments sharing the seed's words, ranked by BM25.
fn by_words(
    conn: &Connection,
    filter: &Eligible,
    words: &[String],
) -> Result<Vec<Ranked>, ParcError> {
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let seed_words: HashSet<&str> = words.iter().map(String::as_str).collect();
    let fts_query = words
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT f.id, fragments_fts.title, fragments_fts.body, fragments_fts.tags,
                bm25(fragments_fts)
         FROM fragments_fts JOIN fragments f ON f.seq = fragments_fts.rowid
         WHERE fragments_fts MATCH ?3 AND {ELIGIBLE}
         ORDER BY bm25(fragments_fts) LIMIT {POOL}"
    ))?;
    let rows = stmt.query_map(
        rusqlite::params![filter.exclude, filter.as_of, fts_query],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                format!(
                    "{}\n{}\n{}",
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?
                ),
                row.get::<_, f64>(4)?,
            ))
        },
    )?;
    let mut ranked = Vec::new();
    for row in rows {
        let (id, text, bm25) = row?;
        let shared: BTreeSet<String> = content_words(&text)
            .into_iter()
            .filter(|w| seed_words.contains(w.as_str()))
            .collect();
        let reasons = Reasons {
            shared_words: shared.into_iter().collect(),
            ..Reasons::default()
        };
        // SQLite's bm25() is lower for better matches.
        ranked.push(Ranked {
            id,
            reasons,
            strength: -bm25,
        });
    }
    Ok(ranked)
}

/// Fragments whose attachments share the seed's words, ranked by best part BM25.
fn by_attachment_words(
    conn: &Connection,
    filter: &Eligible,
    words: &[String],
) -> Result<(Vec<Ranked>, HashMap<String, AttachmentMatch>), ParcError> {
    if words.is_empty() {
        return Ok((Vec::new(), HashMap::new()));
    }
    let seed_words: HashSet<&str> = words.iter().map(String::as_str).collect();
    let fts_query = words
        .iter()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    let row_limit = POOL * 3;
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT f.id, attachment_fts.filename, CAST(attachment_fts.part AS INTEGER),
                af.unit, attachment_fts.text, bm25(attachment_fts)
         FROM attachment_fts
         JOIN fragments f ON f.id = attachment_fts.fragment_id
         LEFT JOIN attachment_files af
           ON af.fragment_id = attachment_fts.fragment_id
          AND af.filename = attachment_fts.filename
         WHERE attachment_fts MATCH ?3 AND {ELIGIBLE}
         ORDER BY bm25(attachment_fts) LIMIT {row_limit}"
    ))?;
    let rows = stmt.query_map(
        rusqlite::params![filter.exclude, filter.as_of, fts_query],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, f64>(5)?,
            ))
        },
    )?;

    struct Acc {
        strength: f64,
        shared: BTreeSet<String>,
        best: AttachmentMatch,
    }
    let mut by_frag: HashMap<String, Acc> = HashMap::new();
    for row in rows {
        let (id, filename, part_i, unit_s, text, bm25) = row?;
        let strength = -bm25;
        let part = part_i.max(1) as usize;
        let unit = unit_s
            .as_deref()
            .and_then(crate::extract::PartUnit::parse)
            .unwrap_or(crate::extract::PartUnit::Page);
        let shared: Vec<String> = content_words(&text)
            .into_iter()
            .filter(|w| seed_words.contains(w.as_str()))
            .collect();
        if shared.is_empty() {
            continue;
        }
        match by_frag.get_mut(&id) {
            Some(acc) => {
                acc.shared.extend(shared);
                if strength > acc.strength {
                    acc.strength = strength;
                    acc.best = AttachmentMatch {
                        filename,
                        part,
                        unit,
                        text,
                    };
                }
            }
            None => {
                by_frag.insert(
                    id,
                    Acc {
                        strength,
                        shared: shared.into_iter().collect(),
                        best: AttachmentMatch {
                            filename,
                            part,
                            unit,
                            text,
                        },
                    },
                );
            }
        }
    }

    let mut ranked = Vec::with_capacity(by_frag.len());
    let mut hits = HashMap::with_capacity(by_frag.len());
    for (id, acc) in by_frag {
        hits.insert(id.clone(), acc.best);
        ranked.push(Ranked {
            id,
            reasons: Reasons {
                shared_words: acc.shared.into_iter().collect(),
                ..Reasons::default()
            },
            strength: acc.strength,
        });
    }
    ranked.sort_by(|a, b| {
        b.strength
            .total_cmp(&a.strength)
            .then_with(|| a.id.cmp(&b.id))
    });
    ranked.truncate(POOL);
    Ok((ranked, hits))
}

/// Fragments sharing the seed's tags. A rare tag says more than a common
/// one, so each shared tag weighs `ln(N / n)`, `n` of the `N` fragments
/// having it; fragments sharing the same tags tie.
fn by_tags(
    conn: &Connection,
    filter: &Eligible,
    tags: &[String],
) -> Result<Vec<Ranked>, ParcError> {
    if tags.is_empty() {
        return Ok(Vec::new());
    }
    let tags_json = serde_json::to_string(tags)?;
    let total: f64 = conn.query_row("SELECT COUNT(*) FROM fragments", [], |r| r.get(0))?;
    let mut stmt = conn.prepare_cached(
        "SELECT tag, COUNT(*) FROM fragment_tags
         WHERE tag IN (SELECT value FROM json_each(?1)) GROUP BY tag",
    )?;
    let weight: HashMap<String, f64> = stmt
        .query_map([&tags_json], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .map(|r| r.map(|(tag, n)| (tag, (total / n).ln().max(0.0) + f64::EPSILON)))
        .collect::<Result<_, _>>()?;
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT f.id, t.tag FROM fragment_tags t JOIN fragments f ON f.id = t.fragment_id
         WHERE t.tag IN (SELECT value FROM json_each(?3)) AND {ELIGIBLE}
         ORDER BY f.id, t.tag"
    ))?;
    let mut shared: HashMap<String, Vec<String>> = HashMap::new();
    for row in stmt.query_map(
        rusqlite::params![filter.exclude, filter.as_of, tags_json],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )? {
        let (id, tag) = row?;
        shared.entry(id).or_default().push(tag);
    }
    let mut ranked: Vec<Ranked> = shared
        .into_iter()
        .map(|(id, shared_tags)| Ranked {
            strength: shared_tags
                .iter()
                .map(|t| weight.get(t).copied().unwrap_or(0.0))
                .sum(),
            id,
            reasons: Reasons {
                shared_tags,
                ..Reasons::default()
            },
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.strength
            .total_cmp(&a.strength)
            .then_with(|| a.id.cmp(&b.id))
    });
    // Keep whole ties: cutting one in two would rank by ID.
    if let Some(edge) = ranked.get(POOL).map(|r| r.strength) {
        ranked.retain(|r| r.strength > edge);
    }
    Ok(ranked)
}

/// Fragments the seed links to or that link to it, all tied: a link is a
/// link.
fn by_links(
    conn: &Connection,
    filter: &Eligible,
    links_to: &[String],
    linked_from: Option<&str>,
) -> Result<Vec<Ranked>, ParcError> {
    if links_to.is_empty() && linked_from.is_none() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT f.id FROM fragments f
         WHERE (f.id IN (SELECT value FROM json_each(?3))
                OR f.id IN (SELECT source_id FROM fragment_links WHERE resolved_id = ?4))
           AND {ELIGIBLE}
         ORDER BY f.created_at DESC, f.id LIMIT {POOL}"
    ))?;
    let ids = stmt
        .query_map(
            rusqlite::params![
                filter.exclude,
                filter.as_of,
                serde_json::to_string(links_to)?,
                linked_from
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids
        .into_iter()
        .map(|id| Ranked {
            id,
            reasons: Reasons {
                linked: true,
                ..Reasons::default()
            },
            strength: 1.0,
        })
        .collect())
}

/// Fragments closest in meaning to the seed's passage, the closest
/// [`POOL`] as a ranking, and every one's similarity. Only fragments with an
/// up-to-date vector take part.
fn by_meaning(
    conn: &Connection,
    filter: &Eligible,
    model: &Model,
    passage: &str,
) -> Result<(Vec<Ranked>, HashMap<String, f32>), ParcError> {
    let seed = model.embed_cached(passage)?;
    let mut scored = embed::closeness(
        conn,
        model,
        &seed,
        ELIGIBLE,
        rusqlite::params![filter.exclude, filter.as_of],
    )?;
    let all: HashMap<String, f32> = scored.iter().cloned().collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    scored.truncate(POOL);
    let ranking = scored
        .into_iter()
        .map(|(id, s)| Ranked {
            id,
            reasons: Reasons {
                similarity: Some(s),
                ..Reasons::default()
            },
            strength: f64::from(s),
        })
        .collect();
    Ok((ranking, all))
}

/// Up to [`MAX_EXCERPTS`] lines of `body` with the most of `words`, in body
/// order; or its first line with text when none has any.
fn excerpts(body: &str, words: &[String]) -> Vec<Excerpt> {
    let wanted: HashSet<&str> = words.iter().map(String::as_str).collect();
    let body = body.strip_prefix('\n').unwrap_or(body);
    let mut first = None;
    let mut matched = Vec::new();
    for (i, raw) in body.split('\n').enumerate() {
        let text = raw.strip_suffix('\r').unwrap_or(raw);
        if first.is_none() && !text.trim().is_empty() {
            first = Some((i + 1, text));
        }
        if !wanted.is_empty() {
            let (ranges, distinct) = word_ranges(text, &wanted);
            if !ranges.is_empty() {
                matched.push((i + 1, distinct, ranges, text));
            }
        }
    }
    matched.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    matched.truncate(MAX_EXCERPTS);
    matched.sort_by_key(|m| m.0);
    if matched.is_empty() {
        return first
            .map(|(line, text)| clip(line, text, &[]))
            .into_iter()
            .collect();
    }
    matched
        .into_iter()
        .map(|(line, _, ranges, text)| clip(line, text, &ranges))
        .collect()
}

/// `[start, end)` character offsets of the words of `text` that are in
/// `wanted` (split and lowercased as [`content_words`] does), and how many
/// different words they are.
fn word_ranges(text: &str, wanted: &HashSet<&str>) -> (Vec<(usize, usize)>, usize) {
    let mut ranges = Vec::new();
    let mut distinct = HashSet::new();
    let mut start = None;
    let mut word = String::new();
    for (i, c) in text.chars().chain([' ']).enumerate() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
                word.clear();
            }
            word.push(c);
        } else if let Some(s) = start.take() {
            let lower = word.to_lowercase();
            if wanted.contains(lower.as_str()) {
                ranges.push((s, i));
                distinct.insert(lower);
            }
        }
    }
    (ranges, distinct.len())
}

/// The line as an excerpt: whole if short, otherwise about
/// [`EXCERPT_CHARS`] characters around its first match, cut between words.
fn clip(line: usize, text: &str, ranges: &[(usize, usize)]) -> Excerpt {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= EXCERPT_CHARS {
        return Excerpt {
            line,
            column: 1,
            text: text.to_string(),
            ranges: ranges.to_vec(),
            attachment: None,
            part: None,
            label: None,
        };
    }
    let (focus, focus_end) = ranges.first().copied().unwrap_or((0, 0));
    let end = (focus.saturating_sub(EXCERPT_CHARS / 4) + EXCERPT_CHARS).min(chars.len());
    let mut start = end - EXCERPT_CHARS;
    let mut end = end;
    if start > 0 {
        if let Some(k) = chars[start..focus.max(start)]
            .iter()
            .position(|c| c.is_whitespace())
        {
            start += k + 1;
        }
    }
    if end < chars.len() {
        let from = focus_end.clamp(start, end);
        if let Some(k) = chars[from..end].iter().rposition(|c| c.is_whitespace()) {
            end = from + k;
        }
    }
    Excerpt {
        line,
        column: start + 1,
        text: chars[start..end].iter().collect(),
        ranges: ranges
            .iter()
            .filter(|r| r.0 >= start && r.1 <= end)
            .map(|r| (r.0 - start, r.1 - start))
            .collect(),
        attachment: None,
        part: None,
        label: None,
    }
}

/// [`content_words`], up to [`MAX_QUERY_WORDS`] of them.
fn query_words(text: &str) -> Vec<String> {
    let mut words = content_words(text);
    words.truncate(MAX_QUERY_WORDS);
    words
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
    use std::path::{Path, PathBuf};

    #[test]
    fn clip_handles_match_extending_beyond_window() {
        let word = "ä".repeat(180);
        let line = format!("{} {} {}", "prefix ".repeat(30), word, "tail ".repeat(80));
        let (ranges, _) = word_ranges(&line, &HashSet::from([word.as_str()]));
        let excerpt = clip(3, &line, &ranges);
        assert_eq!(excerpt.line, 3);
        assert!(excerpt.text.contains("ää"));
        assert!(excerpt.text.chars().count() <= EXCERPT_CHARS);
    }

    fn vault() -> (tempfile::TempDir, PathBuf, Writer) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        crate::vault::init_vault(&vault).unwrap();
        let writer = Writer::new(&vault, ScriptRunner::default());
        (tmp, vault, writer)
    }

    fn note(writer: &mut Writer, title: &str, body: &str, tags: &[&str], links: &[&str]) -> String {
        let vault = writer.vault().to_path_buf();
        let config = crate::config::load_config(&vault).unwrap();
        let schemas = crate::schema::load_schemas(&vault).unwrap();
        let mut f =
            crate::fragment::new_fragment("note", title, schemas.resolve("note").unwrap(), &config);
        f.body = body.to_string();
        f.tags = tags.iter().map(|s| s.to_string()).collect();
        f.links = links.iter().map(|s| s.to_string()).collect();
        writer.create(f).unwrap().id
    }

    fn found(vault: &Path, seed: Seed) -> Vec<(String, Reasons)> {
        let conn = crate::index::open_index(vault).unwrap();
        let query = RecallQuery {
            seed,
            as_of: None,
            limit: 10,
        };
        let mut found: Vec<_> = recall(&conn, &query, None)
            .unwrap()
            .into_iter()
            .map(|c| (c.id, c.reasons))
            .collect();
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found
    }

    fn draft(text: &str) -> Seed {
        Seed::Draft {
            text: text.into(),
            tags: Vec::new(),
            id: None,
        }
    }

    fn linked() -> Reasons {
        Reasons {
            linked: true,
            ..Reasons::default()
        }
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
        assert!(!r.is_evidence(Bars::NONE));
        r.shared_words.push("volunteer".into());
        assert!(r.is_evidence(Bars::NONE));
        assert!(linked().is_evidence(Bars::NONE));
    }

    #[test]
    fn meaning_is_evidence_and_can_veto_shared_words() {
        let bars = Bars {
            similar: 0.5,
            unrelated: 0.2,
        };
        let close = |s, words: &[&str]| Reasons {
            similarity: Some(s),
            shared_words: words.iter().map(|w| w.to_string()).collect(),
            ..Reasons::default()
        };
        assert!(close(0.5, &[]).is_evidence(bars));
        assert!(!close(0.49, &[]).is_evidence(bars));
        assert!(close(0.2, &["pitch", "thursday"]).is_evidence(bars));
        assert!(!close(0.19, &["pitch", "thursday"]).is_evidence(bars));
        let tagged = Reasons {
            shared_tags: vec!["work".into()],
            ..close(0.0, &[])
        };
        assert!(tagged.is_evidence(bars), "a tag is never vetoed");
    }

    #[test]
    fn links_count_in_both_directions_without_shared_words() {
        let (_tmp, vault, mut w) = vault();
        let old = note(&mut w, "Ferry desk", "Route cards", &[], &[]);
        let seed = note(&mut w, "Orientation", "Question slips", &[], &[&old]);
        let later = note(&mut w, "Follow-up", "Went fine", &[], &[&seed]);
        note(&mut w, "Unrelated", "Kitchen timer", &[], &[]);

        let mut want = vec![(old, linked()), (later, linked())];
        want.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(found(&vault, Seed::Fragment(seed)), want);
    }

    #[test]
    fn unknown_seed_is_an_error() {
        let (_tmp, vault, _w) = vault();
        let conn = crate::index::open_index(&vault).unwrap();
        for seed in [
            Seed::Fragment("01JQ7V3XKP5GQZ2N8R6T1WBMVH".into()),
            Seed::Draft {
                text: "words".into(),
                tags: Vec::new(),
                id: Some("01JQ7V3XKP5GQZ2N8R6T1WBMVH".into()),
            },
        ] {
            let query = RecallQuery {
                seed,
                as_of: None,
                limit: 5,
            };
            assert!(matches!(
                recall(&conn, &query, None),
                Err(ParcError::FragmentNotFound(_))
            ));
        }
    }

    #[test]
    fn a_draft_finds_by_words_inline_tags_and_links() {
        let (_tmp, vault, mut w) = vault();
        let ferry = note(
            &mut w,
            "Ferry desk",
            "Visitors marked route cards in private",
            &[],
            &[],
        );
        let tagged = note(&mut w, "Spring fair", "Cake table", &["volunteering"], &[]);
        let titled = note(&mut w, "Projector checklist", "Spare bulb", &[], &[]);
        note(&mut w, "Kitchen timer", "Broken again", &[], &[]);

        let text = "Orientation\nMaybe private route cards, like at the desk? \
                    #volunteering [[Projector checklist]]";
        let got = found(&vault, draft(text));
        let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
        let mut want = vec![ferry.as_str(), tagged.as_str(), titled.as_str()];
        want.sort();
        assert_eq!(ids, want);
        let reasons = |id: &str| &got.iter().find(|(i, _)| i == id).unwrap().1;
        assert_eq!(
            reasons(&ferry).shared_words,
            vec!["cards", "desk", "private", "route"]
        );
        assert_eq!(reasons(&tagged).shared_tags, vec!["volunteering"]);
        assert!(reasons(&titled).linked);
    }

    #[test]
    fn a_draft_of_a_saved_fragment_leaves_it_out_and_keeps_its_links() {
        let (_tmp, vault, mut w) = vault();
        let target = note(&mut w, "Ferry desk", "Unrelated words", &[], &[]);
        let saved = note(
            &mut w,
            "Route cards",
            "Route cards in private",
            &[],
            &[&target],
        );
        let backlink = note(&mut w, "Follow-up", "Other words", &[], &[&saved]);
        let similar = note(&mut w, "Cards", "Route cards again, private", &[], &[]);

        let got = found(
            &vault,
            Seed::Draft {
                text: "Route cards\nRoute cards in private, rewritten".into(),
                tags: Vec::new(),
                id: Some(saved.clone()),
            },
        );
        let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
        let mut want = vec![target.as_str(), backlink.as_str(), similar.as_str()];
        want.sort();
        assert_eq!(ids, want, "the saved fragment itself is left out");
    }

    #[test]
    fn context_quotes_the_lines_with_the_most_shared_words() {
        let (_tmp, vault, mut w) = vault();
        let body = "Intro line.\nThe queue was watching.\nGive each visitor a route card.\n\
                    Mark it in private: route card, private.\nOutro.";
        let id = note(&mut w, "Ferry desk", body, &[], &[]);
        let conn = crate::index::open_index(&vault).unwrap();
        let ctx = context(
            &conn,
            &RecallQuery {
                seed: draft("A route card to mark in private, nobody in the queue"),
                as_of: None,
                limit: 5,
            },
            None,
        )
        .unwrap();
        assert!(!ctx.truncated);
        assert_eq!(ctx.sources.len(), 1);
        let source = &ctx.sources[0];
        assert_eq!(source.id, id);
        assert_eq!(source.title, "Ferry desk");
        assert_eq!(
            source.revision.as_deref(),
            Some(crate::fragment::revision(&vault, &id).unwrap().as_str())
        );
        let lines: Vec<(usize, &str)> = source
            .excerpts
            .iter()
            .map(|e| (e.line, e.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                (3, "Give each visitor a route card."),
                (4, "Mark it in private: route card, private."),
            ]
        );
        let marked: Vec<&str> = source.excerpts[1]
            .ranges
            .iter()
            .map(|&(s, e)| &source.excerpts[1].text[s..e])
            .collect();
        assert_eq!(marked, vec!["Mark", "private", "route", "card", "private"]);
    }

    #[test]
    fn context_is_capped_and_says_so() {
        let (_tmp, vault, mut w) = vault();
        for i in 0..4 {
            note(&mut w, &format!("Ferry desk {i}"), "route cards", &[], &[]);
        }
        let conn = crate::index::open_index(&vault).unwrap();
        let ctx = |limit| {
            context(
                &conn,
                &RecallQuery {
                    seed: draft("ferry desk route cards"),
                    as_of: None,
                    limit,
                },
                None,
            )
            .unwrap()
        };
        assert_eq!((ctx(3).sources.len(), ctx(3).truncated), (3, true));
        assert_eq!((ctx(4).sources.len(), ctx(4).truncated), (4, false));
    }

    #[test]
    fn a_long_line_is_cut_around_its_first_match_between_words() {
        let line = format!("{} ferry desk {}", "word ".repeat(80), "tail ".repeat(80));
        let wanted: HashSet<&str> = ["ferry", "desk"].into();
        let (ranges, distinct) = word_ranges(&line, &wanted);
        assert_eq!(distinct, 2);
        let e = clip(7, &line, &ranges);
        assert!(e.text.chars().count() <= EXCERPT_CHARS);
        assert!(
            e.text.starts_with("word ") && e.text.ends_with(" tail"),
            "{:?}",
            e.text
        );
        let chars: Vec<char> = line.chars().collect();
        let at: String = chars[e.column - 1..e.column - 1 + e.text.chars().count()]
            .iter()
            .collect();
        assert_eq!(at, e.text, "column locates the excerpt in the line");
        let marked: Vec<String> = e
            .ranges
            .iter()
            .map(|&(s, end)| e.text.chars().skip(s).take(end - s).collect())
            .collect();
        assert_eq!(marked, vec!["ferry", "desk"]);
    }

    #[test]
    fn with_no_shared_word_the_first_line_with_text_is_quoted() {
        assert_eq!(
            excerpts("\n\n  \nFirst words.\nSecond.", &[]),
            vec![Excerpt {
                line: 3,
                column: 1,
                text: "First words.".into(),
                ranges: vec![],
                attachment: None,
                part: None,
                label: None,
            }]
        );
    }
}
