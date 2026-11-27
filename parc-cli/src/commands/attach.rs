use std::path::Path;

use anyhow::{bail, Result};
use parc_core::attachment::AttachMode;
use parc_core::display::short_id;
use parc_core::extract::{self, PartUnit};
use parc_core::{fragment, index};

use crate::hooks::{cli_writer, report_warnings};
use crate::render::sanitize_terminal_text;

pub fn run_attach(vault: &Path, id: &str, file: &Path, mv: bool, json: bool) -> Result<()> {
    let id_len = parc_core::config::load_config(vault)?.id_display_length;
    let mut writer = cli_writer(vault);
    let mode = if mv {
        AttachMode::Move
    } else {
        AttachMode::Copy
    };
    let (frag, filename) = writer.attach(id, file, mode)?;
    report_warnings(&mut writer);

    if json {
        let json_val = serde_json::json!({
            "id": frag.id,
            "filename": filename,
            "attached": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!(
            "Attached '{}' to {}",
            sanitize_terminal_text(&filename),
            short_id(&frag.id, id_len)
        );
    }
    Ok(())
}

pub fn run_detach(vault: &Path, id: &str, filename: &str, json: bool) -> Result<()> {
    let id_len = parc_core::config::load_config(vault)?.id_display_length;
    let mut writer = cli_writer(vault);
    let frag = writer.detach(id, filename)?;
    report_warnings(&mut writer);

    if json {
        let json_val = serde_json::json!({
            "id": frag.id,
            "filename": filename,
            "detached": true,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!(
            "Detached '{}' from {}",
            sanitize_terminal_text(filename),
            short_id(&frag.id, id_len)
        );
    }
    Ok(())
}

pub fn run_attachments(
    vault: &Path,
    id: &str,
    text: Option<String>,
    pages: Option<String>,
    json: bool,
) -> Result<()> {
    let id_len = parc_core::config::load_config(vault)?.id_display_length;
    let full_id = fragment::resolve_id(vault, id)?;
    let conn = index::open_index(vault)?;

    if let Some(filename) = text {
        return run_attachment_text(vault, &conn, &full_id, &filename, pages.as_deref(), json);
    }

    if pages.is_some() {
        bail!("--pages requires --text <FILENAME>");
    }

    let attachments = extract::list_status(&conn, vault, &full_id)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&attachments)?);
    } else if attachments.is_empty() {
        println!(
            "No attachments for fragment {}.",
            short_id(&full_id, id_len)
        );
    } else {
        println!("{:<30}  {:>8}  TEXT", "FILENAME", "SIZE");
        for a in &attachments {
            let size = format_size(a.size);
            let status_str = format_text_status(a);
            println!(
                "{:<30}  {:>8}  {}",
                sanitize_terminal_text(&a.filename),
                size,
                sanitize_terminal_text(&status_str),
            );
        }
    }

    Ok(())
}

fn run_attachment_text(
    vault: &Path,
    conn: &rusqlite::Connection,
    full_id: &str,
    filename: &str,
    pages: Option<&str>,
    json: bool,
) -> Result<()> {
    let extracted = extract::read_parts(vault, conn, full_id, filename)?;
    let Some(ext) = extracted else {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "id": full_id,
                    "filename": filename,
                    "status": "no_text",
                    "total_parts": 0,
                    "parts": [],
                }))?
            );
        } else {
            println!(
                "No extracted text for '{}'.",
                sanitize_terminal_text(filename)
            );
        }
        return Ok(());
    };

    let total = ext.parts.len();
    let (from, to) = match pages {
        Some(spec) => parse_page_range(spec, total)?,
        None => (1, total),
    };

    let slice: Vec<serde_json::Value> = (from..=to)
        .filter_map(|p| {
            let text = ext.parts.get(p - 1)?;
            Some(serde_json::json!({
                "part": p,
                "label": ext.unit.format_ref(p, Some(text)),
                "text": text,
            }))
        })
        .collect();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "id": full_id,
                "filename": filename,
                "title": ext.title,
                "unit": ext.unit,
                "format": ext.format,
                "total_parts": total,
                "from": from,
                "to": to,
                "parts": slice,
            }))?
        );
    } else {
        for p in from..=to {
            if let Some(text) = ext.parts.get(p - 1) {
                if total > 1 || pages.is_some() {
                    let label = ext.unit.format_ref(p, Some(text));
                    println!("--- {} ---", sanitize_terminal_text(&label));
                }
                println!("{}", sanitize_terminal_text(text));
                if p < to {
                    println!();
                }
            }
        }
    }
    Ok(())
}

fn parse_page_range(spec: &str, total: usize) -> Result<(usize, usize)> {
    if total == 0 {
        return Ok((1, 0));
    }
    let spec = spec.trim();
    if let Some((a, b)) = spec.split_once('-') {
        let from = if a.trim().is_empty() {
            1
        } else {
            a.trim()
                .parse::<usize>()
                .map_err(|_| anyhow::anyhow!("invalid page range '{}'", spec))?
        };
        let to = if b.trim().is_empty() {
            total
        } else {
            b.trim()
                .parse::<usize>()
                .map_err(|_| anyhow::anyhow!("invalid page range '{}'", spec))?
        };
        if from == 0 || to == 0 || from > to {
            bail!("invalid page range '{}'", spec);
        }
        Ok((from.min(total), to.min(total)))
    } else {
        let n = spec
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("invalid page number '{}'", spec))?;
        if n == 0 || n > total {
            bail!("page {} out of range (1..={})", n, total);
        }
        Ok((n, n))
    }
}

fn format_text_status(a: &extract::AttachmentTextStatus) -> String {
    match a.status.as_str() {
        "ready" => {
            let unit_word = match (a.unit, a.parts) {
                (Some(PartUnit::Page), 1) => "page",
                (Some(PartUnit::Page), _) => "pages",
                (Some(PartUnit::Slide), 1) => "slide",
                (Some(PartUnit::Slide), _) => "slides",
                (Some(PartUnit::Section), 1) => "section",
                (Some(PartUnit::Section), _) => "sections",
                (None, 1) => "part",
                (None, _) => "parts",
            };
            format!("ready ({} {})", a.parts, unit_word)
        }
        "no_text" => "no text".to_string(),
        "error" => match &a.error {
            Some(msg) => format!("error: {msg}"),
            None => "error".to_string(),
        },
        other => other.to_string(),
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
