//! Meaning: a small multilingual text encoder, so that recall can find a note
//! that says the same thing in other words, or in another language.
//!
//! The encoder is a BERT-style sentence model ([`MODEL`]) run here in plain
//! Rust: its tokenizer ([`Tokenizer`]), then the transformer, with `gemm` for the
//! matrix products. No outside program and no native library is involved.
//!
//! Its files are downloaded once per computer (by the desktop app, on request)
//! into [`model_dir`]. That place is never one a vault names: a vault's
//! `config.yml` travels with it and is not trusted. The weights file is
//! memory-mapped and its word-embedding table, most of the file, is read in
//! place, so several parc processes share one copy in the page cache.
//!
//! Vectors live in the vault's `index.db`, each with the revision of the file
//! it was computed from. They are filled in by [`fill`], never by the index
//! sync on open, which must stay fast. Ranking uses whichever are up to date.

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use gemm::Parallelism;
use memmap2::Mmap;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::error::ParcError;

mod tokenizer;
pub use tokenizer::Tokenizer;

/// One file of a model, as published at its pinned revision.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelFile {
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// A model parc knows how to run, pinned to one revision of its repository.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelSpec {
    /// Also the name of its folder under [`models_dir`].
    pub id: &'static str,
    pub label: &'static str,
    /// Hugging Face repository.
    pub repo: &'static str,
    /// The commit the files are pinned to.
    pub revision: &'static str,
    pub license: &'static str,
    pub files: &'static [ModelFile],
    /// Cosine similarity from which two texts count as meaning the same
    /// thing, so that recall returns one for the other with no other reason.
    /// Each model spreads similarities differently; this one is calibrated on
    /// the recall benchmark (`tests/fixtures/recall`).
    pub similar: f32,
    /// Cosine similarity below which two texts count as unrelated, so that
    /// words they happen to share are not evidence on their own.
    pub unrelated: f32,
    /// Most tokens the model reads at once, as it was trained. A longer text
    /// is read in overlapping windows of this many, a vector for each.
    pub window: usize,
    /// Put before every text, as the model was trained to expect.
    #[serde(skip)]
    prefix: &'static str,
    #[serde(skip)]
    pooling: Pooling,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pooling {
    /// The mean of every token's output.
    Mean,
    /// The output of the first token.
    Cls,
}

impl ModelSpec {
    /// Where a file is downloaded from: the pinned revision, never `main`.
    pub fn url(&self, file: &ModelFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, file.name
        )
    }

    /// The download size of all its files.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// What the index labels this model's vectors with. Vectors made by
    /// another model (or another revision) are never compared with them.
    pub fn key(&self) -> String {
        format!("{}@{}", self.id, &self.revision[..12])
    }
}

/// `intfloat/multilingual-e5-small`: about 100 languages, Finnish among them.
/// "query: " is its prefix for symmetric tasks, where both texts are alike.
pub const E5_SMALL: ModelSpec = ModelSpec {
    id: "multilingual-e5-small",
    label: "Multilingual E5 small",
    repo: "intfloat/multilingual-e5-small",
    revision: "614241f622f53c4eeff9890bdc4f31cfecc418b3",
    license: "MIT",
    files: &[
        ModelFile {
            name: "config.json",
            size: 655,
            sha256: "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959",
        },
        ModelFile {
            name: "tokenizer.json",
            size: 17_082_730,
            sha256: "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39",
        },
        ModelFile {
            name: "model.safetensors",
            size: 470_641_600,
            sha256: "1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477",
        },
    ],
    similar: 0.87,
    unrelated: 0.82,
    window: 512,
    prefix: "query: ",
    pooling: Pooling::Mean,
};

/// `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2`.
pub const MINILM_L12: ModelSpec = ModelSpec {
    id: "paraphrase-multilingual-minilm-l12-v2",
    label: "Paraphrase multilingual MiniLM L12",
    repo: "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2",
    revision: "e8f8c211226b894fcb81acc59f3b34ba3efd5f42",
    license: "Apache-2.0",
    files: &[
        ModelFile {
            name: "config.json",
            size: 645,
            sha256: "6300193cb75e01cf80c96decef7187dfb33094d97cc1490b7ead6ff134476e4e",
        },
        ModelFile {
            name: "tokenizer.json",
            size: 9_081_518,
            sha256: "2c3387be76557bd40970cec13153b3bbf80407865484b209e655e5e4729076b8",
        },
        ModelFile {
            name: "model.safetensors",
            size: 470_641_600,
            sha256: "eaa086f0ffee582aeb45b36e34cdd1fe2d6de2bef61f8a559a1bbc9bd955917b",
        },
    ],
    similar: 0.47,
    unrelated: 0.2,
    window: 128,
    prefix: "",
    pooling: Pooling::Mean,
};

/// `ibm-granite/granite-embedding-107m-multilingual`: six layers, stored in
/// bfloat16, so half the download and half the work.
pub const GRANITE_107M: ModelSpec = ModelSpec {
    id: "granite-embedding-107m-multilingual",
    label: "Granite Embedding 107M multilingual",
    repo: "ibm-granite/granite-embedding-107m-multilingual",
    revision: "d6cffd338414d6a1c1f5decfad5fec62eebc90d5",
    license: "Apache-2.0",
    files: &[
        ModelFile {
            name: "config.json",
            size: 697,
            sha256: "aa3b6a5d1d6efeacdb844fb8ba773556463af2195003ad5e70a6f014d11be9cf",
        },
        ModelFile {
            name: "tokenizer.json",
            size: 9_081_382,
            sha256: "7fc9e475f2ac473f070052d92b0c46ec2b91d9cd672935feb6fd837a65d33f78",
        },
        ModelFile {
            name: "model.safetensors",
            size: 214_000_232,
            sha256: "ae6f20cc5dc433041118c900b652d87532e1335dade78c37765f0b94d1161283",
        },
    ],
    similar: 0.71,
    unrelated: 0.6,
    window: 512,
    prefix: "",
    pooling: Pooling::Cls,
};

