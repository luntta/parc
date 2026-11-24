//! A Model Context Protocol (MCP) server over stdio, so agent harnesses
//! (Claude Code, Codex, editors) can search the vault and record notes,
//! todos, decisions, risks and ideas in it.
//!
//! It is a thin layer over the JSON-RPC methods: writes go through the same
//! `ops::Writer` path (schema validation, hooks, history) as the CLI, the TUI
//! and the desktop app. Tool results are plain text written for a model to
//! read, with full fragment IDs. `read_only` leaves the write tools out.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use parc_core::{fragment, recall};

use crate::jsonrpc::{Response, RpcError};
use crate::methods;

/// Protocol revisions this server speaks, newest first. A tools-only server
/// needs nothing that differs between them.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 100;

const INSTRUCTIONS: &str = "parc is the user's local archive of notes, todos, decisions, risks and ideas (Markdown files with YAML frontmatter). \
Search it for earlier decisions, known risks and open todos before starting work on a topic, and respect what you find. \
Record durable, user-relevant context: a decision and why it was made, a risk and how to mitigate it, follow-up work. Don't log routine progress. \
Search before creating to avoid duplicates; prefer updating or linking an existing fragment. \
Call list_types for each type's fields and allowed values, and list_tags to reuse existing tags. \
IDs are ULIDs; any unique prefix works as an ID. Never write secrets into fragments.";

const SEARCH_HELP: &str = "Search the vault. Plain words are full-text search (\"quoted phrase\" for exact). Filters, all AND-ed: \
type:todo, status:open, priority:high (also priority:>=medium), tag:backend or #backend, \
due:today|this-week|overdue|<date> (also >2026-01-01), created:<range>, updated:<range>, by:<author>, \
has:links|attachments|due, linked:<id>, is:archived (archived fragments are left out unless asked for) or is:all. \
Prefix a value with ! to negate it (status:!done). An empty query lists the most recently updated fragments.";

pub struct McpServer {
    vault: PathBuf,
    read_only: bool,
}

impl McpServer {
    pub fn new(vault: PathBuf, read_only: bool) -> Self {
        Self { vault, read_only }
    }

    /// Handle one JSON-RPC message and return the reply to send, if any:
    /// notifications, and responses to requests (this server sends none),
    /// get no reply.
    pub fn handle(&self, message: Value) -> Option<Value> {
        let Some(obj) = message.as_object() else {
            return Some(reply(Value::Null, Err(RpcError::invalid_request())));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            if obj.contains_key("result") || obj.contains_key("error") {
                return None;
            }
            return Some(reply(
                id.unwrap_or(Value::Null),
                Err(RpcError::invalid_request()),
            ));
        };
        let id = id?;
        let params = obj.get("params").cloned().unwrap_or_else(|| json!({}));

        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.tools() })),
            "tools/call" => self.call(&params),
            _ => Err(RpcError::method_not_found(method)),
        };
        Some(reply(id, result))
    }

    fn tools(&self) -> Vec<Value> {
        let mut tools = vec![
            search_tool(),
            recall_tool(),
            get_tool(),
            types_tool(),
            tags_tool(),
        ];
        if !self.read_only {
            tools.extend([create_tool(), update_tool(), link_tool()]);
        }
        tools
    }

    fn call(&self, params: &Value) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::invalid_params("tools/call needs a tool name"))?;
        let empty = Map::new();
        let args = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(args)) => args,
            Some(_) => return Err(RpcError::invalid_params("arguments must be an object")),
        };
        let vault = self.vault.as_path();
        let outcome = match name {
            "search" => search(vault, args),
            "recall" => recall(vault, args),
            "get_fragment" => get_fragment(vault, args),
            "list_types" => list_types(vault, args),
            "list_tags" => list_tags(vault, args),
            "create_fragment" if !self.read_only => create_fragment(vault, args),
            "update_fragment" if !self.read_only => update_fragment(vault, args),
            "link_fragments" if !self.read_only => link_fragments(vault, args),
            _ => {
                return Err(RpcError::invalid_params(&format!("unknown tool: {}", name)));
            }
        };
        // A tool that ran and failed is a result the model should see, not a
        // protocol error.
        Ok(match outcome {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
            Err(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
        })
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "parc", "title": "parc", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn reply(id: Value, result: Result<Value, RpcError>) -> Value {
    let response = match result {
        Ok(value) => Response::success(id, value),
        Err(error) => Response::error(id, error),
    };
    serde_json::to_value(response).expect("a response serializes")
}

// ── Tool definitions ───────────────────────────────────────────────────

fn tool(name: &str, title: &str, description: &str, schema: Value, annotations: Value) -> Value {
    let mut annotations = annotations;
    annotations["title"] = json!(title);
    annotations["openWorldHint"] = json!(false);
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": schema,
        "annotations": annotations,
    })
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn read_only_hints() -> Value {
    json!({ "readOnlyHint": true })
}

