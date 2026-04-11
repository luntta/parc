---
layout: layouts/doc.njk
title: JSON-RPC server
eyebrow: Reference · §05
---

`parc-server` is a JSON-RPC 2.0 server that wraps the `parc-core` library, enabling any non-Rust application to interact with parc programmatically. Twenty methods cover the full core API: fragment CRUD, search, links, attachments, vault inspection, schemas, tags, and history.

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
| `-32602` | Invalid params | Missing or wrong parameter types, unknown schema type |
| `-32603` | Internal error | Core library error (fragment not found, index error, etc.) |

Internal errors include `data` with the error message string.

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

Returns the full fragment object.

```json
{"jsonrpc":"2.0","id":1,"method":"fragment.create","params":{"type":"todo","title":"Review PRD","tags":["project"],"priority":"high","due":"2026-03-01"}}
```

### fragment.get

Retrieve a fragment by ID. Prefix matching supported.

**Params:** `{ "id": "<id-or-prefix>" }`

Returns the full fragment object with `body`, `tags`, `links`, `attachments`, and any extra fields.

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

Returns the updated fragment object.

### fragment.delete

Soft-delete a fragment (moves to trash).

**Params:** `{ "id": "<id-or-prefix>" }`

**Result:** `{ "id": "<full-id>", "deleted": true }`

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

Returns an array of search result objects with `id`, `type`, `title`, `status`, `tags`, `updated_at`, `snippet`.

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

Rebuild the search index from fragment files.

**Params:** `{}`

**Result:** `{ "indexed": <count> }`

### vault.doctor

Run vault health diagnostics.

**Params:** `{}`

**Result:** `{ "fragments_checked": <n>, "healthy": true|false, "findings": [...] }`

Each finding has a `type` field: `broken_link`, `orphan`, `schema_violation`, `attachment_mismatch`, or `vault_size_warning`.

### schema.list

List all registered fragment types.

**Params:** `{}`

**Result:** Array of `{ "name", "alias", "fields": [...] }`.

### schema.get

Get a specific schema definition.

**Params:** `{ "type": "<type-name-or-alias>" }`

**Result:** `{ "name", "alias", "editor_skip", "fields": [...] }`

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

**Result:** Fragment content at that version.

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
