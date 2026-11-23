---
layout: layouts/doc.njk
title: JSON-RPC server
eyebrow: Reference · §05
---

`parc-server` is a JSON-RPC 2.0 server that wraps the `parc-core` library, enabling any non-Rust application to interact with parc programmatically. Twenty-four methods cover the full core API: fragment CRUD, archive and trash, search, links, attachments, vault inspection, schemas, tags, and history.

## Transport

### stdio (default)

Newline-delimited JSON over stdin/stdout. Launch as a child process:

```bash
# Standalone binary
parc-server --vault /path/to/.parc

# Via CLI
parc server --vault /path/to/.parc
```

Send one JSON-RPC request per line. Read one JSON-RPC response per line. Flush after each write.

### Unix domain socket

Persistent server accepting multiple connections:

```bash
parc-server --vault /path/to/.parc --socket
parc-server --vault /path/to/.parc --socket-path /tmp/parc.sock
```

Default socket path: `<vault>/server.sock`. Same newline-delimited protocol per connection.

The socket is bound `0600` (owner-only) — the server itself has no auth, so the file mode is the access boundary. Anyone able to `connect()` to the socket can perform any operation the server can. Place the socket inside a directory only your user can traverse, and prefer the default `<vault>/server.sock` location over shared paths like `/tmp`.

### Server config

The vault's `config.yml` can set server defaults:

```yaml
server:
  transport: stdio    # "stdio" | "socket"
  socket_path: null   # override default socket location
```

CLI flags override config values.

## Protocol