fn search_tool() -> Value {
    tool(
        "search",
        "Search parc",
        SEARCH_HELP,
        object(
            json!({
                "query": { "type": "string", "description": "Words and filters, e.g. `type:decision #search` or `status:open due:this-week`." },
                "limit": { "type": "integer", "minimum": 1, "maximum": MAX_LIMIT, "description": "Most results to return (default 20)." },
            }),
            &[],
        ),
        read_only_hints(),
    )
}

fn recall_tool() -> Value {
    tool(
        "recall",
        "Recall related fragments",
        "Find fragments related to a saved fragment or to text being written, however long ago they were written. \
Each comes with why it matched (shared words, tags, links), its revision and short verbatim excerpts with line numbers. \
Pass `id` for a saved fragment, or `text` for a draft (with `id` too when the draft edits that fragment). \
No results means nothing is clearly related: say so rather than guess. \
Matching is by shared words, tags and links, so a note that says the same thing in other words can be missed.",
        object(
            json!({
                "id": { "type": "string", "description": "Fragment ID or unique prefix." },
                "text": { "type": "string", "description": "Draft title and body. Its #tags and [[links]] count." },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "The draft's tags, with text." },
                "limit": { "type": "integer", "minimum": 1, "maximum": recall::MAX_LIMIT, "description": "Most fragments to return (default 5)." },
            }),
            &[],
        ),
        read_only_hints(),
    )
}

fn get_tool() -> Value {
    tool(
        "get_fragment",
        "Read a fragment",
        "Read one fragment in full: its Markdown file (frontmatter and body), its revision, and the fragments that link to it.",
        object(
            json!({ "id": { "type": "string", "description": "Fragment ID or unique prefix." } }),
            &["id"],
        ),
        read_only_hints(),
    )
}

fn types_tool() -> Value {
    tool(
        "list_types",
        "List fragment types",
        "List the vault's fragment types with their fields, allowed values and defaults. Use it before creating or updating fields.",
        object(json!({}), &[]),
        read_only_hints(),
    )
}

fn tags_tool() -> Value {
    tool(
        "list_tags",
        "List tags",
        "List every tag in use with how many fragments carry it, most used first.",
        object(json!({}), &[]),
        read_only_hints(),
    )
}

fn create_tool() -> Value {
    tool(
        "create_fragment",
        "Create a fragment",
        "Create a note, todo, decision, risk, idea or other vault type. Fields the type defines (status, priority, due, …) go in `fields`; see list_types. \
         Body is Markdown; #hashtags in it become tags and [[id-prefix]] or [[Title]] link to other fragments.",
        object(
            json!({
                "type": { "type": "string", "description": "Type name or alias, e.g. note, todo, decision, risk, idea." },
                "title": { "type": "string" },
                "body": { "type": "string", "description": "Markdown body." },
                "tags": { "type": "array", "items": { "type": "string" } },
                "links": { "type": "array", "items": { "type": "string" }, "description": "IDs (or prefixes) of fragments to link to." },
                "fields": { "type": "object", "description": "Type-specific fields, e.g. {\"status\": \"open\", \"priority\": \"high\", \"due\": \"friday\"}. Dates accept words like today, tomorrow, friday, next-week." },
            }),
            &["type", "title"],
        ),
        json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false }),
    )
}