/// Every model this build can run.
pub const CATALOG: &[ModelSpec] = &[E5_SMALL, MINILM_L12, GRANITE_107M];

/// The model recall uses.
pub const MODEL: &ModelSpec = &MINILM_L12;

/// Most windows read from one text: a note's first two thousand words or so.
/// The rest of a longer one counts for words, tags and links only.
const MAX_WINDOWS: usize = 16;

/// Longest text, in bytes, handed to the tokenizer: more than
/// [`MAX_WINDOWS`] windows hold in any language, so the cut only keeps a huge
/// body from costing time for text that would be dropped anyway.
const MAX_INPUT_BYTES: usize = 32 * 1024;

/// The per-user folder that holds downloaded models: `$PARC_MODELS_DIR`, or
/// the platform's data folder (`$XDG_DATA_HOME/parc/models`, by default
/// `~/.local/share/parc/models`; `~/Library/Application Support/parc/models`
/// on macOS; `%APPDATA%\parc\models` on Windows).
pub fn models_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if let Some(dir) = env("PARC_MODELS_DIR") {
        return Some(dir);
    }
    let base = if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else if cfg!(windows) {
        env("APPDATA")
    } else {
        env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))
    };
    Some(base?.join("parc").join("models"))
}

/// Where `spec`'s files go: its own folder under [`models_dir`].
pub fn model_dir(spec: &ModelSpec) -> Option<PathBuf> {
    Some(models_dir()?.join(spec.id))
}

/// Whether every file of `spec` is in `dir` with its published size. Hashes
/// are checked once, by whatever downloaded the files.
pub fn is_installed(spec: &ModelSpec, dir: &Path) -> bool {
    spec.files.iter().all(|f| {
        std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.is_file() && m.len() == f.size)
    })
}

/// Whether [`shared`] may load the model; see [`set_enabled`].
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turn the meaning model off (or back on) for this process: [`shared`] then
/// returns `None`. For tests, whose results must not depend on whether the
/// model is installed on the computer running them.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// The process's [`MODEL`], once [`shared`] has tried to load it.
static SHARED: Mutex<Option<Result<Arc<Model>, String>>> = Mutex::new(None);

/// [`MODEL`], loaded once per process from [`model_dir`]. `None` while it
/// isn't installed, which is checked again on every call, so a model
/// downloaded while a long-running process waits is picked up. A model that
/// fails to load keeps failing, with the same error, until the process ends.
pub fn shared() -> Result<Option<Arc<Model>>, ParcError> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let mut slot = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        let Some(dir) = model_dir(MODEL).filter(|d| is_installed(MODEL, d)) else {
            return Ok(None);
        };
        *slot = Some(
            Model::load(MODEL, &dir)
                .map(Arc::new)
                .map_err(|e| e.to_string()),
        );
    }
    match slot.as_ref() {
        Some(Ok(model)) => Ok(Some(Arc::clone(model))),
        Some(Err(e)) => Err(ParcError::ModelError(e.clone())),
        None => Ok(None),
    }
}

/// Whether [`shared`] has loaded the model in this process (not counting a
/// load in progress).
pub fn is_loaded() -> bool {
    SHARED
        .try_lock()
        .is_ok_and(|slot| matches!(slot.as_ref(), Some(Ok(_))))
}

/// Why [`shared`] failed to load the model, if it tried and did.
pub fn load_error() -> Option<String> {
    let slot = SHARED.try_lock().ok()?;
    match slot.as_ref() {
        Some(Err(e)) => Some(e.clone()),
        _ => None,
    }
}

fn model_error(what: impl std::fmt::Display) -> ParcError {
    ParcError::ModelError(what.to_string())
}

// ── The model ──────────────────────────────────────────────────────────

/// The architecture, from the model's `config.json`.
#[derive(Debug, Deserialize)]
struct Config {
    model_type: String,
    hidden_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    intermediate_size: usize,
    layer_norm_eps: f32,
    max_position_embeddings: usize,
    pad_token_id: usize,
    hidden_act: String,
}

/// A loaded encoder. Cheap to share between threads.
pub struct Model {
    spec: &'static ModelSpec,
    tokenizer: Tokenizer,
    words: Tensors,
    word_table: Entry,
    positions: Vec<f32>,
    /// The first token type's embedding, the only one a single text uses.
    token_type: Vec<f32>,
    norm: Norm,
    layers: Vec<Layer>,
    hidden: usize,
    heads: usize,
    eps: f32,
    /// Position of the first token: XLM-RoBERTa counts from the padding ID
    /// plus one, BERT from zero.
    position_offset: usize,
    /// Tokens per window, the model's own start and end tokens included.
    window: usize,
    /// The tokens that open and close every window, and the spec's prefix.
    start: Vec<u32>,
    end: Vec<u32>,
    prefix: Vec<u32>,
    /// The last texts [`Model::embed_cached`] embedded: a draft is often
    /// asked about again unchanged (the vault changed, the caret moved).
    recent: Mutex<VecDeque<(String, Windows)>>,
}

/// A text's vectors, one per window, shared.
pub type Windows = Arc<Vec<Vec<f32>>>;

/// How many texts [`Model::embed_cached`] remembers.
const RECENT: usize = 8;