All requests and responses follow [JSON-RPC 2.0](https://www.jsonrpc.org/specification).

```json
// Request
{"jsonrpc": "2.0", "id": 1, "method": "vault.info", "params": {}}

// Success response
{"jsonrpc": "2.0", "id": 1, "result": { "...": "..." }}

// Error response
{"jsonrpc": "2.0", "id": 1, "error": {"code": -32601, "message": "Method not found: foo"}}
```

Batch requests are supported — send a JSON array of requests, receive a JSON array of responses.

## Error codes

| Code | Name | Description |
|------|------|-------------|
| `-32700` | Parse error | Invalid JSON |
| `-32600` | Invalid Request | Missing `jsonrpc: "2.0"` or malformed structure |
| `-32601` | Method not found | Unknown method name |
| `-32602` | Invalid params | Missing or wrong parameter types, unknown schema type, a value the schema rejects, or a `pre-*` hook that refused the write |
| `-32603` | Internal error | Core library error (fragment not found, index error, etc.) |
| `-32010` | Conflict | `fragment.update` or `fragment.archive` named an `expected_revision` (or `expected_updated_at`) the fragment on disk no longer has. `data.updated_at` is the current one |

Internal errors include `data` with the error message string.

## Hooks and warnings

Write methods (`fragment.create`, `fragment.update`, `fragment.delete`, `fragment.link`, `fragment.unlink`) save through the same path as the CLI and TUI, so the vault's lifecycle hooks run exactly as they would for `parc` commands. A failing `post-*` hook doesn't fail the request; its message is added to the result object as `"warnings": ["..."]`. The field is omitted when there is nothing to report.

## Methods

### fragment.create

Create a new fragment.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | yes | Fragment type name or alias |
| `title` | string | no | Fragment title |
| `tags` | string[] | no | Tags |
| `body` | string | no | Markdown body |
| `links` | string[] | no | IDs to link to |
| `status` | string | no | Status value |
| `priority` | string | no | Priority level |
| `due` | string | no | Due date (ISO 8601 or relative: `today`, `tomorrow`, etc.) |
| `assignee` | string | no | Assignee |
| `fields` | object | no | Any field the type's schema declares, by name: `{"impact": "high"}`. See below |

Returns the full fragment object.

```json
{"jsonrpc":"2.0","id":1,"method":"fragment.create","params":{"type":"todo","title":"Review PRD","tags":["project"],"priority":"high","due":"2026-03-01"}}
```

### fragment.get

Retrieve a fragment by ID. Prefix matching supported.

**Params:** `{ "id": "<id-or-prefix>" }`

Returns the full fragment object with `body`, `tags`, `links`, `attachments`, and any extra fields.

Every fragment object returned by `fragment.get`, `fragment.create`, `fragment.update`, `fragment.archive` and `trash.restore` also carries `revision`: a fingerprint of the file's exact contents, which changes with every save. Send it back as `expected_revision` to make an update conditional.

### fragment.update

Update an existing fragment. Only provided fields are changed.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Fragment ID or prefix |
| `title` | string | no | New title |
| `tags` | string[] | no | Replace tags |
| `body` | string | no | Replace body |
| `links` | string[] | no | Replace links |
| `status` | string | no | New status |
| `priority` | string | no | New priority |
| `due` | string | no | New due date |
| `assignee` | string | no | New assignee |
| `fields` | object | no | Schema fields by name; `null` removes an optional field |
| `expected_revision` | string | no | The `revision` your copy was read at. If the file has changed since, the update is refused with `-32010` and nothing changes |
| `expected_updated_at` | string | no | The same check by `updated_at`, at whole-second precision: it can't see a second save within the same second. Prefer `expected_revision` |

Returns the updated fragment object.

`fields` works for every type, including custom ones: names must be fields the type's schema declares, enum values must be listed in the schema, and `date` fields take the same values as `due`. A required field can't be cleared. The named parameters (`status`, `due`, …) still work and apply first.

`expected_revision` lets an editor avoid overwriting a change made by another client since it loaded the fragment: send back the `revision` from `fragment.get` or from your previous write's result.

```json
{"jsonrpc":"2.0","id":2,"method":"fragment.update","params":{"id":"01JQ7V3X","fields":{"status":"done","due":null},"expected_revision":"5d1c0b2e9f3a7c41"}}
```

### fragment.delete

Soft-delete a fragment (moves to trash).

**Params:** `{ "id": "<id-or-prefix>" }`

**Result:** `{ "id": "<full-id>", "deleted": true }`

### trash.restore

Move a fragment back out of the trash and index it again: the undo of `fragment.delete`. Like `history.restore`, it runs no hooks.

**Params:** `{ "id": "<id-or-prefix>" }` (matched against trashed fragments only)

**Result:** The restored fragment object, with `revision`.

### fragment.archive

Archive or unarchive a fragment. Archived fragments stay in the vault but leave default searches; `is:archived` or `is:all` finds them.

**Params:** `{ "id": "<id-or-prefix>", "archived": true|false, "expected_revision": "<optional>" }`

**Result:** The updated fragment object (`"archived": true` while archived).

### fragment.list

List fragments with optional filters.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | no | Filter by type |
| `status` | string | no | Filter by status |
| `tag` | string | no | Filter by tag |
| `limit` | number | no | Max results |
| `sort` | string | no | Sort order |

Returns an array of fragment summary objects.

### fragment.search

Search using the full DSL query language.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `query` | string | yes | DSL query (e.g. `type:todo status:open #backend`) |
| `limit` | number | no | Max results |
| `sort` | string | no | Sort order |

Returns an array of search result objects with `id`, `type`, `title`, `status`, `tags`, `updated_at`, `created_at`, `snippet`, `archived`.

Words are matched as text even when they contain punctuation (`half-baked`, `parc_core::ops`); a word ending in `*` matches by prefix.

### fragment.grep

Find the lines of fragment bodies that contain some text, like `grep`. The desktop's Go to… uses it for its `/` mode.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `pattern` | string | yes | Text to find, at most 1000 characters. It is matched literally (no regular expressions), ignoring case unless it contains an uppercase letter |
| `limit` | number | no | Most matching lines to return (default 200, at most 1000) |

**Result:** `{ "matches": [...], "truncated": <bool> }`. Each match is a fragment, most recently edited first, archived ones included: `{ "id", "type", "title", "status", "archived", "updated_at", "lines": [...] }`. Each line is `{ "line", "column", "text", "ranges" }`: `line` is 1-based within the body as an editor shows it (the blank line after the frontmatter isn't counted), `column` is the 1-based character column of the line's first match, `text` is the whole line, and `ranges` lists every match in it as `[start, end)` character offsets. `truncated` is true when more lines matched than were returned.

```json
{"jsonrpc":"2.0","id":1,"method":"fragment.grep","params":{"pattern":"ferry","limit":50}}
```

### fragment.link

Create a bidirectional link between two fragments.

**Params:** `{ "id_a": "<id>", "id_b": "<id>" }`

**Result:** `{ "linked": ["<id_a>", "<id_b>"] }`

### fragment.unlink

Remove a bidirectional link.

**Params:** `{ "id_a": "<id>", "id_b": "<id>" }`

**Result:** `{ "unlinked": ["<id_a>", "<id_b>"] }`

### fragment.backlinks

List all fragments linking to a given fragment.

**Params:** `{ "id": "<id-or-prefix>" }`

**Result:** Array of `{ "id", "type", "title" }`.

### fragment.attach

Attach a file to a fragment (copies the file).

**Params:** `{ "id": "<id-or-prefix>", "path": "/absolute/path/to/file" }`

**Result:** `{ "id": "<full-id>", "filename": "<name>", "size": <bytes> }`

### fragment.detach

Remove an attachment from a fragment.

**Params:** `{ "id": "<id-or-prefix>", "filename": "<name>" }`

**Result:** `{ "id": "<full-id>", "filename": "<name>", "detached": true }`

### fragment.attachments

List attachments for a fragment.

**Params:** `{ "id": "<id-or-prefix>" }`

**Result:** Array of `{ "filename", "size" }`.

### vault.info

Get vault metadata.

**Params:** `{}`

**Result:** `{ "path": "<vault-path>", "scope": "local"|"global", "fragment_count": <n> }`

### vault.reindex

Rebuild the search index from fragment files, in one transaction. Rarely needed: every method reads an index that has just been brought up to date with the files, so edits made outside the server (an editor, the CLI, `git pull`) are visible to the next request.

**Params:** `{}`

**Result:** `{ "indexed": <count>, "problems": [{ "file": "fragments/<name>.md", "reason": "<why it could not be indexed>" }] }`

### vault.links

Every link in the index, for drawing the vault as a graph in one request. Links stored as ID prefixes are resolved; links to missing or ambiguous targets are left out.

**Params:** `{}`

**Result:** Array of `{ "source": "<full-id>", "target": "<full-id>" }`. Direction is the direction written; `fragment.link` writes both.

### vault.doctor

Run vault health diagnostics.

**Params:** `{}`

**Result:** `{ "fragments_checked": <n>, "healthy": true|false, "findings": [...] }`

Each finding has a `type` field: `broken_link`, `ambiguous_link`, `orphan`, `schema_violation`, `attachment_mismatch`, `vault_size_warning`, `plugin_issue`, or `unreadable_fragment` (a file in `fragments/` that can't be read as a fragment, with `file` and `reason`; it is not in the index).

### schema.list

List all registered fragment types.

**Params:** `{}`

**Result:** Array of `{ "name", "alias", "editor_skip", "fields": [...], "toggle", "enabled", "default", "builtin" }`.

Each field is `{ "name", "kind", "values", "required", "default", "toggle", "categories", "type" }`: `kind` is `string`, `date`, `enum` or `list`; `values` lists an enum's values; `toggle` is the field's `[off, on]` pair if it declares one. On the `status` field, `categories` maps every value to what it means (`open`, `active`, `done` or `closed`), with the built-in meanings filled in when the schema declares none; other fields have `null`. The schema's `toggle` is `{ "field", "off", "on" }` for the one-key toggle (the TUI's `t`), or `null`. `type` is an older spelling of the field type kept for existing clients.

`enabled` is `false` for a type turned off in the vault's settings: `fragment.create` refuses it, but its fragments read and update as usual. `default` marks the type captures start as; `builtin` marks the five types parc ships.

### schema.get

Get a specific schema definition.

**Params:** `{ "type": "<type-name-or-alias>" }`

**Result:** One schema, as in `schema.list`.

### schema.builtin

The definition parc ships for a built-in type (note, todo, decision, risk, idea), to compare with or restore over the vault's copy.

**Params:** `{ "type": "<type-name>" }`

**Result:** One schema, as in `schema.list`.

### schema.usage

How a type is used: its fragments, and how many hold each value of each of its enum fields.

**Params:** `{ "type": "<type-name>" }`

**Result:** `{ "total": <n>, "values": { "<field>": { "<value>": <n> } } }`

### schema.save

Create a type, or change one and bring its fragments along.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `schema` | object | yes | `{ "name", "alias"?, "editor_skip"?, "fields": [...] }`. Each field is `{ "name", "kind", "values"?, "required"?, "default"?, "toggle"?, "categories"? }`, as `schema.list` returns them |
| `create` | boolean | no | `true` for a new type (refused if the name exists); `false` (default) to change an existing one |
| `renames` | array | no | `[{ "field", "from", "to" }]`: move fragments holding `from` in an enum field to `to`. Use it for a renamed value, or to say where a removed value's fragments go |

The schema is checked strictly (names, values, defaults, toggle and categories, and that the name and alias don't collide with another type's). The change is refused with nothing written if it would leave a fragment that is valid now invalid; the error names the first few. Moved fragments keep their `updated_at` and go through the usual save, hooks included.

**Result:** `{ "schema": <as in schema.list>, "migrated": <n>, "warnings"?: [...] }`

Send the whole definition: a field left out of `fields` is removed from the type. This renames a bug status, moving the fragments that had it:

```json
{"jsonrpc":"2.0","id":1,"method":"schema.save","params":{"schema":{"name":"bug","alias":"b","fields":[{"name":"status","kind":"enum","values":["new","fixing","fixed"],"required":true,"default":"new","toggle":["new","fixed"],"categories":{"fixing":"active","fixed":"done"}}]},"renames":[{"field":"status","from":"triaged","to":"fixing"}]}}
```

### schema.delete

Remove a type that no fragment uses. A type with fragments can be turned off instead.

**Params:** `{ "type": "<type-name>" }`

**Result:** `{ "deleted": true }`

### settings.get

The vault's settings from `config.yml`: which types are on.

**Params:** `{}`

**Result:** `{ "types": { "default": "<type>", "disabled": ["<type>", ...] } }`. `default` is the type captures start as.

### settings.update

Change the settings given and keep the others. Only `types` can be changed here; hook and plugin switches are not reachable over RPC.

**Params:** `{ "types": { "default"?: "<type>", "disabled"?: ["<type>", ...] } }`. The default must be a type that is on, and at least one type stays on. Only the `types:` block of `config.yml` is rewritten.

**Result:** As `settings.get`.

### tags.list

List all tags with usage counts.

**Params:** `{}`

**Result:** Array of `{ "tag": "<name>", "count": <n> }`.

### history.list

List version history for a fragment.

**Params:** `{ "id": "<id-or-prefix>" }`

**Result:** Array of `{ "timestamp": "<iso-8601>", "size": <bytes> }`.

### history.get

Retrieve a specific historical version.

**Params:** `{ "id": "<id-or-prefix>", "timestamp": "<iso-8601>" }`

**Result:** The fragment as it was at that version, in the same shape as `fragment.get` (schema fields included, no `revision`).

### history.restore

Restore a previous version. Creates a new snapshot of the current version first.

**Params:** `{ "id": "<id-or-prefix>", "timestamp": "<iso-8601>" }`

**Result:** `{ "id", "type", "title", "restored_from": "<timestamp>" }`

## Integration examples

### Node.js / TypeScript

```typescript
import { spawn } from 'child_process';
import * as readline from 'readline';

const server = spawn('parc-server', ['--vault', '/path/to/.parc']);
const rl = readline.createInterface({ input: server.stdout });

let nextId = 1;
const pending = new Map<number, { resolve: Function; reject: Function }>();

rl.on('line', (line) => {
  const resp = JSON.parse(line);
  const p = pending.get(resp.id);
  if (p) {
    pending.delete(resp.id);
    if (resp.error) p.reject(resp.error);
    else p.resolve(resp.result);
  }
});

function call(method: string, params: any): Promise<any> {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    server.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
  });
}

// Usage
const note = await call('fragment.create', { type: 'note', title: 'Hello', body: 'World' });
const results = await call('fragment.search', { query: 'type:note' });
```

### Python

```python
import subprocess, json

proc = subprocess.Popen(
    ['parc-server', '--vault', '/path/to/.parc'],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE,
    text=True, bufsize=1,
)

def call(method, params, id=1):
    req = json.dumps({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    proc.stdin.write(req + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    return json.loads(line)

# Usage
resp = call("fragment.create", {"type": "todo", "title": "Test", "priority": "high"})
print(resp["result"]["id"])

resp = call("fragment.search", {"query": "type:todo status:open"}, id=2)
print(f"Found {len(resp['result'])} todos")
```
