//! Attachment text extraction, sidecars, isolated child process, and index store.
//!
//! Supported formats (detected from file content, checked against this list):
//! PDF, docx, pptx, HTML, Markdown, plain text. Other files (images, archives,
//! binaries) report [`ExtractOutcome::NoText`], not an error.
//!
//! Extraction uses `xberg` pinned to an exact version, configured purely in code
//! (never `ExtractionConfig::discover()` or `apply_env_overrides()`), with
//! `use_cache: false`, OCR disabled, and strict security limits. Production
//! extraction runs inside a child process of the running parc binary
//! ([`isolated`]) with `RLIMIT_AS`, `RLIMIT_CPU`, and a wall-clock timeout so a
//! hostile or broken file can never hang or crash `parc-server`.

#[cfg(test)]
mod fixtures_tests;
mod process;
pub mod sidecar;
pub mod store;

pub use process::{child_main, dispatch_worker, isolated, isolated_with_exe, CHILD_ARG};
pub use sidecar::{read_sidecar, write_sidecar, SidecarState};
pub use store::{
    attachments_need_sync, claim_lease, ensure_cache_tables, fill, fill_one, has_pending,
    list_status, locate, pending_count, read_parts, read_parts_read_only, release_lease,
    sync_attachments, AttachmentTextStatus, FillReport,
};

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::ParcError;

/// parc's own extracted-text format version. Bump by hand only when parc's
/// output structure changes; never tied to xberg's crate version, so routine
/// dependency updates don't rewrite every sidecar in git.
pub const TEXT_FORMAT: u32 = 1;

/// Default target size for splitting plain-text files into searchable chunks.
const PLAIN_TEXT_CHUNK_CHARS: usize = 3_000;

/// What one "part" represents for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartUnit {
    Page,
    Slide,
    Section,
}

impl PartUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            PartUnit::Page => "page",
            PartUnit::Slide => "slide",
            PartUnit::Section => "section",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "page" => Some(PartUnit::Page),
            "slide" => Some(PartUnit::Slide),
            "section" => Some(PartUnit::Section),
            _ => None,
        }
    }

    /// Human-readable short reference for search hits, recall excerpts, and quotes:
    /// `p. 12`, `slide 3`, or `§ Heading` (falling back to `§ 3`).
    pub fn format_ref(self, part_1based: usize, part_text: Option<&str>) -> String {
        match self {
            PartUnit::Page => format!("p. {part_1based}"),
            PartUnit::Slide => format!("slide {part_1based}"),
            PartUnit::Section => match part_text.and_then(section_heading) {
                Some(heading) => format!("§ {heading}"),
                None => format!("§ {part_1based}"),
            },
        }
    }
}

/// Format of the extracted part strings (and which extension the sidecar uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextFormat {
    Text,
    Markdown,
}

impl TextFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            TextFormat::Text => "text",
            TextFormat::Markdown => "markdown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "text" => Some(TextFormat::Text),
            "markdown" => Some(TextFormat::Markdown),
            _ => None,
        }
    }

    pub fn sidecar_ext(self) -> &'static str {
        match self {
            TextFormat::Text => "txt",
            TextFormat::Markdown => "md",
        }
    }
}

/// Extracted text from one attachment, split into addressable parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extracted {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub unit: PartUnit,
    pub format: TextFormat,
    pub parts: Vec<String>,
}

/// Result of inspecting an attachment file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtractOutcome {
    Extracted(Extracted),
    NoText,
}

/// Resource and security limits applied to extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractLimits {
    /// Maximum raw input file size in bytes.
    pub max_file_bytes: usize,
    /// Maximum extracted text/content bytes inside xberg.
    pub max_content_size: usize,
    /// Maximum uncompressed archive size for OOXML containers (docx/pptx).
    pub max_archive_size: usize,
    /// Maximum archive entries for OOXML containers.
    pub max_files_in_archive: usize,
    /// Maximum zip compression ratio before treating as a zip bomb.
    pub max_compression_ratio: usize,
    /// Maximum pages or slides in a single document.
    pub max_pages: usize,
    /// Virtual address space ceiling (`RLIMIT_AS`) for the child process, in bytes.
    pub max_memory_bytes: u64,
    /// CPU time ceiling (`RLIMIT_CPU`) for the child process, in seconds.
    pub max_cpu_secs: u64,
    /// Wall-clock timeout for the child process, in seconds.
    pub wall_timeout_secs: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 100 * 1024 * 1024,   // 100 MiB
            max_content_size: 50 * 1024 * 1024,  // 50 MiB
            max_archive_size: 100 * 1024 * 1024, // 100 MiB
            max_files_in_archive: 4_000,
            max_compression_ratio: 100,
            max_pages: 2_000,
            max_memory_bytes: 2 * 1024 * 1024 * 1024, // 2 GiB virtual address space
            max_cpu_secs: 30,
            wall_timeout_secs: 45,
        }
    }
}