struct Layer {
    query: Linear,
    key: Linear,
    value: Linear,
    attention_out: Linear,
    attention_norm: Norm,
    up: Linear,
    down: Linear,
    out_norm: Norm,
}

/// `y = x Wᵀ + b`, with `W` as stored: `outputs × inputs`, row-major.
struct Linear {
    weight: Vec<f32>,
    bias: Vec<f32>,
    inputs: usize,
    outputs: usize,
}

struct Norm {
    weight: Vec<f32>,
    bias: Vec<f32>,
}

impl Model {
    /// Load `spec` from `dir`, which must hold its files.
    pub fn load(spec: &'static ModelSpec, dir: &Path) -> Result<Model, ParcError> {
        let config: Config = serde_json::from_slice(&std::fs::read(dir.join("config.json"))?)?;
        if !matches!(config.model_type.as_str(), "bert" | "xlm-roberta") {
            return Err(model_error(format!(
                "unsupported architecture {:?}",
                config.model_type
            )));
        }
        if config.hidden_act != "gelu" {
            return Err(model_error(format!(
                "unsupported activation {:?}",
                config.hidden_act
            )));
        }
        let h = config.hidden_size;
        if config.num_attention_heads == 0 || !h.is_multiple_of(config.num_attention_heads) {
            return Err(model_error("hidden size is not a multiple of the heads"));
        }
        let position_offset = match config.model_type.as_str() {
            "xlm-roberta" => config.pad_token_id + 1,
            _ => 0,
        };
        let window = config
            .max_position_embeddings
            .saturating_sub(position_offset)
            .min(spec.window);

        let tokenizer = Tokenizer::from_file(&dir.join("tokenizer.json"))?;
        // What the model's own template puts around a text: `<s>` … `</s>`.
        let (start, end) = (tokenizer.start.clone(), tokenizer.end.clone());
        // Without its trailing space, which would be a token of its own: the
        // text that follows starts with a word boundary anyway.
        let prefix_ids = tokenizer.encode(spec.prefix.trim_end());
        if window < start.len() + end.len() + prefix_ids.len() + 8 {
            return Err(model_error("too few positions"));
        }

        let t = Tensors::open(&dir.join("model.safetensors"))?;
        // Some exports prefix every name with the model's own ("bert.").
        let prefix = if t.entries.contains_key("embeddings.word_embeddings.weight") {
            ""
        } else if t
            .entries
            .contains_key("bert.embeddings.word_embeddings.weight")
        {
            "bert."
        } else if t
            .entries
            .contains_key("roberta.embeddings.word_embeddings.weight")
        {
            "roberta."
        } else {
            return Err(model_error("no word embeddings in the weights"));
        };
        let name = |n: &str| format!("{prefix}{n}");
        let word_table = t.entry(&name("embeddings.word_embeddings.weight"), None, Some(h))?;
        let positions = t.load(
            &name("embeddings.position_embeddings.weight"),
            &[config.max_position_embeddings, h],
        )?;
        let types = t.entry(
            &name("embeddings.token_type_embeddings.weight"),
            None,
            Some(h),
        )?;
        let mut token_type = vec![0.0; h];
        t.row(&types, 0, &mut token_type)?;
        let norm = |t: &Tensors, n: &str| -> Result<Norm, ParcError> {
            Ok(Norm {
                weight: t.load(&name(&format!("{n}.weight")), &[h])?,
                bias: t.load(&name(&format!("{n}.bias")), &[h])?,
            })
        };
        let linear = |t: &Tensors, n: &str, inputs, outputs| -> Result<Linear, ParcError> {
            Ok(Linear {
                weight: t.load(&name(&format!("{n}.weight")), &[outputs, inputs])?,
                bias: t.load(&name(&format!("{n}.bias")), &[outputs])?,
                inputs,
                outputs,
            })
        };
        let i = config.intermediate_size;
        let layers = (0..config.num_hidden_layers)
            .map(|l| {
                let p = format!("encoder.layer.{l}");
                Ok(Layer {
                    query: linear(&t, &format!("{p}.attention.self.query"), h, h)?,
                    key: linear(&t, &format!("{p}.attention.self.key"), h, h)?,
                    value: linear(&t, &format!("{p}.attention.self.value"), h, h)?,
                    attention_out: linear(&t, &format!("{p}.attention.output.dense"), h, h)?,
                    attention_norm: norm(&t, &format!("{p}.attention.output.LayerNorm"))?,
                    up: linear(&t, &format!("{p}.intermediate.dense"), h, i)?,
                    down: linear(&t, &format!("{p}.output.dense"), i, h)?,
                    out_norm: norm(&t, &format!("{p}.output.LayerNorm"))?,
                })
            })
            .collect::<Result<Vec<_>, ParcError>>()?;
        Ok(Model {
            spec,
            tokenizer,
            norm: norm(&t, "embeddings.LayerNorm")?,
            words: t,
            word_table,
            positions,
            token_type,
            layers,
            hidden: h,
            heads: config.num_attention_heads,
            eps: config.layer_norm_eps,
            position_offset,
            window,
            start,
            end,
            prefix: prefix_ids,
            recent: Mutex::new(VecDeque::with_capacity(RECENT)),
        })
    }

