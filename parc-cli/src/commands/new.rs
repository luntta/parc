use std::path::Path;

use anyhow::{bail, Context, Result};
use parc_core::config::{get_editor, load_config};
use parc_core::fragment::{self, parse_fragment, serialize_fragment, validate_fragment};
use parc_core::hook::{self, HookEvent};
use parc_core::index;
use parc_core::schema::{load_schemas, load_template};
use parc_core::secure_fs;
use serde_json::Value;

use crate::hooks::CliHookRunner;
use crate::render::sanitize_terminal_text;

#[allow(clippy::too_many_arguments)]
pub fn run(
    vault: &Path,
    type_name: &str,
    title: Option<String>,
    body: Option<String>,
    tags: Vec<String>,
    links: Vec<String>,
    due: Option<String>,
    priority: Option<String>,
    status: Option<String>,
    assignee: Option<String>,
    json: bool,
) -> Result<()> {
    let config = load_config(vault)?;
    let schemas = load_schemas(vault)?;

    let schema = schemas
        .resolve(type_name)
        .ok_or_else(|| anyhow::anyhow!("unknown type: {}", type_name))?;

    let resolved_type = &schema.name.clone();
    let mut fragment = fragment::new_fragment(resolved_type, "", schema, &config);

    // Apply CLI arguments
    if let Some(ref t) = title {
        fragment.title = t.clone();
    }
    for tag in &tags {
        if !fragment.tags.contains(tag) {
            fragment.tags.push(tag.clone());
        }
    }
    fragment.links = links;

    // Apply type-specific fields
    if let Some(s) = status {
        fragment
            .extra_fields
            .insert("status".to_string(), Value::String(s));
    }
    if let Some(d) = due {
        let resolved = parc_core::date::resolve_due_date(&d)?;
        fragment
            .extra_fields
            .insert("due".to_string(), Value::String(resolved));
    }
    if let Some(p) = priority {
        fragment
            .extra_fields
            .insert("priority".to_string(), Value::String(p));
    }
    if let Some(a) = assignee {
        fragment
            .extra_fields
            .insert("assignee".to_string(), Value::String(a));
    }

    // Set body if provided via --body flag
    if let Some(ref b) = body {
        fragment.body = b.clone();
    }

    // Decide whether to open editor (--body skips editor)
    let should_open_editor = body.is_none() && (!schema.editor_skip || title.is_none());

    let runner = CliHookRunner;

    #[cfg(feature = "wasm-plugins")]
    let mut plugin_manager = parc_core::plugin::manager::PluginManager::load_all(vault, &config)
        .unwrap_or_else(|e| {
            eprintln!("Warning: failed to load plugins: {}", e);
            parc_core::plugin::manager::PluginManager::empty().unwrap()
        });

    if should_open_editor {
        // Prepare template content
        let template = load_template(vault, resolved_type).unwrap_or_default();

        // If we have a template, parse it and merge with our fragment data
        let editor_content = build_editor_content(&fragment, &template);

        let fragment = run_editor_loop(vault, &editor_content, schema, &config)?;

        // Run pre-create hooks
        #[cfg(feature = "wasm-plugins")]
        let fragment = hook::run_pre_hooks_with_plugins(
            &runner,
            vault,
            HookEvent::PreCreate,
            &fragment,
            &mut plugin_manager,
        )?;
        #[cfg(not(feature = "wasm-plugins"))]
        let fragment = hook::run_pre_hooks(&runner, vault, HookEvent::PreCreate, &fragment)?;

        fragment::create_fragment(vault, &fragment)?;

        // Index
        let conn = index::open_index(vault)?;
        index::index_fragment_auto(&conn, &fragment, vault)?;

        // Run post-create hooks
        #[cfg(feature = "wasm-plugins")]
        hook::run_post_hooks_with_plugins(
            &runner,
            vault,
            HookEvent::PostCreate,
            &fragment,
            &mut plugin_manager,
        );
        #[cfg(not(feature = "wasm-plugins"))]
        hook::run_post_hooks(&runner, vault, HookEvent::PostCreate, &fragment);

        print_result(&fragment, json)?;
    } else {
        // Skip editor — create directly
        validate_fragment(&fragment, schema)?;

        // Run pre-create hooks
        #[cfg(feature = "wasm-plugins")]
        let fragment = hook::run_pre_hooks_with_plugins(
            &runner,
            vault,
            HookEvent::PreCreate,
            &fragment,
            &mut plugin_manager,
        )?;
        #[cfg(not(feature = "wasm-plugins"))]
        let fragment = hook::run_pre_hooks(&runner, vault, HookEvent::PreCreate, &fragment)?;

        fragment::create_fragment(vault, &fragment)?;

        let conn = index::open_index(vault)?;
        index::index_fragment_auto(&conn, &fragment, vault)?;

        // Run post-create hooks
        #[cfg(feature = "wasm-plugins")]
        hook::run_post_hooks_with_plugins(
            &runner,
            vault,
            HookEvent::PostCreate,
            &fragment,
            &mut plugin_manager,
        );
        #[cfg(not(feature = "wasm-plugins"))]
        hook::run_post_hooks(&runner, vault, HookEvent::PostCreate, &fragment);

        print_result(&fragment, json)?;
    }

    Ok(())
}

