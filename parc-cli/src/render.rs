use parc_core::attachment::AttachmentInfo;
use parc_core::fragment::Fragment;
use parc_core::index::BacklinkInfo;
use parc_core::search::SearchResult;
use parc_core::tag;

pub fn print_table(results: &[SearchResult], id_len: usize) {
    // Header
    println!(
        "{:<width$}  {:<10}  {:<12}  {:<40}  TAGS",
        "ID",
        "TYPE",
        "STATUS",
        "TITLE",
        width = id_len
    );

    for result in results {
        let short_id = if result.id.len() > id_len {
            &result.id[..id_len]
        } else {
            &result.id
        };
        let status = result.status.as_deref().unwrap_or("\u{2014}");
        let title = if result.title.len() > 40 {
            format!("{}...", &result.title[..37])
        } else {
            result.title.clone()
        };
        let tags = result.tags.join(", ");

        println!(
            "{:<width$}  {:<10}  {:<12}  {:<40}  {}",
            short_id,
            result.fragment_type,
            status,
            title,
            tags,
            width = id_len
        );
    }
}

pub fn print_fragment(fragment: &Fragment, backlinks: &[BacklinkInfo], attachments: &[AttachmentInfo], id_len: usize) {
    let inline_tags = tag::extract_inline_tags(&fragment.body);
    let merged_tags = tag::merge_tags(&fragment.tags, &inline_tags);

    // Metadata header
    println!("--- {} ---", fragment.fragment_type);
    println!("ID:      {}", fragment.id);
    println!("Title:   {}", fragment.title);
    if !merged_tags.is_empty() {
        println!("Tags:    {}", merged_tags.join(", "));
    }
    if !fragment.links.is_empty() {
        println!("Links:   {}", fragment.links.join(", "));
    }
    for (key, val) in &fragment.extra_fields {
        if let Some(s) = val.as_str() {
            println!("{}: {}", capitalize(key), s);
        }
    }
    println!(
        "Created: {}",
        fragment
            .created_at
            .format("%Y-%m-%d %H:%M")
    );
    println!(
        "Updated: {}",
        fragment
            .updated_at
            .format("%Y-%m-%d %H:%M")
    );
    if let Some(ref by) = fragment.created_by {
        println!("By:      {}", by);
    }
    println!();

    // Body — render with termimad
    if !fragment.body.is_empty() {
        let skin = termimad::MadSkin::default();
        skin.print_text(&fragment.body);
    }

    // Backlinks section
    if !backlinks.is_empty() {
        println!();
        println!("\u{2500}\u{2500}\u{2500} Backlinks \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
        for bl in backlinks {
            let short = if bl.source_id.len() > id_len {
                &bl.source_id[..id_len]
            } else {
                &bl.source_id
            };
            println!("  {}  {}  {}", short, bl.source_type, bl.source_title);
        }
    }

    // Attachments section
    if !attachments.is_empty() {
        println!();
        println!("\u{2500}\u{2500}\u{2500} Attachments \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
        for a in attachments {
            let size = format_size(a.size);
            println!("  {} ({})", a.filename, size);
        }
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

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let upper: String = c.to_uppercase().collect();
            format!("{}{}", upper, chars.collect::<String>())
        }
    }
}