    pub fn spec(&self) -> &'static ModelSpec {
        self.spec
    }

    /// Length of the vectors.
    pub fn dims(&self) -> usize {
        self.hidden
    }

    /// The text's vectors, one per window of [`ModelSpec::window`] tokens
    /// (each overlapping the last by a quarter), at most [`MAX_WINDOWS`], all
    /// of unit length: texts that mean the same thing point the same way,
    /// whatever their words or language. A short text has one.
    pub fn embed(&self, text: &str) -> Result<Vec<Vec<f32>>, ParcError> {
        self.embed_with(text, Parallelism::None)
    }

    fn embed_with(&self, text: &str, par: Parallelism) -> Result<Vec<Vec<f32>>, ParcError> {
        Ok(self
            .windows(text)?
            .iter()
            .map(|ids| self.forward(ids, par))
            .collect())
    }

    /// [`Model::embed`] for someone waiting: faster on a multi-core
    /// computer, and remembering the last few texts.
    pub fn embed_cached(&self, text: &str) -> Result<Windows, ParcError> {
        let recent = || self.recent.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, v)) = recent().iter().find(|(t, _)| t == text) {
            return Ok(Arc::clone(v));
        }
        let v = Arc::new(self.embed_with(text, Parallelism::Rayon(GEMM_THREADS))?);
        let mut r = recent();
        if r.len() == RECENT {
            r.pop_front();
        }
        r.push_back((text.to_string(), Arc::clone(&v)));
        Ok(v)
    }

    fn windows(&self, text: &str) -> Result<Vec<Vec<u32>>, ParcError> {
        let mut cut = text.len().min(MAX_INPUT_BYTES);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let body = self.tokenizer.encode(&text[..cut]);
        let room = self.window - self.start.len() - self.end.len() - self.prefix.len();
        let step = room - room / 4;
        let mut windows = Vec::new();
        let mut from = 0;
        loop {
            let to = (from + room).min(body.len());
            let mut ids = Vec::with_capacity(self.window);
            ids.extend(&self.start);
            ids.extend(&self.prefix);
            ids.extend(&body[from..to]);
            ids.extend(&self.end);
            windows.push(ids);
            if to == body.len() || windows.len() == MAX_WINDOWS {
                return Ok(windows);
            }
            from += step;
        }
    }

    fn forward(&self, ids: &[u32], par: Parallelism) -> Vec<f32> {
        let n = ids.len();
        let h = self.hidden;
        let mut x = vec![0.0f32; n * h];
        for (t, &id) in ids.iter().enumerate() {
            let row = &mut x[t * h..(t + 1) * h];
            // A token ID the table doesn't have is left as zeros; the
            // tokenizer that came with the model never produces one.
            let _ = self.words.row(&self.word_table, id as usize, row);
            let pos = &self.positions[(t + self.position_offset) * h..][..h];
            for ((v, p), ty) in row.iter_mut().zip(pos).zip(&self.token_type) {
                *v += p + ty;
            }
        }
        layer_norm(&mut x, h, &self.norm, self.eps);

        let d = h / self.heads;
        let scale = 1.0 / (d as f32).sqrt();
        let intermediate = self.layers.first().map_or(0, |l| l.up.outputs);
        let mut q = vec![0.0; n * h];
        let mut k = vec![0.0; n * h];
        let mut v = vec![0.0; n * h];
        let mut ctx = vec![0.0; n * h];
        let mut tmp = vec![0.0; n * h];
        let mut scores = vec![0.0; n * n];
        let mut inner = vec![0.0; n * intermediate];
        for layer in &self.layers {
            layer.query.apply(&x, n, &mut q, par);
            layer.key.apply(&x, n, &mut k, par);
            layer.value.apply(&x, n, &mut v, par);
            for head in 0..self.heads {
                let off = head * d;
                // scores = Q_h K_hᵀ / √d
                matmul(
                    (n, n, d),
                    View::new(&mut scores, 0, n, 1),
                    Read::new(&q, off, h, 1),
                    Read::new(&k, off, 1, h),
                    scale,
                    par,
                );
                for row in scores.chunks_exact_mut(n) {
                    softmax(row);
                }
                // ctx_h = softmax(scores) V_h
                matmul(
                    (n, d, n),
                    View::new(&mut ctx, off, h, 1),
                    Read::new(&scores, 0, n, 1),
                    Read::new(&v, off, h, 1),
                    1.0,
                    par,
                );
            }
            layer.attention_out.apply(&ctx, n, &mut tmp, par);
            add_into(&mut x, &tmp);
            layer_norm(&mut x, h, &layer.attention_norm, self.eps);

            layer.up.apply(&x, n, &mut inner, par);
            inner.iter_mut().for_each(|v| *v = gelu(*v));
            layer.down.apply(&inner, n, &mut tmp, par);
            add_into(&mut x, &tmp);
            layer_norm(&mut x, h, &layer.out_norm, self.eps);
        }

        let mut pooled = vec![0.0f32; h];
        match self.spec.pooling {
            Pooling::Cls => pooled.copy_from_slice(&x[..h]),
            Pooling::Mean => {
                for row in x.chunks_exact(h) {
                    add_into(&mut pooled, row);
                }
                pooled.iter_mut().for_each(|v| *v /= n as f32);
            }
        }
        let norm = pooled.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        pooled.iter_mut().for_each(|v| *v /= norm);
        pooled
    }
}

impl Linear {
    /// `out` (rows × outputs) = `x` (rows × inputs) Wᵀ + b.
    fn apply(&self, x: &[f32], rows: usize, out: &mut [f32], par: Parallelism) {
        let (i, o) = (self.inputs, self.outputs);
        matmul(
            (rows, o, i),
            View::new(out, 0, o, 1),
            Read::new(x, 0, i, 1),
            // Wᵀ: element (r, c) is W[c][r].
            Read::new(&self.weight, 0, 1, i),
            1.0,
            par,
        );
        for row in out[..rows * o].chunks_exact_mut(o) {
            add_into(row, &self.bias);
        }
    }
}

