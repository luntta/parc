use std::path::PathBuf;

use parc_core::attachment::{self, AttachMode};
use parc_core::config::{save_attachments, AttachmentTextMode, AttachmentsConfig};
use parc_core::extract::{
    self, in_process, sidecar, ExtractLimits, ExtractOutcome, Extracted, PartUnit, TextFormat,
    TEXT_FORMAT,
};
use parc_core::fragment::{self, new_fragment};
use parc_core::hook::{HookStderr, ScriptRunner};
use parc_core::index;
use parc_core::ops::Writer;
use parc_core::schema::load_schemas;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/attachments")
}

fn check_or_bless(name: &str, extracted: &Extracted) {
    let expected_path = fixture_dir().join(format!("{name}.expected.json"));
    let actual = serde_json::to_string_pretty(extracted).unwrap() + "\n";
    if std::env::var_os("PARC_EXTRACT_BLESS").is_some() {
        std::fs::write(&expected_path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("missing golden {}: {e}", expected_path.display()));
    assert_eq!(
        actual, expected,
        "extracted text changed for {name}; if intentional, bump TEXT_FORMAT and rerun with PARC_EXTRACT_BLESS=1"
    );
}

#[test]
fn golden_pdf_docx_pptx_html() {
    let limits = ExtractLimits::default();
    let cases = [
        ("sample.pdf", PartUnit::Page, TextFormat::Text),
        ("sample.docx", PartUnit::Section, TextFormat::Markdown),
        ("sample.pptx", PartUnit::Slide, TextFormat::Markdown),
        ("sample.html", PartUnit::Section, TextFormat::Markdown),
    ];

    for (name, expected_unit, expected_format) in cases {
        let path = fixture_dir().join(name);
        let bytes = std::fs::read(&path).unwrap();
        let outcome = in_process(&bytes, Some(name), &limits)
            .unwrap_or_else(|e| panic!("failed extracting {name}: {e}"));
        let ExtractOutcome::Extracted(ext) = outcome else {
            panic!("{name} produced NoText");
        };
        assert_eq!(ext.unit, expected_unit, "{name} unit");
        assert_eq!(ext.format, expected_format, "{name} format");
        assert!(!ext.parts.is_empty(), "{name} parts should not be empty");
        check_or_bless(name, &ext);
    }

    // Verify specific structural guarantees on the fixtures:
    // - PDF has 2 pages
    let pdf = match in_process(
        &std::fs::read(fixture_dir().join("sample.pdf")).unwrap(),
        Some("sample.pdf"),
        &limits,
    )
    .unwrap()
    {
        ExtractOutcome::Extracted(e) => e,
        _ => unreachable!(),
    };
    assert_eq!(pdf.parts.len(), 2, "PDF should split into 2 pages");
    assert!(pdf.parts[0].contains("Icebreaker"));
    assert!(pdf.parts[1].contains("crew rotation"));

    // - PPTX has 2 slides and captures speaker notes
    let pptx = match in_process(
        &std::fs::read(fixture_dir().join("sample.pptx")).unwrap(),
        Some("sample.pptx"),
        &limits,
    )
    .unwrap()
    {
        ExtractOutcome::Extracted(e) => e,
        _ => unreachable!(),
    };
    assert_eq!(pptx.parts.len(), 2, "PPTX should have 2 slides");
    assert!(pptx.parts[0].contains("Local-first"));
    assert!(pptx.parts[1].contains("Next Milestones"));

    // - HTML has no broken `## title:` header, keeps heading & table, title from H1
    let html = match in_process(
        &std::fs::read(fixture_dir().join("sample.html")).unwrap(),
        Some("sample.html"),
        &limits,
    )
    .unwrap()
    {
        ExtractOutcome::Extracted(e) => e,
        _ => unreachable!(),
    };
    assert_eq!(html.title.as_deref(), Some("Harbor Operations Handbook"));
    assert!(!html.parts.iter().any(|p| p.contains("## title:")));
    assert!(html.parts.len() >= 3, "HTML should split into heading sections");
}

#[test]
fn sidecar_stale_newer_and_index_only_modes() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();

    let mut w = Writer::new(&vault, ScriptRunner::new(HookStderr::Capture));
    let cfg = parc_core::config::load_config(&vault).unwrap();
    let schemas = load_schemas(&vault).unwrap();
    let frag = w
        .create(new_fragment("note", "Reading", schemas.resolve("note").unwrap(), &cfg))
        .unwrap();

    // Default mode is `sidecar`: attaching writes `.text/sample.html.md`
    let html_src = fixture_dir().join("sample.html");
    let (_, filename) = w.attach(&frag.id, &html_src, AttachMode::Copy).unwrap();
    let sidecar_file = sidecar::sidecar_path(&vault, &frag.id, &filename, TextFormat::Markdown);
    assert!(sidecar_file.exists(), "sidecar should be written on attach");
    // `.text/` is never listed as an attachment
    let listed = attachment::list_attachments(&vault, &frag.id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].filename, "sample.html");

    // A stale sidecar (wrong hash) is redone on fill/reindex
    std::fs::write(
        &sidecar_file,
        "<!-- parc-text {\"v\":1,\"sha256\":\"0000\",\"unit\":\"section\",\"format\":\"markdown\"} -->\nStale text",
    )
    .unwrap();
    index::reindex(&vault).unwrap();
    let conn = index::open_index(&vault).unwrap();
    extract::fill(&vault, &conn).unwrap();
    let refreshed = std::fs::read_to_string(&sidecar_file).unwrap();
    assert!(refreshed.contains("Harbor Operations Handbook"));

    // A newer-version sidecar is left untouched on disk and still indexed
    let sha = extract::sha256_file(&vault.join("attachments").join(&frag.id).join(&filename))
        .unwrap();
    let future_v = TEXT_FORMAT + 5;
    let future_content = format!(
        "<!-- parc-text {{\"v\":{future_v},\"sha256\":\"{sha}\",\"unit\":\"section\",\"format\":\"markdown\",\"title\":\"Future Title\"}} -->\nFuture section text"
    );
    std::fs::write(&sidecar_file, &future_content).unwrap();
    index::reindex(&vault).unwrap();
    let conn = index::open_index(&vault).unwrap();
    extract::fill(&vault, &conn).unwrap();
    assert_eq!(
        std::fs::read_to_string(&sidecar_file).unwrap(),
        future_content,
        "newer-version sidecar must never be overwritten"
    );
    let read_back = extract::read_parts(&vault, &conn, &frag.id, &filename)
        .unwrap()
        .unwrap();
    assert_eq!(read_back.parts, vec!["Future section text"]);

    // Switching to `index` mode writes no sidecars on attach and survives reindex
    save_attachments(
        &vault,
        &AttachmentsConfig {
            text: AttachmentTextMode::Index,
        },
    )
    .unwrap();
    let pdf_src = fixture_dir().join("sample.pdf");
    let (_, pdf_name) = w.attach(&frag.id, &pdf_src, AttachMode::Copy).unwrap();
    let pdf_sidecar = sidecar::sidecar_path(&vault, &frag.id, &pdf_name, TextFormat::Text);
    assert!(
        !pdf_sidecar.exists(),
        "index mode must not write a sidecar file"
    );
    // Index rebuild keeps attachment_text cache and re-populates attachment_fts without re-extracting
    index::reindex(&vault).unwrap();
    assert!(!pdf_sidecar.exists());
    let conn = index::open_index(&vault).unwrap();
    let parts = extract::read_parts(&vault, &conn, &frag.id, &pdf_name)
        .unwrap()
        .unwrap();
    assert_eq!(parts.parts.len(), 2);
}