fn update_tool() -> Value {
    tool(
        "update_fragment",
        "Update a fragment",
        "Change a fragment's title, body, tags or fields. `body` replaces the whole body; `append` adds a paragraph to the end instead. \
         `fields` sets type-specific fields; null removes an optional one. Pass the revision from get_fragment as `expected_revision` \
         to refuse the update if the fragment was saved since. Earlier versions stay in the fragment's history.",
        object(
            json!({
                "id": { "type": "string", "description": "Fragment ID or unique prefix." },
                "title": { "type": "string" },
                "body": { "type": "string", "description": "New Markdown body, replacing the old one." },
                "append": { "type": "string", "description": "Markdown to add at the end of the body." },
                "tags": { "type": "array", "items": { "type": "string" }, "description": "Replaces the frontmatter tags." },
                "fields": { "type": "object", "description": "Type-specific fields to set, e.g. {\"status\": \"done\"}." },
                "expected_revision": { "type": "string" },
            }),
            &["id"],
        ),
        json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false }),
    )
}

fn link_tool() -> Value {
    tool(
        "link_fragments",
        "Link two fragments",
        "Link two fragments to each other (both directions), e.g. a decision to the risk it addresses.",
        object(
            json!({
                "a": { "type": "string", "description": "Fragment ID or unique prefix." },
                "b": { "type": "string", "description": "Fragment ID or unique prefix." },
            }),
            &["a", "b"],
        ),
        json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true }),
    )
}

// ── Tool implementations ───────────────────────────────────────────────

type ToolResult = Result<String, String>;

/// Refuse arguments the tool doesn't take, so a misspelt `content` for
/// `body` is an error the model can fix rather than silently dropped text.
fn check_args(args: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for key in args.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "unknown argument '{}'; this tool takes: {}",
                key,
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

fn string_arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(format!("'{}' must be a string", key)),
    }
}

fn required_string<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    string_arg(args, key)?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("'{}' is required", key))
}