/// A matrix inside a slice: element (r, c) is `data[offset + r·rs + c·cs]`.
struct Read<'a> {
    data: &'a [f32],
    offset: usize,
    rs: usize,
    cs: usize,
}

impl<'a> Read<'a> {
    fn new(data: &'a [f32], offset: usize, rs: usize, cs: usize) -> Self {
        Read {
            data,
            offset,
            rs,
            cs,
        }
    }

    fn check(&self, rows: usize, cols: usize) {
        assert!(rows > 0 && cols > 0);
        let last = self.offset + (rows - 1) * self.rs + (cols - 1) * self.cs;
        assert!(last < self.data.len(), "matrix view out of bounds");
    }
}

struct View<'a> {
    data: &'a mut [f32],
    offset: usize,
    rs: usize,
    cs: usize,
}

impl<'a> View<'a> {
    fn new(data: &'a mut [f32], offset: usize, rs: usize, cs: usize) -> Self {
        View {
            data,
            offset,
            rs,
            cs,
        }
    }
}

/// Threads a matrix product is split over when one text is embedded while
/// someone waits ([`Model::embed_cached`]); more don't help at these sizes.
/// [`fill`] embeds many texts side by side instead, each on one thread, which
/// costs less in all. gemm splits the output, never the sums, so either way
/// the same text gives the same vector bit for bit, and a draft matches the
/// fragment it is saved as.
const GEMM_THREADS: usize = 4;

/// `dst` (m × n) = `scale` · `lhs` (m × k) · `rhs` (k × n).
fn matmul(
    (m, n, k): (usize, usize, usize),
    dst: View,
    lhs: Read,
    rhs: Read,
    scale: f32,
    par: Parallelism,
) {
    lhs.check(m, k);
    rhs.check(k, n);
    assert!(m > 0 && n > 0);
    let last = dst.offset + (m - 1) * dst.rs + (n - 1) * dst.cs;
    assert!(last < dst.data.len(), "matrix view out of bounds");
    // SAFETY: the checks above keep every element gemm reads or writes inside
    // its slice; `dst` is borrowed mutably, so it overlaps neither input.
    unsafe {
        gemm::gemm(
            m,
            n,
            k,
            dst.data.as_mut_ptr().add(dst.offset),
            dst.cs as isize,
            dst.rs as isize,
            false,
            lhs.data.as_ptr().add(lhs.offset),
            lhs.cs as isize,
            lhs.rs as isize,
            rhs.data.as_ptr().add(rhs.offset),
            rhs.cs as isize,
            rhs.rs as isize,
            0.0,
            scale,
            false,
            false,
            false,
            par,
        );
    }
}

fn add_into(into: &mut [f32], other: &[f32]) {
    for (a, b) in into.iter_mut().zip(other) {
        *a += b;
    }
}

fn layer_norm(x: &mut [f32], width: usize, norm: &Norm, eps: f32) {
    for row in x.chunks_exact_mut(width) {
        let mean = row.iter().sum::<f32>() / width as f32;
        let var = row.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / width as f32;
        let inv = 1.0 / (var + eps).sqrt();
        for ((v, w), b) in row.iter_mut().zip(&norm.weight).zip(&norm.bias) {
            *v = (*v - mean) * inv * w + b;
        }
    }
}

fn softmax(row: &mut [f32]) {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0;
    for v in row.iter_mut() {
        *v = (*v - max).exp();
        sum += *v;
    }
    row.iter_mut().for_each(|v| *v /= sum);
}

/// GELU as BERT defines it, with the exact error function.
fn gelu(x: f32) -> f32 {
    0.5 * x * (1.0 + erf(x / std::f32::consts::SQRT_2))
}

/// The error function, to about 1e-7 (Numerical Recipes' `erfc` by Chebyshev
/// fit), computed in f64.
fn erf(x: f32) -> f32 {
    let x = f64::from(x);
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -z * z - 1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let erfc = t * poly.exp();
    (if x >= 0.0 { 1.0 - erfc } else { erfc - 1.0 }) as f32
}

// ── Safetensors ────────────────────────────────────────────────────────

/// A `.safetensors` file, memory-mapped.
struct Tensors {
    map: Mmap,
    entries: HashMap<String, Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    dtype: Dtype,
    shape: Vec<usize>,
    /// Byte range in the file.
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Dtype {
    F32,
    BF16,
}

impl Dtype {
    fn size(self) -> usize {
        match self {
            Dtype::F32 => 4,
            Dtype::BF16 => 2,
        }
    }
}

#[derive(Deserialize)]
struct RawEntry {
    dtype: String,
    shape: Vec<usize>,
    data_offsets: [usize; 2],
}

impl Tensors {
    fn open(path: &Path) -> Result<Tensors, ParcError> {
        let file = File::open(path)?;
        // SAFETY: the file is a downloaded model in the user's own data
        // folder. Nothing in parc writes to it while it is mapped; a file
        // changed underneath by something else gives wrong vectors, and one
        // truncated could fault, as with any mapped file.
        let map = unsafe { Mmap::map(&file)? };
        let bad = |what: &str| model_error(format!("model.safetensors: {what}"));
        let len = map
            .get(..8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()) as usize)
            .ok_or_else(|| bad("too short"))?;
        if len > 100 << 20 || 8 + len > map.len() {
            return Err(bad("bad header length"));
        }
        let header: HashMap<String, serde_json::Value> = serde_json::from_slice(&map[8..8 + len])?;
        let data = 8 + len;
        let mut entries = HashMap::new();
        for (name, value) in header {
            if name == "__metadata__" {
                continue;
            }
            let raw: RawEntry = serde_json::from_value(value)?;
            let dtype = match raw.dtype.as_str() {
                "F32" => Dtype::F32,
                "BF16" => Dtype::BF16,
                // Integer buffers (position IDs) aren't weights.
                "I64" | "I32" => continue,
                other => return Err(bad(&format!("unsupported type {other} in {name}"))),
            };
            let [from, to] = raw.data_offsets;
            let count: usize = raw.shape.iter().product();
            if from > to || data + to > map.len() || to - from != count * dtype.size() {
                return Err(bad(&format!("bad offsets for {name}")));
            }
            entries.insert(
                name,
                Entry {
                    dtype,
                    shape: raw.shape,
                    start: data + from,
                    end: data + to,
                },
            );
        }
        Ok(Tensors { map, entries })
    }