fn build_editor_content(fragment: &fragment::Fragment, template: &str) -> String {
    // Start with the serialized fragment as the base
    let mut frag_for_editor = fragment.clone();

    // If template has body content, use that
    if let Ok((_, template_body)) = parse_template_parts(template) {
        if frag_for_editor.body.is_empty() && !template_body.trim().is_empty() {
            frag_for_editor.body = template_body;
        }
    }

    serialize_fragment(&frag_for_editor)
}

fn parse_template_parts(template: &str) -> Result<(String, String)> {
    let content = template.trim_start();
    if !content.starts_with("---") {
        return Ok((String::new(), template.to_string()));
    }
    let after = &content[3..];
    let after = after.trim_start_matches(['\r', '\n']);
    if let Some(end) = after.find("\n---") {
        let _frontmatter = &after[..end];
        let body_start = end + 4;
        let body = if body_start < after.len() {
            let rest = &after[body_start..];
            rest.strip_prefix('\n').unwrap_or(rest).to_string()
        } else {
            String::new()
        };
        Ok((_frontmatter.to_string(), body))
    } else {
        Ok((String::new(), template.to_string()))
    }
}

fn run_editor_loop(
    vault: &Path,
    initial_content: &str,
    schema: &parc_core::schema::Schema,
    config: &parc_core::config::Config,
) -> Result<fragment::Fragment> {
    let editor = get_editor(config);
    let tmp_path = secure_fs::write_private_temp("parc", ".md", initial_content.as_bytes())?;

    let mut last_error: Option<String> = None;

    loop {
        if let Some(ref err) = last_error {
            eprintln!("Validation error: {}", sanitize_terminal_text(err));
            eprintln!("Re-opening editor. Save empty file to abort.");
        }

        let status = std::process::Command::new(&editor)
            .arg(&tmp_path)
            .status()
            .with_context(|| format!("failed to launch editor: {}", editor))?;

        if !status.success() {
            let _ = std::fs::remove_file(&tmp_path);
            bail!("editor exited with non-zero status");
        }

        let content = std::fs::read_to_string(&tmp_path)?;

        // Abort if empty or unchanged
        if content.trim().is_empty() {
            let _ = std::fs::remove_file(&tmp_path);
            bail!("aborted: empty content");
        }

        match parse_fragment(&content) {
            Ok(frag) => {
                // Validate
                let actual_schema = parc_core::schema::load_schemas(vault)?;
                let s = actual_schema.resolve(&frag.fragment_type).unwrap_or(schema);
                match validate_fragment(&frag, s) {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&tmp_path);
                        return Ok(frag);
                    }
                    Err(e) => {
                        last_error = Some(e.to_string());
                    }
                }
            }
            Err(e) => {
                last_error = Some(e.to_string());
            }
        }
    }
}

fn print_result(fragment: &fragment::Fragment, json: bool) -> Result<()> {
    if json {
        let json_val = serde_json::json!({
            "id": fragment.id,
            "type": fragment.fragment_type,
            "title": fragment.title,
        });
        println!("{}", serde_json::to_string_pretty(&json_val)?);
    } else {
        println!("{}", fragment.id);
    }
    Ok(())
}