/// Resolve an ID or prefix to a full ID. An ambiguous prefix lists what it
/// matches, so the model can pick one without another search.
fn resolve_id(vault: &Path, id: &str) -> Result<String, String> {
    match fragment::resolve_id(vault, id) {
        Ok(full) => Ok(full),
        Err(parc_core::ParcError::AmbiguousId(prefix, count)) => {
            let upper = prefix.to_uppercase();
            let mut text = format!(
                "'{}' matches {} fragments; use a longer prefix:",
                prefix, count
            );
            let ids = fragment::list_fragment_ids(vault).map_err(|e| e.to_string())?;
            for candidate in ids.iter().filter(|c| c.starts_with(&upper)).take(10) {
                let title = fragment::read_fragment(vault, candidate)
                    .map(|f| format!("[{}]  {}", f.fragment_type, f.title))
                    .unwrap_or_default();
                text.push_str(&format!("\n- {}  {}", candidate, title));
            }
            Err(text)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn rpc_message(e: RpcError) -> String {
    match e.data {
        Some(Value::String(detail)) => detail,
        _ => e.message,
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn tags_text(value: &Value) -> String {
    value
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(|t| format!("#{}", t))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

fn warnings_text(value: &Value) -> String {
    value
        .get("warnings")
        .and_then(Value::as_array)
        .map(|w| {
            w.iter()
                .filter_map(Value::as_str)
                .map(|w| format!("\nwarning: {}", w))
                .collect()
        })
        .unwrap_or_default()
}

fn search(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(args, &["query", "limit"])?;
    let query = string_arg(args, "query")?.unwrap_or("").trim();
    let limit = match args.get("limit") {
        None | Some(Value::Null) => DEFAULT_LIMIT,
        Some(v) => v
            .as_u64()
            .filter(|n| *n >= 1)
            .ok_or("'limit' must be a positive integer")?
            .min(MAX_LIMIT),
    };
    let results = methods::fragment::search(vault, json!({ "query": query, "limit": limit }))
        .map_err(rpc_message)?;
    let results = results.as_array().cloned().unwrap_or_default();
    if results.is_empty() {
        return Ok(if query.is_empty() {
            "The vault has no fragments yet.".to_string()
        } else {
            format!("No fragments match `{}`.", query)
        });
    }

    let mut text = format!(
        "{} fragment{}{}:\n",
        results.len(),
        if results.len() == 1 { "" } else { "s" },
        if results.len() as u64 == limit {
            " (limit reached; narrow the query or raise limit)"
        } else {
            ""
        }
    );
    for r in &results {
        let mut kind = str_field(r, "type").to_string();
        if let Some(status) = r.get("status").and_then(Value::as_str) {
            kind = format!("{}, {}", kind, status);
        }
        if r.get("archived").and_then(Value::as_bool) == Some(true) {
            kind.push_str(", archived");
        }
        let updated = str_field(r, "updated_at");
        text.push_str(&format!(
            "\n{}  [{}]  {}",
            str_field(r, "id"),
            kind,
            str_field(r, "title")
        ));
        let tags = tags_text(r);
        if !tags.is_empty() {
            text.push_str(&format!("  {}", tags));
        }
        text.push_str(&format!(
            "  (updated {})",
            updated.get(..10).unwrap_or(updated)
        ));
        if let Some(snippet) = r.get("snippet").and_then(Value::as_str) {
            let snippet = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
            if !snippet.is_empty() {
                text.push_str(&format!("\n    {}", snippet));
            }
        }
    }
    Ok(text)
}

fn recall(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(args, &["id", "text", "tags", "limit"])?;
    let mut params = json!({});
    if let Some(id) = string_arg(args, "id")?.filter(|s| !s.trim().is_empty()) {
        params["id"] = json!(resolve_id(vault, id)?);
    }
    if let Some(text) = string_arg(args, "text")? {
        params["text"] = json!(text);
    }
    for key in ["tags", "limit"] {
        if let Some(value) = args.get(key).filter(|v| !v.is_null()) {
            params[key] = value.clone();
        }
    }
    let ctx = methods::recall::context(vault, params).map_err(rpc_message)?;
    let sources = ctx["sources"].as_array().cloned().unwrap_or_default();
    if sources.is_empty() {
        let searched = ctx["seed"]["words"].as_array().map_or(0, Vec::len);
        return Ok(
            if searched == 0 && ctx["seed"]["tags"].as_array().is_none_or(Vec::is_empty) {
                "Nothing to go on yet: the text has no words to search for.".to_string()
            } else {
                "Nothing in the vault is clearly related.".to_string()
            },
        );
    }
    let mut text = format!(
        "{} related fragment{}{}:\n",
        sources.len(),
        if sources.len() == 1 { "" } else { "s" },
        if ctx["truncated"] == true {
            " (more were found; raise limit to see them)"
        } else {
            ""
        }
    );
    for s in &sources {
        let mut kind = str_field(s, "type").to_string();
        if let Some(status) = s.get("status").and_then(Value::as_str) {
            kind = format!("{}, {}", kind, status);
        }
        if s["archived"] == true {
            kind.push_str(", archived");
        }
        let created = str_field(s, "created_at");
        text.push_str(&format!(
            "\n{}  [{}]  {}  (created {}, revision {})",
            str_field(s, "id"),
            kind,
            str_field(s, "title"),
            created.get(..10).unwrap_or(created),
            str_field(s, "revision"),
        ));
        let reasons = &s["reasons"];
        let list = |key: &str| -> Vec<String> {
            reasons[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut why = Vec::new();
        if reasons["linked"] == true {
            why.push("linked".to_string());
        }
        let tags = list("shared_tags");
        if !tags.is_empty() {
            why.push(format!(
                "tags {}",
                tags.iter()
                    .map(|t| format!("#{t}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        let words = list("shared_words");
        if !words.is_empty() {
            why.push(format!("words {}", words.join(", ")));
        }
        text.push_str(&format!("\n    why: {}", why.join("; ")));
        for e in s["excerpts"].as_array().into_iter().flatten() {
            let column = e["column"].as_u64().unwrap_or(1);
            let at = if column > 1 {
                format!("line {}, from column {}", e["line"], column)
            } else {
                format!("line {}", e["line"])
            };
            text.push_str(&format!("\n    {}: \"{}\"", at, str_field(e, "text")));
        }
    }
    Ok(text)
}

fn get_fragment(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(args, &["id"])?;
    let id = resolve_id(vault, required_string(args, "id")?)?;
    let frag = fragment::read_fragment(vault, &id).map_err(|e| e.to_string())?;
    let revision = fragment::revision(vault, &frag.id).map_err(|e| e.to_string())?;
    let backlinks =
        methods::link::backlinks(vault, json!({ "id": frag.id })).map_err(rpc_message)?;

    let mut text = format!(
        "Fragment {} ({}), file fragments/{}.md, revision {}\n\n{}",
        frag.id,
        frag.fragment_type,
        frag.id,
        revision,
        fragment::serialize_fragment(&frag).trim_end()
    );
    let backlinks = backlinks.as_array().cloned().unwrap_or_default();
    if !backlinks.is_empty() {
        text.push_str("\n\nLinked from:");
        for b in &backlinks {
            text.push_str(&format!(
                "\n- {}  [{}]  {}",
                str_field(b, "id"),
                str_field(b, "type"),
                str_field(b, "title")
            ));
        }
    }
    Ok(text)
}

fn list_types(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(args, &[])?;
    let schemas = methods::schema::list(vault, json!({})).map_err(rpc_message)?;
    let mut text =
        String::from("Fragment types (the body and title, tags and links are common to all):\n");
    for schema in schemas.as_array().into_iter().flatten() {
        text.push_str(&format!("\n{}", str_field(schema, "name")));
        if let Some(alias) = schema.get("alias").and_then(Value::as_str) {
            text.push_str(&format!(" (alias {})", alias));
        }
        if schema.get("enabled").and_then(Value::as_bool) == Some(false) {
            text.push_str(
                " [turned off: existing fragments can change, new ones can't be created]",
            );
        }
        let fields = schema
            .get("fields")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if fields.is_empty() {
            text.push_str(": no extra fields");
        }
        for field in &fields {
            let kind = match field.get("values").and_then(Value::as_array) {
                Some(values) => values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" | "),
                None => str_field(field, "kind").to_string(),
            };
            text.push_str(&format!("\n  {}: {}", str_field(field, "name"), kind));
            if field.get("required").and_then(Value::as_bool) == Some(true) {
                text.push_str(", required");
            }
            if let Some(default) = field.get("default").and_then(Value::as_str) {
                text.push_str(&format!(", default {}", default));
            }
        }
    }
    Ok(text)
}

fn list_tags(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(args, &[])?;
    let tags = methods::tags::list(vault, json!({})).map_err(rpc_message)?;
    let tags = tags.as_array().cloned().unwrap_or_default();
    if tags.is_empty() {
        return Ok("No tags in use yet.".to_string());
    }
    Ok(tags
        .iter()
        .map(|t| {
            format!(
                "#{} ({})",
                str_field(t, "tag"),
                t.get("count").and_then(Value::as_u64).unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

fn fields_arg(args: &Map<String, Value>) -> Result<Value, String> {
    match args.get("fields") {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::Object(fields)) => Ok(Value::Object(fields.clone())),
        Some(_) => Err("'fields' must be an object".into()),
    }
}

fn create_fragment(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(args, &["type", "title", "body", "tags", "links", "fields"])?;
    let mut params = json!({
        "type": required_string(args, "type")?,
        "title": required_string(args, "title")?,
        "fields": fields_arg(args)?,
    });
    for key in ["body", "tags", "links"] {
        if let Some(value) = args.get(key).filter(|v| !v.is_null()) {
            params[key] = value.clone();
        }
    }
    let saved = methods::fragment::create(vault, params).map_err(rpc_message)?;
    Ok(format!(
        "Created {} {}: {} (revision {}){}",
        str_field(&saved, "type"),
        str_field(&saved, "id"),
        str_field(&saved, "title"),
        str_field(&saved, "revision"),
        warnings_text(&saved)
    ))
}

fn update_fragment(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(
        args,
        &[
            "id",
            "title",
            "body",
            "append",
            "tags",
            "fields",
            "expected_revision",
        ],
    )?;
    let id = resolve_id(vault, required_string(args, "id")?)?;
    let mut params = json!({ "id": id, "fields": fields_arg(args)? });
    for key in ["title", "body", "tags", "expected_revision"] {
        if let Some(value) = args.get(key).filter(|v| !v.is_null()) {
            params[key] = value.clone();
        }
    }

    if let Some(append) = string_arg(args, "append")? {
        if args.get("body").is_some_and(|v| !v.is_null()) {
            return Err("pass either 'body' or 'append', not both".into());
        }
        let frag = fragment::read_fragment(vault, &id).map_err(|e| e.to_string())?;
        let revision = fragment::revision(vault, &frag.id).map_err(|e| e.to_string())?;
        let body = frag.body.trim_end();
        params["body"] = json!(if body.is_empty() {
            append.trim().to_string()
        } else {
            format!("{}\n\n{}", body, append.trim())
        });
        // The body was built from this revision: never write it over a newer save.
        if params.get("expected_revision").is_none() {
            params["expected_revision"] = json!(revision);
        }
    }

    let saved = methods::fragment::update(vault, params).map_err(|e| {
        if e.code == crate::jsonrpc::CONFLICT {
            "the fragment was saved since that revision; read it again with get_fragment and retry"
                .to_string()
        } else {
            rpc_message(e)
        }
    })?;
    Ok(format!(
        "Updated {} {}: {} (revision {}){}",
        str_field(&saved, "type"),
        str_field(&saved, "id"),
        str_field(&saved, "title"),
        str_field(&saved, "revision"),
        warnings_text(&saved)
    ))
}

fn link_fragments(vault: &Path, args: &Map<String, Value>) -> ToolResult {
    check_args(args, &["a", "b"])?;
    let a = resolve_id(vault, required_string(args, "a")?)?;
    let b = resolve_id(vault, required_string(args, "b")?)?;
    let result =
        methods::link::link(vault, json!({ "id_a": a, "id_b": b })).map_err(rpc_message)?;
    let ids: Vec<&str> = result
        .get("linked")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Ok(format!(
        "Linked {} and {} (both directions).{}",
        ids.first().copied().unwrap_or(&a),
        ids.get(1).copied().unwrap_or(&b),
        warnings_text(&result)
    ))
}

// ── Transport ──────────────────────────────────────────────────────────

/// Serve MCP over stdin/stdout: one JSON-RPC message per line. Logs belong on
/// stderr; stdout carries only protocol messages.
pub fn run_stdio(vault: PathBuf, read_only: bool) -> anyhow::Result<()> {
    let server = McpServer::new(vault, read_only);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(&server, &mut stdin.lock(), &mut stdout.lock())
}

/// The stdio loop over any reader and writer, for tests.
pub fn serve(
    server: &McpServer,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> anyhow::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = input
            .by_ref()
            .take(MAX_MESSAGE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(());
        }
        let reply = if line.len() > MAX_MESSAGE_BYTES && !line.ends_with(b"\n") {
            skip_line(input)?;
            Some(self::reply(
                Value::Null,
                Err(RpcError::invalid_params("message too large")),
            ))
        } else {
            let text = String::from_utf8_lossy(&line);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(text) {
                Ok(message) => server.handle(message),
                Err(_) => Some(self::reply(Value::Null, Err(RpcError::parse_error()))),
            }
        };
        if let Some(reply) = reply {
            serde_json::to_writer(&mut *output, &reply)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

/// Discard input up to and including the next newline.
fn skip_line(input: &mut impl BufRead) -> std::io::Result<()> {
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(());
        }
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                input.consume(i + 1);
                return Ok(());
            }
            None => {
                let len = buf.len();
                input.consume(len);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(read_only: bool) -> (tempfile::TempDir, McpServer) {
        let tmp = tempfile::TempDir::new().unwrap();
        let vault = tmp.path().join(".parc");
        parc_core::vault::init_vault(&vault).unwrap();
        (tmp, McpServer::new(vault, read_only))
    }

    fn request(server: &McpServer, method: &str, params: Value) -> Value {
        server
            .handle(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
            .expect("a request gets a reply")
    }

    /// Call a tool; returns (text, is_error).
    fn call(server: &McpServer, name: &str, arguments: Value) -> (String, bool) {
        let reply = request(
            server,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        let result = &reply["result"];
        assert!(result.is_object(), "{reply}");
        (
            result["content"][0]["text"].as_str().unwrap().to_string(),
            result["isError"].as_bool().unwrap_or(false),
        )
    }

    fn created_id(text: &str) -> String {
        // "Created <type> <ID>: <title> …"
        text.split_whitespace()
            .nth(2)
            .unwrap()
            .trim_end_matches(':')
            .to_string()
    }

    #[test]
    fn initialize_agrees_on_a_protocol_version() {
        let (_tmp, server) = server(false);
        let reply = request(
            &server,
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
        );
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(reply["result"]["serverInfo"]["name"], "parc");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        assert!(reply["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("Search"));

        let reply = request(
            &server,
            "initialize",
            json!({ "protocolVersion": "1999-01-01" }),
        );
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn notifications_and_client_responses_get_no_reply() {
        let (_tmp, server) = server(false);
        assert!(server
            .handle(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .is_none());
        assert!(server
            .handle(json!({ "jsonrpc": "2.0", "id": 7, "result": {} }))
            .is_none());
        assert_eq!(request(&server, "ping", json!({}))["result"], json!({}));
        assert_eq!(
            request(&server, "resources/list", json!({}))["error"]["code"],
            -32601
        );
    }

    #[test]
    fn read_only_leaves_out_the_write_tools() {
        let (_tmp, full) = server(false);
        let (_tmp2, read_only) = server(true);
        let names = |s: &McpServer| -> Vec<String> {
            request(s, "tools/list", json!({}))["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    assert_eq!(t["inputSchema"]["type"], "object");
                    t["name"].as_str().unwrap().to_string()
                })
                .collect()
        };
        assert_eq!(names(&full).len(), 8);
        assert_eq!(
            names(&read_only),
            vec![
                "search",
                "recall",
                "get_fragment",
                "list_types",
                "list_tags"
            ]
        );

        let reply = request(
            &read_only,
            "tools/call",
            json!({ "name": "create_fragment", "arguments": { "type": "note", "title": "x" } }),
        );
        assert_eq!(reply["error"]["code"], -32602);
    }

    #[test]
    fn create_search_read_update_and_link() {
        let (_tmp, server) = server(false);

        let (text, error) = call(
            &server,
            "create_fragment",
            json!({
                "type": "decision",
                "title": "Use SQLite for the index",
                "body": "Files stay the source of truth. #storage",
                "fields": { "status": "accepted" },
            }),
        );
        assert!(!error, "{text}");
        let decision = created_id(&text);

        let (text, error) = call(
            &server,
            "create_fragment",
            json!({ "type": "risk", "title": "Index drifts from files", "tags": ["storage"] }),
        );
        assert!(!error, "{text}");
        let risk = created_id(&text);

        let (text, _) = call(
            &server,
            "search",
            json!({ "query": "type:decision source of truth" }),
        );
        assert!(text.contains(&decision), "{text}");
        assert!(text.contains("[decision, accepted]"), "{text}");
        assert!(!text.contains(&risk));

        let (text, _) = call(&server, "list_tags", json!({}));
        assert!(text.contains("#storage (2)"), "{text}");

        let (text, error) = call(
            &server,
            "link_fragments",
            json!({ "a": &decision[..12], "b": risk }),
        );
        assert!(!error, "{text}");
        let (text, _) = call(&server, "get_fragment", json!({ "id": risk }));
        assert!(
            text.contains("Linked from:") && text.contains(&decision),
            "{text}"
        );

        let (text, error) = call(
            &server,
            "update_fragment",
            json!({ "id": risk, "append": "Mitigated: open_index syncs first.", "fields": { "status": "resolved" } }),
        );
        assert!(!error, "{text}");
        let (text, _) = call(&server, "get_fragment", json!({ "id": risk }));
        assert!(text.contains("status: resolved"), "{text}");
        assert!(
            text.contains("Mitigated: open_index syncs first."),
            "{text}"
        );
    }

    #[test]
    fn recall_quotes_related_fragments_for_a_draft_or_a_saved_one() {
        let (_tmp, server) = server(false);
        let (text, _) = call(
            &server,
            "create_fragment",
            json!({ "type": "note", "title": "What helped at the ferry desk",
                    "body": "The queue was watching.\nA route card to mark in private worked better." }),
        );
        let ferry = created_id(&text);
        let (text, _) = call(
            &server,
            "create_fragment",
            json!({ "type": "note", "title": "Orientation", "body": "Try private route cards." }),
        );
        let orientation = created_id(&text);

        let (text, is_error) = call(
            &server,
            "recall",
            json!({ "text": "Nobody asks in front of everyone. Private route cards?", "limit": 1 }),
        );
        assert!(!is_error, "{text}");
        assert!(
            text.starts_with("1 related fragment (more were found"),
            "{text}"
        );
        assert!(text.contains("why: words cards, private, route"), "{text}");

        let (text, _) = call(&server, "recall", json!({ "id": &orientation[..12] }));
        assert!(text.contains(&ferry), "{text}");
        assert!(
            text.contains("line 2: \"A route card to mark in private worked better.\""),
            "{text}"
        );
        assert!(
            !text.contains(&format!("\n{orientation}")),
            "never itself: {text}"
        );

        let (text, is_error) = call(&server, "recall", json!({ "text": "kitchen timer broken" }));
        assert!(!is_error);
        assert_eq!(text, "Nothing in the vault is clearly related.");
        let (text, is_error) = call(&server, "recall", json!({}));
        assert!(is_error, "{text}");
    }

    #[test]
    fn failures_are_results_the_model_can_act_on() {
        let (_tmp, server) = server(false);
        let (text, error) = call(&server, "get_fragment", json!({ "id": "01ZZZZ" }));
        assert!(error && text.contains("not found"), "{text}");

        let (text, error) = call(
            &server,
            "create_fragment",
            json!({ "type": "note", "title": "x", "content": "oops" }),
        );
        assert!(
            error && text.contains("unknown argument 'content'"),
            "{text}"
        );

        let (text, error) = call(
            &server,
            "create_fragment",
            json!({ "type": "todo", "title": "x", "fields": { "status": "someday" } }),
        );
        assert!(error, "{text}");

        let (text, _) = call(
            &server,
            "create_fragment",
            json!({ "type": "note", "title": "Draft" }),
        );
        let id = created_id(&text);
        let (text, error) = call(
            &server,
            "update_fragment",
            json!({ "id": id, "body": "new", "expected_revision": "0000000000000000" }),
        );
        assert!(error && text.contains("saved since"), "{text}");
        let (text, error) = call(
            &server,
            "update_fragment",
            json!({ "id": id, "body": "a", "append": "b" }),
        );
        assert!(error && text.contains("not both"), "{text}");
    }

    #[test]
    fn an_ambiguous_prefix_lists_its_matches() {
        let (_tmp, server) = server(false);
        let (a, _) = call(
            &server,
            "create_fragment",
            json!({ "type": "note", "title": "First" }),
        );
        let (b, _) = call(
            &server,
            "create_fragment",
            json!({ "type": "note", "title": "Second" }),
        );
        let (a, b) = (created_id(&a), created_id(&b));
        let common = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
        let (text, error) = call(&server, "get_fragment", json!({ "id": &a[..common] }));
        assert!(error, "{text}");
        assert!(text.contains(&a) && text.contains("First"), "{text}");
        assert!(text.contains(&b) && text.contains("Second"), "{text}");
    }

    #[test]
    fn list_types_shows_fields_and_allowed_values() {
        let (_tmp, server) = server(false);
        let (text, _) = call(&server, "list_types", json!({}));
        assert!(text.contains("todo (alias t)"), "{text}");
        assert!(
            text.contains("status: open | in-progress | done | cancelled"),
            "{text}"
        );
        assert!(!text.contains("turned off"), "{text}");

        let types = parc_core::config::TypesConfig {
            default: Some("note".into()),
            disabled: vec!["risk".into()],
        };
        parc_core::config::save_types(&_tmp.path().join(".parc"), &types).unwrap();
        let (text, _) = call(&server, "list_types", json!({}));
        assert!(text.contains("risk (alias r) [turned off"), "{text}");
        let (text, is_error) = call(
            &server,
            "create_fragment",
            json!({ "type": "risk", "title": "Disk fills up" }),
        );
        assert!(is_error && text.contains("turned off"), "{text}");
    }

    #[test]
    fn serve_frames_one_message_per_line() {
        let (_tmp, server) = server(false);
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n\n",
            "not json\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        );
        let mut output = Vec::new();
        serve(&server, &mut std::io::Cursor::new(input), &mut output).unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[1]["error"]["code"], -32700);
        assert_eq!(lines[2]["id"], 2);
    }
}