#[test]
fn sync_on_open_never_extracts_and_failures_are_remembered() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();

    let cfg = parc_core::config::load_config(&vault).unwrap();
    let schemas = load_schemas(&vault).unwrap();
    let mut frag = new_fragment("note", "Pulled", schemas.resolve("note").unwrap(), &cfg);
    frag.attachments = vec!["pulled.html".into(), "broken.pdf".into()];
    fragment::create_fragment(&vault, &frag).unwrap();

    // Simulate a `git pull` landing attachments without sidecars
    let att_dir = vault.join("attachments").join(&frag.id);
    std::fs::create_dir_all(&att_dir).unwrap();
    std::fs::copy(fixture_dir().join("sample.html"), att_dir.join("pulled.html")).unwrap();
    std::fs::write(att_dir.join("broken.pdf"), b"%PDF-1.4\nthis is corrupted garbage\n").unwrap();

    // open_index syncs attachment_files as pending, never extracting or writing sidecars
    let conn = index::open_index(&vault).unwrap();
    assert!(extract::has_pending(&conn).unwrap());
    assert!(!att_dir.join(".text").exists());

    // Now fill extracts the good file and records the failure on the broken PDF
    let report = extract::fill(&vault, &conn).unwrap();
    assert_eq!(report.extracted, 1);
    assert_eq!(report.errors, 1);
    assert_eq!(report.remaining, 0);
    assert!(!extract::has_pending(&conn).unwrap());

    // Opening again does not re-mark the broken PDF as pending
    let conn2 = index::open_index(&vault).unwrap();
    assert!(!extract::has_pending(&conn2).unwrap());
    let statuses = extract::list_status(&conn2, &vault, &frag.id).unwrap();
    let broken = statuses.iter().find(|s| s.filename == "broken.pdf").unwrap();
    assert_eq!(broken.status, "error");
    assert!(broken.error.is_some());
}