    /// The entry `name`, checked to have `rows` rows (any, if `None`) of
    /// `cols` columns.
    fn entry(
        &self,
        name: &str,
        rows: Option<usize>,
        cols: Option<usize>,
    ) -> Result<Entry, ParcError> {
        let e = self
            .entries
            .get(name)
            .ok_or_else(|| model_error(format!("model.safetensors has no {name}")))?;
        let ok = match e.shape.as_slice() {
            [r, c] => rows.is_none_or(|n| n == *r) && cols.is_none_or(|n| n == *c),
            [c] => rows.is_none() && cols.is_none_or(|n| n == *c),
            _ => false,
        };
        if !ok {
            return Err(model_error(format!(
                "{name} has shape {:?}, not the one config.json describes",
                e.shape
            )));
        }
        Ok(e.clone())
    }

    /// The whole tensor `name`, of exactly `shape`, as f32.
    fn load(&self, name: &str, shape: &[usize]) -> Result<Vec<f32>, ParcError> {
        let e = match shape {
            [r, c] => self.entry(name, Some(*r), Some(*c))?,
            [c] => self.entry(name, None, Some(*c))?,
            _ => unreachable!(),
        };
        let mut out = vec![0.0; shape.iter().product()];
        decode(e.dtype, &self.map[e.start..e.end], &mut out);
        Ok(out)
    }

    /// Row `i` of a 2-D entry, as f32, read from the mapped file.
    fn row(&self, e: &Entry, i: usize, out: &mut [f32]) -> Result<(), ParcError> {
        let cols = *e.shape.last().unwrap_or(&0);
        let rows = e.shape.first().copied().unwrap_or(0);
        if i >= rows || out.len() != cols {
            return Err(model_error("row out of range"));
        }
        let width = cols * e.dtype.size();
        let start = e.start + i * width;
        decode(e.dtype, &self.map[start..start + width], out);
        Ok(())
    }
}

fn decode(dtype: Dtype, bytes: &[u8], out: &mut [f32]) {
    match dtype {
        Dtype::F32 => {
            for (v, b) in out.iter_mut().zip(bytes.chunks_exact(4)) {
                *v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
        }
        Dtype::BF16 => {
            for (v, b) in out.iter_mut().zip(bytes.chunks_exact(2)) {
                *v = f32::from_bits(u32::from(u16::from_le_bytes([b[0], b[1]])) << 16);
            }
        }
    }
}

// ── What gets embedded ─────────────────────────────────────────────────

/// Text as the model reads it: a fragment's title and body together (or a
/// draft's text), with `[[links]]` reduced to their label (an ID says nothing
/// to the model), attachment embeds left out, and runs of whitespace made one
/// space, so a draft and the fragment it is saved as read the same.
pub fn passage(text: &str) -> String {
    static LINK: OnceLock<regex::Regex> = OnceLock::new();
    let link =
        LINK.get_or_init(|| regex::Regex::new(r"(!?)\[\[([^\]|]*)(?:\|([^\]]*))?\]\]").unwrap());
    let text = link.replace_all(text, |c: &regex::Captures| {
        if !c[1].is_empty() {
            return String::new();
        }
        match c.get(3) {
            Some(label) => label.as_str().to_string(),
            None if looks_like_id(&c[2]) => String::new(),
            None => c[2].to_string(),
        }
    });
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An ID or ID prefix: Crockford base32, as ULIDs are written.
fn looks_like_id(s: &str) -> bool {
    s.len() >= 4
        && s.chars()
            .all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && !"ILOU".contains(c)))
}

// ── Vectors in the index ───────────────────────────────────────────────

/// Kept out of the index's own schema (and its format stamp), so that an
/// index rebuilt for a new parc keeps its vectors: they are keyed by file
/// revision, which a rebuild doesn't change. `meaning_model` names the model
/// the vectors came from; another model's are cleared, never compared.
const MEANING_SQL: &str = "
CREATE TABLE IF NOT EXISTS meaning_model (
    one     INTEGER PRIMARY KEY CHECK (one = 1),
    model   TEXT NOT NULL,
    format  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS meaning_vectors (
    id       TEXT PRIMARY KEY,
    revision TEXT NOT NULL,
    vector   BLOB NOT NULL
);
-- Which process is filling in vectors, until when (Unix milliseconds): see
-- `claim`.
CREATE TABLE IF NOT EXISTS meaning_lease (
    one      INTEGER PRIMARY KEY CHECK (one = 1),
    holder   TEXT NOT NULL,
    until_ms INTEGER NOT NULL
);
";

/// Bump when the stored vectors change meaning for the same model (what text
/// is embedded, or how), so old ones are cleared.
const MEANING_FORMAT: i64 = 2;

/// Fragments embedded per write transaction.
const BATCH: usize = 32;

/// How far the index's vectors are from complete.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Coverage {
    /// Fragments with an up-to-date vector.
    pub embedded: usize,
    /// Fragments in the index.
    pub total: usize,
}