/// Compute lowercase hex SHA-256 of bytes.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Compute lowercase hex SHA-256 of a file without loading it all into memory.
pub fn sha256_file(path: &Path) -> Result<String, ParcError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedKind {
    Pdf,
    Docx,
    Pptx,
    Html,
    Markdown,
    PlainText,
}

const MIME_PDF: &str = "application/pdf";
const MIME_DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const MIME_PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
const MIME_HTML: &str = "text/html";

/// Extract text from raw file bytes in the current process.
///
/// Production code should prefer [`isolated`] to run extraction in a bounded
/// child process; tests and the child process itself call this directly.
pub fn in_process(
    bytes: &[u8],
    filename_hint: Option<&str>,
    limits: &ExtractLimits,
) -> Result<ExtractOutcome, ParcError> {
    if bytes.len() > limits.max_file_bytes {
        return Err(ParcError::ValidationError(format!(
            "file is too large ({} bytes; limit {} bytes)",
            bytes.len(),
            limits.max_file_bytes
        )));
    }
    if bytes.is_empty() {
        return Ok(ExtractOutcome::NoText);
    }

    let Some(kind) = detect_supported_kind(bytes, filename_hint)? else {
        return Ok(ExtractOutcome::NoText);
    };

    match kind {
        SupportedKind::Markdown => extract_markdown_bytes(bytes),
        SupportedKind::PlainText => extract_plain_text_bytes(bytes),
        SupportedKind::Pdf | SupportedKind::Docx | SupportedKind::Pptx | SupportedKind::Html => {
            extract_with_xberg(bytes, kind, limits)
        }
    }
}

fn detect_supported_kind(
    bytes: &[u8],
    filename_hint: Option<&str>,
) -> Result<Option<SupportedKind>, ParcError> {
    // Magic-byte PDF check first (xberg handles it too, and this avoids sniffing ambiguity).
    if bytes.starts_with(b"%PDF-") {
        return Ok(Some(SupportedKind::Pdf));
    }

    // Content-based detection via xberg (never trusts filename alone for binary formats).
    let detected = match xberg::detect_mime_type_from_bytes(bytes) {
        Ok(mime) => mime,
        Err(err) => {
            let msg = err.to_string();
            // Security errors during package inspection (e.g. zip bomb) must fail loudly.
            if msg.to_lowercase().contains("bomb")
                || msg.to_lowercase().contains("security")
                || msg.to_lowercase().contains("too large")
                || msg.to_lowercase().contains("too many")
            {
                return Err(ParcError::ValidationError(format!(
                    "refused unsafe attachment: {msg}"
                )));
            }
            String::new()
        }
    };

    match detected.as_str() {
        MIME_PDF => return Ok(Some(SupportedKind::Pdf)),
        MIME_DOCX => return Ok(Some(SupportedKind::Docx)),
        MIME_PPTX => return Ok(Some(SupportedKind::Pptx)),
        "text/html" | "application/xhtml+xml" => return Ok(Some(SupportedKind::Html)),
        "text/markdown" | "text/x-markdown" => return Ok(Some(SupportedKind::Markdown)),
        _ => {}
    }

    // If it's a ZIP container with a .docx or .pptx hint whose inner entries failed detection
    // (e.g., a crafted/broken office zip or zip bomb), still route to office extraction so
    // security limits are enforced rather than silently returning NoText.
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        let ext = extension_lower(filename_hint);
        if ext.as_deref() == Some("docx") {
            return Ok(Some(SupportedKind::Docx));
        }
        if ext.as_deref() == Some("pptx") {
            return Ok(Some(SupportedKind::Pptx));
        }
        return Ok(None);
    }

    // Reject other known binary formats (images, audio, video, executables, archives, etc.).
    if !detected.is_empty()
        && !detected.starts_with("text/")
        && detected != "application/octet-stream"
    {
        return Ok(None);
    }

    // For UTF-8 text inputs, distinguish HTML, Markdown, and plain text.
    if let Ok(text) = std::str::from_utf8(bytes) {
        if looks_binary_text(text) {
            return Ok(None);
        }
        let trimmed = text.trim_start_matches('\u{feff}').trim_start();
        let lower_head: String = trimmed
            .chars()
            .take(32)
            .flat_map(char::to_lowercase)
            .collect();
        if lower_head.starts_with("<!doctype html") || lower_head.starts_with("<html") {
            return Ok(Some(SupportedKind::Html));
        }
        match extension_lower(filename_hint).as_deref() {
            Some("md" | "markdown") => return Ok(Some(SupportedKind::Markdown)),
            Some("html" | "htm" | "xhtml") => return Ok(Some(SupportedKind::Html)),
            Some("txt" | "text") => return Ok(Some(SupportedKind::PlainText)),
            _ if detected == "text/plain" => return Ok(Some(SupportedKind::PlainText)),
            _ => {}
        }
    }

    Ok(None)
}