#[test]
fn limits_refuse_zip_bomb_and_oversized_file() {
    // 1. Oversized file refused before extraction
    let mut tight = ExtractLimits::default();
    tight.max_file_bytes = 1024;
    let big = vec![b'a'; 2048];
    let err = in_process(&big, Some("big.txt"), &tight).unwrap_err();
    assert!(err.to_string().contains("too large"), "{err}");

    // 2. Zip bomb (crafted .docx whose word/document.xml compresses 4 MiB of zeros into ~5 KB -> >500:1 ratio)
    let bomb_bytes = make_docx_zip_bomb(4 * 1024 * 1024);
    assert!(
        bomb_bytes.len() < 32 * 1024,
        "bomb should be tiny compressed, got {} bytes",
        bomb_bytes.len()
    );
    let err = in_process(&bomb_bytes, Some("bomb.docx"), &ExtractLimits::default()).unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("bomb")
            || msg.contains("ratio")
            || msg.contains("security")
            || msg.contains("large")
            || msg.contains("failed"),
        "expected zip bomb refusal, got: {err}"
    );
}

#[test]
fn in_files_search_and_attachment_recall_name_pages() {
    let tmp = tempfile::TempDir::new().unwrap();
    let vault = tmp.path().join(".parc");
    parc_core::vault::init_vault(&vault).unwrap();

    let mut w = Writer::new(&vault, ScriptRunner::new(HookStderr::Capture));
    let cfg = parc_core::config::load_config(&vault).unwrap();
    let schemas = load_schemas(&vault).unwrap();
    let pdf_frag = w
        .create(new_fragment("note", "Winter filing", schemas.resolve("note").unwrap(), &cfg))
        .unwrap();
    let plain_frag = w
        .create(new_fragment("note", "Grocery list", schemas.resolve("note").unwrap(), &cfg))
        .unwrap();
    w.attach(&pdf_frag.id, &fixture_dir().join("sample.pdf"), AttachMode::Copy)
        .unwrap();

    let conn = index::open_index(&vault).unwrap();

    // Plain search without in:files must NOT match text that only exists inside attachments.
    let plain_q = parc_core::search::parse_query("icebreaker").unwrap();
    assert!(parc_core::search::search(&conn, &plain_q).unwrap().is_empty());

    // `in:files` finds the fragment and returns page-labeled file_hits.
    let files_q = parc_core::search::parse_query("in:files crew rotation").unwrap();
    let hits = parc_core::search::search(&conn, &files_q).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, pdf_frag.id);
    assert_eq!(hits[0].file_hits.len(), 1);
    assert_eq!(hits[0].file_hits[0].filename, "sample.pdf");
    assert_eq!(hits[0].file_hits[0].part, 2);
    assert_eq!(hits[0].file_hits[0].label, "p. 2");
    assert!(hits[0].file_hits[0].snippet.contains("crew"));

    // Recall picks up attachment full text and produces page-labeled excerpts.
    let ctx = parc_core::recall::context(
        &conn,
        &parc_core::recall::RecallQuery {
            seed: parc_core::recall::Seed::Draft {
                text: "Planning icebreaker escort and crew rotation for winter ferries".into(),
                tags: Vec::new(),
                id: Some(plain_frag.id.clone()),
            },
            as_of: None,
            limit: 5,
        },
        None,
    )
    .unwrap();
    assert_eq!(ctx.sources.len(), 1);
    assert_eq!(ctx.sources[0].id, pdf_frag.id);
    assert_eq!(
        ctx.sources[0].excerpts[0].attachment.as_deref(),
        Some("sample.pdf")
    );
    assert!(ctx.sources[0].excerpts[0].part.is_some());
    assert!(ctx.sources[0].excerpts[0]
        .label
        .as_deref()
        .unwrap()
        .starts_with("p. "));
}