fn tables_exist(conn: &Connection) -> Result<bool, ParcError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meaning_model'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Whether the index's vectors are `spec`'s, in the current format.
fn vectors_are(conn: &Connection, spec: &ModelSpec) -> Result<bool, ParcError> {
    if !tables_exist(conn)? {
        return Ok(false);
    }
    let stored: Option<(String, i64)> = conn
        .query_row(
            "SELECT model, format FROM meaning_model WHERE one = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(stored == Some((spec.key(), MEANING_FORMAT)))
}

/// Create the vector tables, and clear them if they hold another model's.
fn prepare(conn: &Connection, spec: &ModelSpec) -> Result<(), ParcError> {
    if vectors_are(conn, spec)? {
        return Ok(());
    }
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    tx.execute_batch(MEANING_SQL)?;
    if !vectors_are(&tx, spec)? {
        tx.execute("DELETE FROM meaning_vectors", [])?;
        tx.execute(
            "INSERT OR REPLACE INTO meaning_model (one, model, format) VALUES (1, ?1, ?2)",
            rusqlite::params![spec.key(), MEANING_FORMAT],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Claim the filling of this index's vectors for `seconds`, so that two
/// long-running processes on one vault (say, the desktop's server and an
/// agent's) don't both compute every vector. True when `holder` has the
/// claim: it was free, had lapsed, or was already `holder`'s (renewed).
pub fn claim(
    conn: &Connection,
    spec: &ModelSpec,
    holder: &str,
    seconds: u64,
) -> Result<bool, ParcError> {
    prepare(conn, spec)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    tx.execute_batch(MEANING_SQL)?;
    let current: Option<(String, i64)> = tx
        .query_row(
            "SELECT holder, until_ms FROM meaning_lease WHERE one = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let free = current.is_none_or(|(h, until)| h == holder || until < now);
    if free {
        tx.execute(
            "INSERT OR REPLACE INTO meaning_lease (one, holder, until_ms) VALUES (1, ?1, ?2)",
            rusqlite::params![holder, now + (seconds * 1000) as i64],
        )?;
    }
    tx.commit()?;
    Ok(free)
}

/// Give up a [`claim`], if `holder` has it.
pub fn release(conn: &Connection, holder: &str) -> Result<(), ParcError> {
    if tables_exist(conn)? {
        conn.execute("DELETE FROM meaning_lease WHERE holder = ?1", [holder])?;
    }
    Ok(())
}

/// How many fragments have an up-to-date vector from `spec`.
pub fn coverage(conn: &Connection, spec: &ModelSpec) -> Result<Coverage, ParcError> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM fragments", [], |r| r.get(0))?;
    let embedded: i64 = if vectors_are(conn, spec)? {
        conn.query_row(
            "SELECT COUNT(*) FROM fragments f
             JOIN fragment_files ff ON ff.id = f.id
             JOIN meaning_vectors v ON v.id = f.id AND v.revision = ff.revision",
            [],
            |r| r.get(0),
        )?
    } else {
        0
    };
    Ok(Coverage {
        embedded: embedded as usize,
        total: total as usize,
    })
}

/// What [`fill`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FillReport {
    /// Vectors computed by this call.
    pub embedded: usize,
    /// Fragments still without an up-to-date vector.
    pub remaining: usize,
}

/// Compute the vectors that are missing or stale, newest fragments first, at
/// most `max` of them (all, if `None`), on `threads` threads. Each batch is
/// committed as it is done, so stopping early loses nothing. `progress` is
/// called with the number done so far and the number this call set out to do.
pub fn fill(
    conn: &Connection,
    model: &Model,
    max: Option<usize>,
    threads: usize,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<FillReport, ParcError> {
    use rayon::prelude::*;

    prepare(conn, model.spec)?;
    conn.execute(
        "DELETE FROM meaning_vectors WHERE id NOT IN (SELECT id FROM fragments)",
        [],
    )?;
    let mut stmt = conn.prepare(
        "SELECT f.id, ff.revision, f.title, f.body FROM fragments f
         JOIN fragment_files ff ON ff.id = f.id
         LEFT JOIN meaning_vectors v ON v.id = f.id
         WHERE v.id IS NULL OR v.revision <> ff.revision
         ORDER BY f.created_at DESC, f.id",
    )?;
    let stale = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                passage(&format!(
                    "{}\n{}",
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?
                )),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let todo = max.map_or(stale.len(), |m| m.min(stale.len()));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
        .map_err(model_error)?;
    let mut done = 0;
    for batch in stale[..todo].chunks(BATCH) {
        let vectors = pool.install(|| {
            batch
                .par_iter()
                .map(|(_, _, text)| model.embed(text))
                .collect::<Result<Vec<_>, _>>()
        })?;
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        {
            let mut put = tx.prepare_cached(
                "INSERT OR REPLACE INTO meaning_vectors (id, revision, vector) VALUES (?1, ?2, ?3)",
            )?;
            for ((id, revision, _), vector) in batch.iter().zip(&vectors) {
                put.execute(rusqlite::params![id, revision, to_blob(vector)])?;
            }
        }
        tx.commit()?;
        done += batch.len();
        progress(done, todo);
    }
    Ok(FillReport {
        embedded: done,
        remaining: stale.len() - done,
    })
}

/// How close `seed` is to each fragment `filter_sql` keeps (SQL over `f`,
/// with its parameters) that has an up-to-date vector from `model`: see
/// [`similarity`]. Scored straight from the stored bytes, as this runs over
/// the whole vault on every recall.
pub(crate) fn closeness(
    conn: &Connection,
    model: &Model,
    seed: &[Vec<f32>],
    filter_sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Vec<(String, f32)>, ParcError> {
    if !vectors_are(conn, model.spec)? {
        return Ok(Vec::new());
    }
    let dims = model.dims();
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT f.id, v.vector FROM fragments f
         JOIN fragment_files ff ON ff.id = f.id
         JOIN meaning_vectors v ON v.id = f.id AND v.revision = ff.revision
         WHERE {filter_sql}"
    ))?;
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let blob = row
            .get_ref(1)?
            .as_blob()
            .map_err(|e| ParcError::IndexError(format!("meaning vector: {e}")))?;
        let best = blob
            .chunks_exact(4 * dims)
            .flat_map(|stored| seed.iter().map(move |s| dot_bytes(s, stored)))
            .fold(-1.0f32, f32::max);
        out.push((row.get::<_, String>(0)?, best));
    }
    Ok(out)
}

/// `a · b`, `b` as stored: little-endian f32. Eight running sums, so the
/// compiler can use vector instructions.
fn dot_bytes(a: &[f32], b: &[u8]) -> f32 {
    let mut acc = [0.0f32; 8];
    let mut a8 = a.chunks_exact(8);
    let mut b8 = b.chunks_exact(32);
    for (x, y) in (&mut a8).zip(&mut b8) {
        for i in 0..8 {
            let v = f32::from_le_bytes([y[4 * i], y[4 * i + 1], y[4 * i + 2], y[4 * i + 3]]);
            acc[i] += x[i] * v;
        }
    }
    let mut sum: f32 = acc.iter().sum();
    for (x, y) in a8.remainder().iter().zip(b8.remainder().chunks_exact(4)) {
        sum += x * f32::from_le_bytes([y[0], y[1], y[2], y[3]]);
    }
    sum
}

/// A text's window vectors, one after another, as little-endian f32.
fn to_blob(windows: &[Vec<f32>]) -> Vec<u8> {
    windows
        .iter()
        .flatten()
        .flat_map(|x| x.to_le_bytes())
        .collect()
}

/// How close two texts are in meaning: the cosine similarity of their
/// closest windows (see [`Model::embed`]), so a passage of a long note can
/// match a short one. At most 1; -1 when either has no vector. The same
/// arithmetic as recall uses on stored vectors.
pub fn similarity(a: &[Vec<f32>], b: &[Vec<f32>]) -> f32 {
    let mut best = -1.0f32;
    for y in b {
        let bytes = to_blob(std::slice::from_ref(y));
        for x in a {
            best = best.max(dot_bytes(x, &bytes));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_is_the_closest_windows_dot_product() {
        let unit = |v: &[f32]| {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter().map(|x| x / n).collect::<Vec<f32>>()
        };
        let a: Vec<f32> = unit(&(0..19).map(|i| i as f32 - 9.0).collect::<Vec<_>>());
        let b: Vec<f32> = unit(&(0..19).map(|i| (i * i % 7) as f32).collect::<Vec<_>>());
        let plain: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
        let one = std::slice::from_ref;
        assert!((similarity(one(&a), one(&b)) - plain).abs() < 1e-6);
        assert!((similarity(one(&a), &[b.clone(), a.clone()]) - 1.0).abs() < 1e-6);
        assert_eq!(similarity(&[], &[a]), -1.0);
    }

    #[test]
    fn erf_matches_known_values() {
        for (x, want) in [
            (0.0, 0.0),
            (0.5, 0.520_499_9),
            (1.0, 0.842_700_8),
            (-2.0, -0.995_322_3),
        ] {
            assert!((erf(x) - want).abs() < 1e-6, "erf({x}) = {}", erf(x));
        }
    }

    #[test]
    fn matmul_follows_strides() {
        // [1 2 3; 4 5 6] × [1 0; 0 1; 1 1] = [4 5; 10 11]
        let a = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let b = [1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let mut out = [0.0; 4];
        matmul(
            (2, 2, 3),
            View::new(&mut out, 0, 2, 1),
            Read::new(&a, 0, 3, 1),
            Read::new(&b, 0, 2, 1),
            1.0,
            Parallelism::None,
        );
        assert_eq!(out, [4.0, 5.0, 10.0, 11.0]);
        // Bᵀ given as B stored transposed: (r, c) is bt[c][r].
        let bt = [1.0, 0.0, 1.0, 0.0, 1.0, 1.0];
        let mut out = [0.0; 4];
        matmul(
            (2, 2, 3),
            View::new(&mut out, 0, 2, 1),
            Read::new(&a, 0, 3, 1),
            Read::new(&bt, 0, 1, 3),
            2.0,
            Parallelism::None,
        );
        assert_eq!(out, [8.0, 10.0, 20.0, 22.0]);
    }

    #[test]
    fn passages_drop_ids_and_attachments_but_keep_labels() {
        assert_eq!(
            passage(
                " Ferry desk \n\nSee [[01JQ7V4Y]] and [[01JQ7V4Y|the route cards]], [[Spring fair]].\n![[attach:photo.png]]"
            ),
            "Ferry desk See and the route cards, Spring fair."
        );
    }

    #[test]
    fn spec_urls_are_pinned_to_a_revision() {
        for spec in CATALOG {
            for f in spec.files {
                let url = spec.url(f);
                assert!(url.starts_with("https://huggingface.co/"), "{url}");
                assert!(
                    url.contains(spec.revision) && !url.contains("/main/"),
                    "{url}"
                );
                assert_eq!(f.sha256.len(), 64);
            }
        }
    }
}
