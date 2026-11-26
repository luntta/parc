//! The meaning model's tokenizer: the part of Hugging Face's `tokenizer.json`
//! format these models use (SentencePiece's precompiled normalizer, then
//! Metaspace, then a Unigram vocabulary), kept small. The `tokenizers` crate
//! holds this vocabulary in about 280 MB; here it takes about 20, which
//! counts with a server per vault. `tests/tokenizer.rs` checks that both give
//! the same tokens.
//!
//! Special tokens written in a text (`<s>`, `<mask>`) are read as plain
//! characters, never as the model's own markers.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;
use spm_precompiled::Precompiled;

use crate::error::ParcError;

/// Added to the lowest piece score for a character no piece covers, as in
/// SentencePiece.
const UNK_PENALTY: f64 = 10.0;

/// Marks a word's start, standing in for the space before it.
const SPACE: char = '\u{2581}';

pub struct Tokenizer {
    normalizer: Precompiled,
    /// Runs of spaces become one after normalizing (E5, Granite).
    collapse_spaces: bool,
    /// Split at whitespace before marking word starts (MiniLM); otherwise
    /// every space becomes a word start, as in SentencePiece.
    whitespace_split: bool,
    /// Piece → ID, for every piece but the special tokens.
    pieces: HashMap<Box<str>, u32>,
    /// Score by ID.
    scores: Vec<f64>,
    /// Longest piece, in bytes.
    longest: usize,
    unk: u32,
    unk_score: f64,
    /// The model's own tokens before and after a text (`<s>`, `</s>`).
    pub start: Vec<u32>,
    pub end: Vec<u32>,
}

#[derive(Deserialize)]
struct File {
    normalizer: Value,
    pre_tokenizer: Value,
    post_processor: Value,
    model: Unigram,
    #[serde(default)]
    added_tokens: Vec<Added>,
}

#[derive(Deserialize)]
struct Unigram {
    #[serde(rename = "type")]
    kind: String,
    unk_id: Option<u32>,
    vocab: Vec<(String, f64)>,
    #[serde(default)]
    byte_fallback: bool,
}

#[derive(Deserialize)]
struct Added {
    id: u32,
    special: bool,
}

fn unsupported(what: &str) -> ParcError {
    ParcError::ModelError(format!("tokenizer.json: unsupported {what}"))
}

impl Tokenizer {
    pub fn from_file(path: &Path) -> Result<Tokenizer, ParcError> {
        let file: File = serde_json::from_slice(&std::fs::read(path)?)?;
        let model = file.model;
        if model.kind != "Unigram" || model.byte_fallback {
            return Err(unsupported("model"));
        }
        let unk = model
            .unk_id
            .ok_or_else(|| unsupported("model without <unk>"))?;

        let (normalizer, collapse_spaces) = match file.normalizer["type"].as_str() {
            Some("Precompiled") => (precompiled(&file.normalizer)?, false),
            Some("Sequence") => {
                match file.normalizer["normalizers"].as_array().map(Vec::as_slice) {
                    Some([first, second])
                        if second["type"] == "Replace"
                            && second["pattern"]["Regex"] == " {2,}"
                            && second["content"] == " " =>
                    {
                        (precompiled(first)?, true)
                    }
                    _ => return Err(unsupported("normalizer")),
                }
            }
            _ => return Err(unsupported("normalizer")),
        };

        let whitespace_split = match file.pre_tokenizer["type"].as_str() {
            Some("Metaspace") => {
                check_metaspace(&file.pre_tokenizer)?;
                false
            }
            Some("Sequence") => match file.pre_tokenizer["pretokenizers"]
                .as_array()
                .map(Vec::as_slice)
            {
                Some([split, meta])
                    if split["type"] == "WhitespaceSplit" && meta["type"] == "Metaspace" =>
                {
                    check_metaspace(meta)?;
                    true
                }
                _ => return Err(unsupported("pre-tokenizer")),
            },
            _ => return Err(unsupported("pre-tokenizer")),
        };

        let post = &file.post_processor;
        if post["type"] != "TemplateProcessing" {
            return Err(unsupported("post-processor"));
        }
        let (mut start, mut end, mut seen) = (Vec::new(), Vec::new(), false);
        for item in post["single"]
            .as_array()
            .ok_or_else(|| unsupported("template"))?
        {
            if item.get("Sequence").is_some() {
                seen = true;
                continue;
            }
            let name = item["SpecialToken"]["id"]
                .as_str()
                .ok_or_else(|| unsupported("template"))?;
            let ids = post["special_tokens"][name]["ids"]
                .as_array()
                .ok_or_else(|| unsupported("template"))?;
            for id in ids {
                let id = id.as_u64().ok_or_else(|| unsupported("template"))? as u32;
                if seen {
                    end.push(id)
                } else {
                    start.push(id)
                }
            }
        }

        let special: std::collections::HashSet<u32> = file
            .added_tokens
            .iter()
            .filter(|a| a.special)
            .map(|a| a.id)
            .collect();
        let min = model
            .vocab
            .iter()
            .map(|(_, s)| *s)
            .fold(f64::INFINITY, f64::min);
        let mut scores = Vec::with_capacity(model.vocab.len());
        let mut pieces = HashMap::with_capacity(model.vocab.len());
        let mut longest = 0;
        for (id, (piece, score)) in model.vocab.into_iter().enumerate() {
            scores.push(score);
            if !special.contains(&(id as u32)) {
                longest = longest.max(piece.len());
                pieces.insert(piece.into_boxed_str(), id as u32);
            }
        }
        if unk as usize >= scores.len() {
            return Err(unsupported("<unk> ID"));
        }
        Ok(Tokenizer {
            normalizer,
            collapse_spaces,
            whitespace_split,
            pieces,
            scores,
            longest,
            unk,
            unk_score: min - UNK_PENALTY,
            start,
            end,
        })
    }