fn extension_lower(filename: Option<&str>) -> Option<String> {
    Path::new(filename?)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
}

fn looks_binary_text(s: &str) -> bool {
    s.bytes()
        .take(4096)
        .any(|b| b == 0 || (b < 0x09) || (b > 0x0d && b < 0x20 && b != 0x1b))
}

fn build_xberg_config(kind: SupportedKind, limits: &ExtractLimits) -> xberg::ExtractionConfig {
    let mut security = xberg::SecurityLimits::default();
    security.max_content_size = limits.max_content_size;
    security.max_archive_size = limits.max_archive_size;
    security.max_files_in_archive = limits.max_files_in_archive;
    security.max_compression_ratio = limits.max_compression_ratio;
    security.max_pages = Some(limits.max_pages);

    let output_format = match kind {
        SupportedKind::Pdf => xberg::OutputFormat::Plain,
        SupportedKind::Docx | SupportedKind::Pptx | SupportedKind::Html => {
            xberg::OutputFormat::Markdown
        }
        SupportedKind::Markdown | SupportedKind::PlainText => xberg::OutputFormat::Plain,
    };

    let html_options = if kind == SupportedKind::Html {
        let mut opts = xberg::ConversionOptions::default();
        opts.extract_metadata = false;
        Some(opts)
    } else {
        None
    };

    let mut cfg = xberg::ExtractionConfig::default();
    cfg.mime_detection_policy = xberg::MimeDetectionPolicy::ContentOnly;
    cfg.use_cache = false;
    cfg.disable_ocr = true;
    cfg.ocr = None;
    cfg.force_ocr = false;
    cfg.pages = Some(xberg::PageConfig {
        extract_pages: true,
        insert_page_markers: false,
        ..Default::default()
    });
    cfg.security_limits = Some(security);
    cfg.output_format = output_format;
    cfg.html_options = html_options;
    cfg.extraction_timeout_secs = None; // enforced by the parent process wall clock
    cfg
}

