---
layout: layouts/doc.njk
title: MCP server
eyebrow: Reference · §07
---

`parc mcp` serves a vault to AI agents over the [Model Context Protocol](https://modelcontextprotocol.io): Claude Code, Codex, editors and other MCP clients can search the vault, read fragments, and record notes, todos, decisions, risks and ideas, without shelling out to the CLI.

Writes go through the same path as every other parc frontend: schema validation, vault hooks, a history snapshot, and the index. An agent's save is indistinguishable from yours, and every earlier version stays in the fragment's history.

## Setup

The server speaks MCP over stdio. It finds the vault the way the CLI does (`--vault`, `PARC_VAULT`, a `.parc/` in the working directory or a parent, then `~/.parc`), so a client started in a project directory gets that project's vault.

**Claude Code**

```bash
claude mcp add parc -- parc mcp                  # this project's vault
claude mcp add parc-personal -- parc -g mcp      # your global ~/.parc vault
```

**Codex** (`~/.codex/config.toml`)

```toml
[mcp_servers.parc]
command = "parc"
args = ["mcp"]
```

**Other clients** use the common JSON form:

```json
{ "mcpServers": { "parc": { "command": "parc", "args": ["mcp"] } } }
```

`parc-server --mcp --vault <path>` is the same server from the standalone binary.

Writes can run enabled hooks only after you review the vault and explicitly approve it with `parc vault trust`.

## Tools

| Tool | Does |
|------|------|
| `search` | Full-text search plus the [search DSL]({{ '/search-dsl/' | url }}) (`type:todo status:open #backend due:this-week`). An empty query lists recent fragments. |
| `recall` | The fragments related to a saved fragment (`id`) or to text being written (`text`), however old, each with why it matched, its revision and short verbatim excerpts with line numbers. With the [meaning model]({{ '/cli/search/' | url }}#the-meaning-model) installed, it also finds notes that say the same thing in other words or another language, and says when the model isn't there. No results means nothing is clearly related. |
| `get_fragment` | One fragment in full: its Markdown file, its revision, and what links to it. |
| `list_types` | Each type's fields, allowed values and defaults. |
| `list_tags` | Tags in use, most used first. |
| `create_fragment` | A new fragment of any type, with title, body, tags, links and type-specific `fields`. |
| `update_fragment` | Change title, body, tags or fields. `append` adds to the end of the body instead of replacing it. Pass `expected_revision` (from `get_fragment`) to refuse the update if the fragment was saved since; `append` does this by itself. |
| `link_fragments` | Link two fragments in both directions. |

Results are plain text written for a model to read, with full IDs. Any unique ID prefix is accepted; an ambiguous one lists what it matches. A tool that fails (an unknown ID, a value the schema doesn't allow, a stale revision) returns the reason as an error result the agent can act on.

On connect the server sends brief instructions: search before starting on a topic and before creating, record durable context (decisions, risks, follow-ups) rather than routine progress, and never write secrets into fragments.

## Read-only mode

```bash
parc mcp --read-only
```

Offers only `search`, `recall`, `get_fragment`, `read_attachment`, `list_types` and `list_tags`. This mode uses a private in-memory index and starts no background workers. Reading an uncached attachment extracts its text without writing it back to the vault.

## What it leaves out

There is no delete, archive, attachment-management or history tool: those stay with you, in the CLI, the TUI and the desktop app.