    /// The text's token IDs, without the start and end tokens.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut normal = self.normalizer.normalize_string(text);
        if self.collapse_spaces {
            let mut out = String::with_capacity(normal.len());
            for c in normal.chars() {
                if !(c == ' ' && out.ends_with(' ')) {
                    out.push(c);
                }
            }
            normal = out;
        }
        let mut ids = Vec::new();
        if self.whitespace_split {
            for word in normal.split(char::is_whitespace).filter(|w| !w.is_empty()) {
                self.words(word, &mut ids);
            }
        } else if !normal.is_empty() {
            self.words(&normal, &mut ids);
        }
        ids
    }

    /// Metaspace: spaces become word starts, one is put first, and the text
    /// is cut before each.
    fn words(&self, text: &str, ids: &mut Vec<u32>) {
        let mut marked: String = text.replace(' ', &SPACE.to_string());
        if !marked.starts_with(SPACE) {
            marked.insert(0, SPACE);
        }
        let mut from = 0;
        for (at, _) in marked.match_indices(SPACE) {
            if at > from {
                self.unigram(&marked[from..at], ids);
                from = at;
            }
        }
        self.unigram(&marked[from..], ids);
    }

    /// The most likely pieces (Viterbi), as SentencePiece and `tokenizers`
    /// choose them: ties go to the shorter piece found first, a character
    /// no piece covers is `<unk>`, and neighbouring `<unk>`s are one.
    fn unigram(&self, word: &str, ids: &mut Vec<u32>) {
        #[derive(Clone, Copy)]
        struct Best {
            score: f64,
            from: Option<usize>,
            id: u32,
        }
        let n = word.len();
        let mut best = vec![
            Best {
                score: 0.0,
                from: None,
                id: 0
            };
            n + 1
        ];
        let mut at = 0;
        while at < n {
            let here = best[at].score;
            let first = word[at..].chars().next().map_or(1, char::len_utf8);
            let mut single = false;
            for (offset, c) in word[at..].char_indices() {
                let to = at + offset + c.len_utf8();
                if to - at > self.longest {
                    break;
                }
                if let Some(&id) = self.pieces.get(&word[at..to]) {
                    let score = here + self.scores[id as usize];
                    let node = &mut best[to];
                    if node.from.is_none() || score > node.score {
                        *node = Best {
                            score,
                            from: Some(at),
                            id,
                        };
                    }
                    single |= to - at == first;
                }
            }
            if !single {
                let score = here + self.unk_score;
                let node = &mut best[at + first];
                if node.from.is_none() || score > node.score {
                    *node = Best {
                        score,
                        from: Some(at),
                        id: self.unk,
                    };
                }
            }
            at += first;
        }
        // Back from the end; neighbouring <unk>s become one token, looked up
        // again as a whole.
        let mut out = Vec::new();
        let (mut to, mut unk_from) = (n, None::<usize>);
        let flush = |out: &mut Vec<u32>, from: usize, to: usize| {
            out.push(
                self.pieces
                    .get(&word[from..to])
                    .copied()
                    .unwrap_or(self.unk),
            );
        };
        let mut unk_to = n;
        while to > 0 {
            let node = best[to];
            let from = node.from.expect("every position is reachable");
            if node.id == self.unk {
                if unk_from.is_none() {
                    unk_to = to;
                }
                unk_from = Some(from);
            } else {
                if let Some(f) = unk_from.take() {
                    flush(&mut out, f, unk_to);
                }
                out.push(node.id);
            }
            to = from;
        }
        if let Some(f) = unk_from {
            flush(&mut out, f, unk_to);
        }
        out.reverse();
        ids.extend(out);
    }
}

fn precompiled(value: &Value) -> Result<Precompiled, ParcError> {
    Precompiled::deserialize(value)
        .map_err(|e| ParcError::ModelError(format!("tokenizer.json: {e}")))
}

/// Only the Metaspace these models use: `▁`, put before the first word, and
/// cut before each.
fn check_metaspace(meta: &Value) -> Result<(), ParcError> {
    let prepends = meta["add_prefix_space"] == true || meta["prepend_scheme"] == "always";
    let splits = meta.get("split").is_none_or(|s| s == true);
    if meta["replacement"] != "\u{2581}" || !prepends || !splits {
        return Err(unsupported("Metaspace"));
    }
    Ok(())
}
