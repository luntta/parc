use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;

use anyhow::{bail, Result};
use parc_core::config::load_config;
use parc_core::recall::{self, RecallQuery, Seed};
use parc_core::{embed, fragment, index};

use crate::render::sanitize_terminal_text;

/// `parc recall`: the fragments related to a saved one (`id`) or to draft
/// text (`--text`, `-` for stdin), however old. With the meaning model
/// installed, vectors still missing are computed first, so the first run on a
/// large vault takes a while.
pub fn run(
    vault: &Path,
    id: Option<String>,
    text: Option<String>,
    tags: Vec<String>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let id = id
        .map(|prefix| fragment::resolve_id(vault, &prefix))
        .transpose()?;
    let text = match text.as_deref() {
        Some("-") => {
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            Some(input)
        }
        _ => text,
    };
    let seed = match (id, text) {
        (id, Some(text)) => Seed::Draft { text, tags, id },
        (Some(_), None) if !tags.is_empty() => {
            bail!("--tag describes draft text: use it with --text")
        }
        (Some(id), None) => Seed::Fragment(id),
        (None, None) => bail!("give a fragment ID, --text, or both"),
    };
    let draft_of = match &seed {
        Seed::Fragment(_) => None,
        Seed::Draft { id, .. } => Some(id.is_some()),
    };
    let conn = index::open_index(vault)?;
    let model = match embed::shared() {
        Ok(model) => model,
        Err(e) => {
            eprintln!("warning: {e}; recalling by words, tags and links only");
            None
        }
    };
    if let Some(model) = &model {
        read_for_meaning(&conn, model)?;
    }
    let ctx = recall::context(
        &conn,
        &RecallQuery {
            seed,
            as_of: None,
            limit,
        },
        model.as_deref(),
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&ctx)?);
        return Ok(());
    }

    let id_len = load_config(vault)?.id_display_length;
    let short = |id: &str| id[..id_len.min(id.len())].to_string();
    match (&ctx.seed.id, draft_of) {
        (Some(id), Some(true)) => println!("RELATED TO THE DRAFT OF {}", short(id)),
        (Some(id), _) => println!("RELATED TO {}", short(id)),
        (None, _) => println!("RELATED TO THE DRAFT"),
    }
    if ctx.sources.is_empty() {
        println!();
        println!("Nothing in the vault is clearly related.");
        return Ok(());
    }

    for s in &ctx.sources {
        println!();
        let mut kind = s.fragment_type.clone();
        if s.archived {
            kind.push_str(", archived");
        }
        println!(
            "{}  {:<10}  {}  ({})",
            short(&s.id),
            sanitize_terminal_text(&kind),
            sanitize_terminal_text(&s.title),
            s.created_at.get(..10).unwrap_or(&s.created_at),
        );
        let indent = " ".repeat(id_len + 2);
        let mut why = Vec::new();
        if s.reasons.linked {
            why.push("linked".to_string());
        }
        if !s.reasons.shared_tags.is_empty() {
            let tags: Vec<String> = s
                .reasons
                .shared_tags
                .iter()
                .map(|t| format!("#{t}"))
                .collect();
            why.push(format!("tags {}", tags.join(" ")));
        }
        if !s.reasons.shared_words.is_empty() {
            why.push(format!("words {}", s.reasons.shared_words.join(", ")));
        }
        if s.reasons.similar {
            why.push("similar meaning".to_string());
        }
        println!("{indent}{}", sanitize_terminal_text(&why.join("; ")));
        for e in &s.excerpts {
            if let Some(ref att) = e.attachment {
                let loc = e.label.as_deref().unwrap_or("");
                println!(
                    "{indent}{} ({}): {}",
                    sanitize_terminal_text(att),
                    sanitize_terminal_text(loc),
                    sanitize_terminal_text(&e.text)
                );
            } else {
                println!("{indent}{}: {}", e.line, sanitize_terminal_text(&e.text));
            }
        }
    }
    if ctx.truncated {
        println!();
        println!("More were found; raise --limit to see them.");
    }
    Ok(())
}

/// Compute the vectors the index lacks, on every core, saying how far along
/// it is when stderr is a terminal.
fn read_for_meaning(conn: &rusqlite::Connection, model: &embed::Model) -> Result<()> {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let tty = io::stderr().is_terminal();
    let mut shown = false;
    embed::fill(conn, model, None, threads, &mut |done, total| {
        if tty {
            eprint!("\rReading notes for meaning: {done} of {total}");
            let _ = io::stderr().flush();
            shown = true;
        }
    })?;
    if shown {
        eprintln!();
    }
    Ok(())
}
