use crate::error::ParcError;
use crate::fragment::Fragment;
use crate::tag;

/// Export fragments as a JSON array.
pub fn export_json(fragments: &[Fragment]) -> Result<String, ParcError> {
    let json_val: Vec<serde_json::Value> = fragments
        .iter()
        .map(|f| {
            let inline_tags = tag::extract_inline_tags(&f.body);
            let merged_tags = tag::merge_tags(&f.tags, &inline_tags);
            serde_json::json!({
                "id": f.id,
                "type": f.fragment_type,
                "title": f.title,
                "tags": merged_tags,
                "links": f.links,
                "attachments": f.attachments,
                "created_at": f.created_at.to_rfc3339(),
                "updated_at": f.updated_at.to_rfc3339(),
                "created_by": f.created_by,
                "extra_fields": f.extra_fields,
                "body": f.body,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&json_val)?)
}

/// Export fragments as CSV (metadata only, no body).
pub fn export_csv(fragments: &[Fragment]) -> Result<String, ParcError> {
    let mut output = String::new();
    output.push_str("id,type,title,tags,status,priority,due,created_at,updated_at\n");

    for f in fragments {
        let inline_tags = tag::extract_inline_tags(&f.body);
        let merged_tags = tag::merge_tags(&f.tags, &inline_tags);
        let tags_str = merged_tags.join(";");
        let status = f
            .extra_fields
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let priority = f
            .extra_fields
            .get("priority")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let due = f
            .extra_fields
            .get("due")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        output.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            csv_escape(&f.id),
            csv_escape(&f.fragment_type),
            csv_escape(&f.title),
            csv_escape(&tags_str),
            csv_escape(status),
            csv_escape(priority),
            csv_escape(due),
            csv_escape(&f.created_at.to_rfc3339()),
            csv_escape(&f.updated_at.to_rfc3339()),
        ));
    }

    Ok(output)
}

/// Export fragments as HTML files. Returns Vec of (filename, html_content).
pub fn export_html(fragments: &[Fragment]) -> Result<Vec<(String, String)>, ParcError> {
    let mut files = Vec::new();

    for f in fragments {
        let html_body = comrak::markdown_to_html(&f.body, &comrak::Options::default());
        let html = format!(
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{title}</title>\n\
             <style>body {{ font-family: sans-serif; max-width: 800px; margin: 2em auto; padding: 0 1em; }}</style>\n\
             </head>\n<body>\n<h1>{title}</h1>\n\
             <p><strong>Type:</strong> {ftype} | <strong>ID:</strong> {id}</p>\n\
             {html_body}\n</body>\n</html>",
            title = html_escape(&f.title),
            ftype = html_escape(&f.fragment_type),
            id = &f.id,
            html_body = html_body,
        );
        let filename = format!("{}.html", &f.id[..8.min(f.id.len())]);
        files.push((filename, html));
    }

    // Generate index.html
    let mut index_html = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Parc Export</title>\n\
         <style>body { font-family: sans-serif; max-width: 800px; margin: 2em auto; padding: 0 1em; }</style>\n\
         </head>\n<body>\n<h1>Parc Export</h1>\n<ul>\n",
    );
    for f in fragments {
        let short = &f.id[..8.min(f.id.len())];
        index_html.push_str(&format!(
            "<li><a href=\"{short}.html\">{title}</a> ({ftype})</li>\n",
            short = short,
            title = html_escape(&f.title),
            ftype = html_escape(&f.fragment_type),
        ));
    }
    index_html.push_str("</ul>\n</body>\n</html>");
    files.push(("index.html".to_string(), index_html));

    Ok(files)
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