/// Build a valid ZIP archive containing `[Content_Types].xml` and a deflate-compressed
/// `word/document.xml` that expands to `uncompressed_size` bytes of spaces (ratio > 500:1).
fn make_docx_zip_bomb(uncompressed_size: usize) -> Vec<u8> {
    let ct = b"<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"></Types>";
    let ct_crc = crc32(ct);

    let bomb_deflated = deflate_repeat_byte(b' ', uncompressed_size);
    let bomb_crc = crc32_repeat(b' ', uncompressed_size);

    let mut out = Vec::new();
    let mut central = Vec::new();

    // Entry 1: [Content_Types].xml (stored)
    write_zip_entry(
        &mut out,
        &mut central,
        b"[Content_Types].xml",
        0,
        ct_crc,
        ct.len() as u32,
        ct.len() as u32,
        ct,
    );
    // Entry 2: word/document.xml (deflated)
    write_zip_entry(
        &mut out,
        &mut central,
        b"word/document.xml",
        8,
        bomb_crc,
        bomb_deflated.len() as u32,
        uncompressed_size as u32,
        &bomb_deflated,
    );

    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    // End of central directory record
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // disk
    out.extend_from_slice(&0u16.to_le_bytes()); // cd disk
    out.extend_from_slice(&2u16.to_le_bytes()); // entries on disk
    out.extend_from_slice(&2u16.to_le_bytes()); // total entries
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    out
}

#[allow(clippy::too_many_arguments)]
fn write_zip_entry(
    out: &mut Vec<u8>,
    central: &mut Vec<u8>,
    name: &[u8],
    method: u16,
    crc: u32,
    comp_size: u32,
    uncomp_size: u32,
    data: &[u8],
) {
    let offset = out.len() as u32;
    // Local file header
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0u16.to_le_bytes()); // flags
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // mod time
    out.extend_from_slice(&0u16.to_le_bytes()); // mod date
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&comp_size.to_le_bytes());
    out.extend_from_slice(&uncomp_size.to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra len
    out.extend_from_slice(name);
    out.extend_from_slice(data);

    // Central directory header
    central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
    central.extend_from_slice(&20u16.to_le_bytes()); // version made by
    central.extend_from_slice(&20u16.to_le_bytes()); // version needed
    central.extend_from_slice(&0u16.to_le_bytes()); // flags
    central.extend_from_slice(&method.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes());
    central.extend_from_slice(&crc.to_le_bytes());
    central.extend_from_slice(&comp_size.to_le_bytes());
    central.extend_from_slice(&uncomp_size.to_le_bytes());
    central.extend_from_slice(&(name.len() as u16).to_le_bytes());
    central.extend_from_slice(&0u16.to_le_bytes()); // extra len
    central.extend_from_slice(&0u16.to_le_bytes()); // comment len
    central.extend_from_slice(&0u16.to_le_bytes()); // disk start
    central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
    central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
    central.extend_from_slice(&offset.to_le_bytes());
    central.extend_from_slice(name);
}

/// Emit a valid RFC 1951 fixed-Huffman raw deflate stream encoding `count`
/// repetitions of ASCII byte `b` (`b <= 143`).
fn deflate_repeat_byte(b: u8, count: usize) -> Vec<u8> {
    assert!(b <= 143 && count >= 1);
    let mut out = Vec::new();
    let mut bitbuf: u64 = 0;
    let mut bits: u32 = 0;
    let mut push_bits = |val: u32, n: u32| {
        bitbuf |= u64::from(val) << bits;
        bits += n;
        while bits >= 8 {
            out.push((bitbuf & 0xff) as u8);
            bitbuf >>= 8;
            bits -= 8;
        }
    };
    let reverse = |mut v: u32, n: u32| -> u32 {
        let mut r = 0;
        for _ in 0..n {
            r = (r << 1) | (v & 1);
            v >>= 1;
        }
        r
    };

    // BFINAL=1, BTYPE=01 (fixed Huffman)
    push_bits(1, 1);
    push_bits(1, 2);

    // Emit one literal `b`: codes 0..=143 are 8-bit 00110000 + b (MSB first).
    let lit_code = reverse(0b0011_0000 + u32::from(b), 8);
    push_bits(lit_code, 8);
    let mut remaining = count - 1;

    // Length 258 (symbol 285 -> 8-bit code 11000101), distance 1 (code 0 -> 5-bit 00000)
    let len258 = reverse(0b1100_0101, 8);
    while remaining >= 258 {
        push_bits(len258, 8);
        push_bits(0, 5);
        remaining -= 258;
    }
    while remaining > 0 {
        push_bits(lit_code, 8);
        remaining -= 1;
    }
    // End-of-block symbol 256 -> 7-bit 0000000
    push_bits(0, 7);
    if bits > 0 {
        out.push((bitbuf & 0xff) as u8);
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn crc32_repeat(b: u8, count: usize) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for _ in 0..count {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