fn extract_with_xberg(
    bytes: &[u8],
    kind: SupportedKind,
    limits: &ExtractLimits,
) -> Result<ExtractOutcome, ParcError> {
    let mime = match kind {
        SupportedKind::Pdf => MIME_PDF,
        SupportedKind::Docx => MIME_DOCX,
        SupportedKind::Pptx => MIME_PPTX,
        SupportedKind::Html => MIME_HTML,
        SupportedKind::Markdown | SupportedKind::PlainText => unreachable!(),
    };
    let config = build_xberg_config(kind, limits);
    let input = xberg::ExtractInput::from_bytes(bytes.to_vec(), mime, None);
    let result = pollster::block_on(xberg::extract(input, &config))
        .map_err(|e| ParcError::ValidationError(format!("extraction failed: {e}")))?;

    if let Some(err) = result.errors.first() {
        return Err(ParcError::ValidationError(format!(
            "extraction failed: {}",
            err.message
        )));
    }
    let Some(doc) = result.results.into_iter().next() else {
        return Ok(ExtractOutcome::NoText);
    };

    let meta_title = clean_title(doc.metadata.title.as_deref()).or_else(|| {
        if let Some(xberg::FormatMetadata::Docx(ref docx)) = doc.metadata.format {
            docx.core_properties
                .as_ref()
                .and_then(|cp| clean_title(cp.title.as_deref()))
        } else {
            None
        }
    });

    match kind {
        SupportedKind::Pdf => {
            let mut parts: Vec<String> = match doc.pages {
                Some(pages) if !pages.is_empty() => pages
                    .into_iter()
                    .map(|p| sanitize_part(&p.content))
                    .collect(),
                _ => {
                    let text = sanitize_part(&doc.content);
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![text]
                    }
                }
            };
            if parts.len() > limits.max_pages {
                return Err(ParcError::ValidationError(format!(
                    "document has too many pages ({} > {})",
                    parts.len(),
                    limits.max_pages
                )));
            }
            if parts.iter().all(|p| p.trim().is_empty()) {
                parts.clear();
            }
            if parts.is_empty() {
                return Ok(ExtractOutcome::NoText);
            }
            Ok(ExtractOutcome::Extracted(Extracted {
                title: meta_title,
                unit: PartUnit::Page,
                format: TextFormat::Text,
                parts,
            }))
        }
        SupportedKind::Pptx => {
            let mut parts: Vec<String> = match doc.pages {
                Some(pages) if !pages.is_empty() => pages
                    .into_iter()
                    .map(|p| {
                        let mut text = sanitize_part(&p.content);
                        if let Some(notes) = p.speaker_notes.as_deref().map(str::trim) {
                            if !notes.is_empty() {
                                if !text.is_empty() {
                                    text.push_str("\n\n");
                                }
                                text.push_str("> Notes: ");
                                text.push_str(&sanitize_part(notes).replace('\n', "\n> "));
                            }
                        }
                        text
                    })
                    .collect(),
                _ => {
                    let text = sanitize_part(&doc.content);
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![text]
                    }
                }
            };
            if parts.iter().all(|p| p.trim().is_empty()) {
                parts.clear();
            }
            if parts.is_empty() {
                return Ok(ExtractOutcome::NoText);
            }
            let title = meta_title.or_else(|| parts.first().and_then(|p| section_heading(p)));
            Ok(ExtractOutcome::Extracted(Extracted {
                title,
                unit: PartUnit::Slide,
                format: TextFormat::Markdown,
                parts,
            }))
        }
        SupportedKind::Docx => {
            let content = sanitize_part(&doc.content);
            let parts = split_markdown_sections(&content);
            if parts.is_empty() {
                return Ok(ExtractOutcome::NoText);
            }
            let title = meta_title.or_else(|| first_heading(&parts));
            Ok(ExtractOutcome::Extracted(Extracted {
                title,
                unit: PartUnit::Section,
                format: TextFormat::Markdown,
                parts,
            }))
        }
        SupportedKind::Html => {
            let content = sanitize_part(&doc.content);
            let parts = split_markdown_sections(&content);
            if parts.is_empty() {
                return Ok(ExtractOutcome::NoText);
            }
            // HTML runs with extract_metadata: false, so the title comes from the first heading.
            let title = first_heading(&parts);
            Ok(ExtractOutcome::Extracted(Extracted {
                title,
                unit: PartUnit::Section,
                format: TextFormat::Markdown,
                parts,
            }))
        }
        SupportedKind::Markdown | SupportedKind::PlainText => unreachable!(),
    }
}

fn extract_markdown_bytes(bytes: &[u8]) -> Result<ExtractOutcome, ParcError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ParcError::ValidationError("markdown file is not valid UTF-8".into()))?;
    let clean = sanitize_part(text.strip_prefix('\u{feff}').unwrap_or(text));
    let parts = split_markdown_sections(&clean);
    if parts.is_empty() {
        return Ok(ExtractOutcome::NoText);
    }
    let title = first_heading(&parts);
    Ok(ExtractOutcome::Extracted(Extracted {
        title,
        unit: PartUnit::Section,
        format: TextFormat::Markdown,
        parts,
    }))
}

fn extract_plain_text_bytes(bytes: &[u8]) -> Result<ExtractOutcome, ParcError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ParcError::ValidationError("text file is not valid UTF-8".into()))?;
    let clean = sanitize_part(text.strip_prefix('\u{feff}').unwrap_or(text));
    let parts = split_plain_text_chunks(&clean, PLAIN_TEXT_CHUNK_CHARS);
    if parts.is_empty() {
        return Ok(ExtractOutcome::NoText);
    }
    Ok(ExtractOutcome::Extracted(Extracted {
        title: None,
        unit: PartUnit::Section,
        format: TextFormat::Text,
        parts,
    }))
}

fn clean_title(title: Option<&str>) -> Option<String> {
    let t = title?.trim();
    if t.is_empty() {
        return None;
    }
    let single = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if single.is_empty() {
        None
    } else {
        Some(single)
    }
}

/// Normalize CRLF to LF, replace form feeds with newlines (since `\x0c` is the
/// sidecar part separator), and trim trailing whitespace.
fn sanitize_part(s: &str) -> String {
    s.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\x0c', "\n")
        .trim()
        .to_string()
}

/// Split Markdown text into sections starting at each ATX heading (`# `..`###### `),
/// ignoring `#` inside fenced code blocks. If there are no headings, falls back
/// to paragraph-boundary chunking when long.
pub fn split_markdown_sections(markdown: &str) -> Vec<String> {
    let trimmed = markdown.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut sections: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;
    let mut saw_heading = false;

    for line in trimmed.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if !in_fence && is_atx_heading(t) {
            saw_heading = true;
            if !current.trim().is_empty() {
                sections.push(current.trim().to_string());
                current.clear();
            }
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.trim().is_empty() {
        sections.push(current.trim().to_string());
    }

    if !saw_heading && trimmed.chars().count() > PLAIN_TEXT_CHUNK_CHARS * 4 / 3 {
        return split_plain_text_chunks(trimmed, PLAIN_TEXT_CHUNK_CHARS);
    }
    sections
}

/// Split plain text into chunks of roughly `target_chars` at paragraph breaks
/// (`\n\n`).
pub fn split_plain_text_chunks(text: &str, target_chars: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for para in trimmed.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        let para_len = para.chars().count();
        if current_len > 0 && current_len + 2 + para_len > target_chars {
            chunks.push(std::mem::take(&mut current));
            current_len = 0;
        }
        if para_len > target_chars && current.is_empty() {
            // Very long single paragraph: split by lines, or by char boundary.
            for piece in split_long_paragraph(para, target_chars) {
                chunks.push(piece);
            }
            continue;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
            current_len += 2;
        }
        current.push_str(para);
        current_len += para_len;
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

fn split_long_paragraph(para: &str, target_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for word in para.split_whitespace() {
        let wlen = word.chars().count();
        if cur_len > 0 && cur_len + 1 + wlen > target_chars {
            out.push(std::mem::take(&mut cur));
            cur_len = 0;
        }
        if !cur.is_empty() {
            cur.push(' ');
            cur_len += 1;
        }
        cur.push_str(word);
        cur_len += wlen;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_atx_heading(trimmed_start: &str) -> bool {
    let hashes = trimmed_start.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return false;
    }
    let rest = &trimmed_start[hashes..];
    rest.starts_with(' ') || rest.starts_with('\t')
}

/// Extract the text of the first ATX heading in a Markdown section, if any.
pub fn section_heading(part: &str) -> Option<String> {
    for line in part.lines() {
        let t = line.trim();
        if is_atx_heading(t) {
            let text = t
                .trim_start_matches('#')
                .trim()
                .trim_end_matches('#')
                .trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn first_heading(parts: &[String]) -> Option<String> {
    parts.iter().find_map(|p| section_heading(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_markdown_sections_ignores_fenced_headings() {
        let md = "Intro text\n\n# First\nBody one.\n\n```sh\n# not a heading\n```\n\n## Second\nBody two.";
        let sections = split_markdown_sections(md);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0], "Intro text");
        assert!(sections[1].starts_with("# First"));
        assert!(sections[1].contains("# not a heading"));
        assert_eq!(section_heading(&sections[1]).as_deref(), Some("First"));
        assert_eq!(section_heading(&sections[2]).as_deref(), Some("Second"));
    }

    #[test]
    fn split_plain_text_chunks_splits_at_paragraphs() {
        let p1 = "a".repeat(2000);
        let p2 = "b".repeat(2000);
        let text = format!("{p1}\n\n{p2}");
        let chunks = split_plain_text_chunks(&text, 3000);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], p1);
        assert_eq!(chunks[1], p2);
    }

    #[test]
    fn non_text_binary_returns_no_text() {
        // Minimal PNG header
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        let out = in_process(png, Some("photo.png"), &ExtractLimits::default()).unwrap();
        assert_eq!(out, ExtractOutcome::NoText);
    }

    #[test]
    fn oversized_file_is_refused() {
        let mut limits = ExtractLimits::default();
        limits.max_file_bytes = 16;
        let err = in_process(b"0123456789abcdefX", Some("big.txt"), &limits).unwrap_err();
        assert!(err.to_string().contains("too large"), "{err}");
    }
}
